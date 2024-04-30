#![feature(iter_collect_into)]
#![feature(let_chains)]
#![allow(non_snake_case)]
#![feature(mem_copy_fn)]
#![feature(macro_metavar_expr)]
#![feature(slice_take)]
#![allow(clippy::multiple_crate_versions, clippy::future_not_send)]
#![allow(clippy::empty_docs)]

use {
    crate::{
        chat::Chat,
        login::{Login, Register},
        profile::Profile,
    },
    anyhow::Context,
    chain_api::Nonce,
    chat_client_node::{
        encode_direct_chat_name, BootPhase, FriendMessage, MailVariants, Node, NodeHandle,
        RequestContext, Sub, UserKeys, Vault, VaultComponentId,
    },
    chat_spec::{ChatName, Topic},
    codec::{Decode, ReminderOwned},
    leptos::*,
    leptos_router::{Route, Router, Routes, A},
    libp2p::futures::{FutureExt, StreamExt},
    std::{cmp::Ordering, convert::identity, fmt::Display, future::Future, time::Duration},
    web_sys::wasm_bindgen::JsValue,
};

mod chat;
mod db;
mod file_upload;
mod login;
mod profile;

pub fn main() {
    console_error_panic_hook::set_once();
    _ = console_log::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Error
    });

    mount_to_body(App)
}

#[derive(Default, Clone, Copy)]
struct State {
    keys: RwSignal<Option<UserKeys>>,
    requests: StoredValue<Option<NodeHandle>>,
    vault: RwSignal<Vault>,
    vault_version: StoredValue<Nonce>,
    mail_action: StoredValue<Nonce>,
    friend_messages: RwSignal<Option<(ChatName, db::Message)>>,
}

type Result<T> = anyhow::Result<T>;

fn rc_error(name: &'static str) -> impl FnOnce() -> String {
    move || format!("{name} not available, might need to reload")
}

impl RequestContext for State {
    fn try_with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> Result<R>) -> Result<R> {
        self.vault.try_update(action).with_context(rc_error("vault")).and_then(identity)
    }

    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> Result<R> {
        self.vault.try_update(action).with_context(rc_error("vault"))
    }

    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> Result<R> {
        self.keys
            .try_with_untracked(|keys| keys.as_ref().map(action))
            .flatten()
            .with_context(rc_error("keys"))
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R> {
        self.vault_version.try_update_value(action).with_context(rc_error("vault version"))
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R> {
        self.mail_action.try_update_value(action).with_context(rc_error("mail action"))
    }

    fn subscription_for<T: Into<Topic> + Clone>(
        &self,
        t: T,
    ) -> impl Future<Output = Result<Sub<T>>> {
        let fut = self
            .requests
            .try_with_value(|r| r.as_ref().map(|r| r.subscription_for(t)))
            .flatten()
            .context("dispatch closed");
        async { fut?.await.map_err(Into::into) }
    }
}

impl State {
    fn set_chat_nonce(&self, chat: ChatName, nonce: Nonce) {
        self.vault.update(|v| {
            let chat = v.chats.get_mut(&chat).expect("chat not found");
            chat.action_no = nonce;
        });
    }

    fn chat_secret(self, chat_name: ChatName) -> Option<crypto::SharedSecret> {
        self.vault.with_untracked(|vault| vault.chats.get(&chat_name).map(|c| c.secret))
    }
}

fn App() -> impl IntoView {
    let (rboot_phase, wboot_phase) = create_signal(None::<BootPhase>);
    let (errors, set_errors) = create_signal(None::<anyhow::Error>);
    provide_context(Errors(set_errors));
    let state = State::default();

    async fn handle_mail(
        mail: MailVariants,
        new_messages: &mut Vec<db::Message>,
        vault_updates: &mut Vec<VaultComponentId>,
        state: State,
    ) -> Result<()> {
        let mut messages = Vec::new();
        mail.handle(&state, vault_updates, &mut messages).await?;
        if let Some((sender, FriendMessage::DirectMessage { content })) = messages.pop() {
            let message = db::Message {
                sender,
                owner: state.with_keys(|k| k.name)?,
                content,
                chat: encode_direct_chat_name(sender),
            };
            new_messages.push(message.clone());
            state.friend_messages.set(Some((sender, message)));
        }
        Ok(())
    }

    create_effect(move |_| {
        let Some(keys) = state.keys.get() else {
            return;
        };

        handled_spawn_local("initializing node", async move {
            let identity = keys.identity();
            navigate_to("/");
            let (node, vault, dispatch, vault_version, mail_action) =
                Node::new(keys, |s| wboot_phase(Some(s)))
                    .await
                    .inspect_err(|_| navigate_to("/login"))?;

            let listen = async move {
                let mut vault_updates = Vec::new();
                let mut new_messages = Vec::new();
                loop {
                    let ReminderOwned(list) = state.read_mail().await?;
                    for mail in chat_spec::unpack_mail(&list) {
                        let mail = MailVariants::decode(&mut &*mail)
                            .with_context(|| format!("cant decode mail: {mail:?}"))?;
                        handle_error(
                            handle_mail(mail, &mut new_messages, &mut vault_updates, state).await,
                        );
                    }
                    db::save_messages(&new_messages).await?;
                    vault_updates.sort_unstable();
                    vault_updates.dedup();
                    state.save_vault_components(vault_updates.drain(..)).await?;

                    let mut profile_sub = state.subscription_for(identity).await?;
                    let mut account =
                        profile_sub.subscribe().await.context("subscribin to account")?;
                    while let Some(mail) = account.next().await {
                        let task = async {
                            handle_mail(mail, &mut new_messages, &mut vault_updates, state).await?;
                            db::save_messages(&new_messages).await?;
                            state.save_vault_components(vault_updates.drain(..)).await
                        };
                        handle_error(task.await);
                        new_messages.clear();
                    }
                }
            };

            state.requests.set_value(Some(dispatch));
            state.vault.set_untracked(vault);
            state.vault_version.set_value(vault_version);
            state.mail_action.set_value(mail_action);
            navigate_to("/chat");

            let res = libp2p::futures::select! {
                _ = node.fuse() => return Ok(()),
                e = listen.fuse() => e,
            };
            navigate_to("/login");
            res
        });
    });

    let chat = move || view! { <Chat state/> };
    let profile = move || view! { <Profile state/> };
    let login = move || view! { <Login state/> };
    let register = move || view! { <Register state/> };
    let boot = move || view! { <Boot rboot_phase/> };

    view! {
        <Router>
        <Routes>
            <Route path="/chat/:id?" view=chat></Route>
            <Route path="/profile" view=profile></Route>
            <Route path="/login" view=login></Route>
            <Route path="/register" view=register></Route>
            <Route path="/" view=boot></Route>
        </Routes>
        </Router>
        <ErrorPanel errors/>
    }
}

#[component]
fn ErrorPanel(errors: ReadSignal<Option<anyhow::Error>>) -> impl IntoView {
    let error_nodes = create_node_ref::<html::Div>();
    let error_message = move |message: String| {
        let elem = view! {
            <div class="tbm" onclick="this.remove()">
                <div class="ec hov bp pea" style="cursor: pointer">{message}</div>
            </div>
        };

        let celem = elem.clone();
        set_timeout(move || celem.remove(), Duration::from_secs(7));

        elem
    };

    create_effect(move |_| {
        errors.with(|e| {
            if let Some(e) = e {
                log::error!("{e:#}");
                error_nodes
                    .get_untracked()
                    .unwrap()
                    .append_child(&error_message(format!("{e:#}")))
                    .unwrap();
            }
        });
    });

    view! {
        <div class="fsc jcfe flx pen">
            <div class="bm" style="align-self: flex-end;" node_ref=error_nodes />
        </div>
    }
}

#[component]
fn Boot(rboot_phase: ReadSignal<Option<BootPhase>>) -> impl IntoView {
    let phases = (0..BootPhase::ChatRun.discriminant())
        .map(|i| {
            let margin = if i == 0 { "" } else { "lbm" };
            let compute_class = move || {
                rboot_phase.with(|phase| match phase.map(|p| i.cmp(&(p.discriminant()))) {
                    Some(Ordering::Less) => "bar-loaded",
                    Some(Ordering::Equal) => "bar-loading",
                    Some(Ordering::Greater) => "bar-unloaded",
                    None => "",
                })
            };
            view! { <span class=move || format!("bp hc fg1 tac {} {margin}", compute_class()) /> }
        })
        .collect_view();

    let message = move || match rboot_phase() {
        Some(s) => format!("{s}"),
        None => {
            navigate_to("/login");
            "confused".to_string()
        }
    };

    view! {
        <main class="ma sc bp">
            <h1>Initiating connections</h1>
            <p>{message}</p>
            <div class="flx bp pc">
                {phases}
            </div>
        </main>
    }
}

#[component]
fn Nav(keys: RwSignal<Option<UserKeys>>) -> impl IntoView {
    let menu = create_node_ref::<html::Div>();
    let balance = create_rw_signal(0u128);

    let on_menu_toggle = move |_| {
        let menu = menu.get_untracked().unwrap();
        menu.set_hidden(!menu.hidden());
    };
    let reload_balance = handled_async_closure("reloading balance", move || async move {
        let keys = keys.get_untracked().context("no keys")?;
        let b = keys.chain_client().await?.get_balance().await?;
        balance.set(b);
        Ok(())
    });

    reload_balance();

    let uname = move || keys.get_untracked().as_ref().map_or("", |k| &k.name).to_owned();
    let balance = move || chain_api::format_balance(balance());

    Some(view! {
        <nav class="sc flx fdc fg0 phone-only">
            <div class="flx jcsb">
                <button class="rsb hov sc nav-menu-button" on:click=on_menu_toggle>/menu</button>
                <div class="bp bf hc lsb">{uname}</div>
            </div>
            <div class="flx fdc tsm" hidden node_ref=menu>
                <A class="bf hov bp sc bsb" href="/chat">/rooms</A>
                <A class="bf hov bp sc sb" href="/profile">/profile</A>
                <A class="bf hov bp sc tsb" href="/login">/logout</A>
            </div>
        </nav>

        <nav class="sc flx jcsb fg0 desktop-only">
            <div class="flx">
                <A class="bf hov bp sc rsb" href="/chat">/rooms</A>
                <A class="bf hov bp sc sb" href="/profile">/profile</A>
                <A class="bf hov bp sc sb" href="/login">/logout</A>
            </div>
            <div class="bp bf hc" on:click=move |_| reload_balance()>{balance}</div>
            <div class="bp bf hc lsb">{uname}</div>
        </nav>
    })
}

fn get_value(elem: NodeRef<html::Input>) -> String {
    elem.get_untracked().unwrap().value()
}

fn navigate_to(path: impl Display) {
    leptos_router::use_navigate()(&format!("{path}"), Default::default());
}

fn not(signal: impl Fn() -> bool + Copy) -> impl Fn() -> bool + Copy {
    move || !signal()
}

fn handle_js_err(jv: JsValue) -> anyhow::Error {
    anyhow::anyhow!("{jv:?}")
}

#[derive(Clone, Copy)]
struct Errors(WriteSignal<Option<anyhow::Error>>);

fn handle_error<T>(r: anyhow::Result<T>) -> Option<T> {
    let Errors(errors) = use_context().unwrap();
    r.map_err(|e| errors.set(Some(e))).ok()
}

fn _handled_closure(
    context: &'static str,
    f: impl Fn() -> anyhow::Result<()> + 'static + Copy,
) -> impl Fn() + 'static + Copy {
    let Errors(errors) = use_context().unwrap();
    move || {
        if let Err(e) = f().context(context) {
            errors.set(Some(e));
        }
    }
}

fn handled_callback<T>(
    context: &'static str,
    f: impl Fn(T) -> anyhow::Result<()> + 'static + Copy,
) -> impl Fn(T) + 'static + Copy {
    let Errors(errors) = use_context().unwrap();
    move |v| {
        if let Err(e) = f(v).context(context) {
            errors.set(Some(e));
        }
    }
}

fn handled_async_closure<F: Future<Output = anyhow::Result<()>> + 'static>(
    context: &'static str,
    f: impl Fn() -> F + 'static + Copy,
) -> impl Fn() + 'static + Copy {
    let Errors(errors) = use_context().unwrap();
    move || {
        let fut = f();
        spawn_local(async move {
            if let Err(e) = fut.await.context(context) {
                errors.set(Some(e));
            }
        });
    }
}

fn handled_async_callback<F: Future<Output = anyhow::Result<()>> + 'static, T>(
    context: &'static str,
    f: impl Fn(T) -> F + 'static + Copy,
) -> impl Fn(T) + 'static + Copy {
    let Errors(errors) = use_context().unwrap();
    move |v| {
        let fut = f(v);
        spawn_local(async move {
            if let Err(e) = fut.await.context(context) {
                errors.set(Some(e));
            }
        });
    }
}

fn handled_spawn_local(
    context: &'static str,
    f: impl Future<Output = anyhow::Result<()>> + 'static,
) {
    let Errors(errors) = use_context().unwrap();
    spawn_local(async move {
        if let Err(e) = f.await.context(context) {
            errors.set(Some(e));
        }
    });
}
