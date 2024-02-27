use {
    crate::{
        db, handled_async_closure, handled_callback, handled_spawn_local,
        node::{self, HardenedChatInvitePayload, Mail, MessageContent, RawChatMessage},
        protocol::SubsOwner,
    },
    anyhow::Context,
    chat_spec::{
        username_to_raw, ChatEvent, ChatName, CreateChat, FetchMessages, FetchProfile, UserName,
    },
    component_utils::{Codec, DropFn, Reminder},
    crypto::{
        enc::{self},
        TransmutationCircle,
    },
    leptos::{
        html::{Input, Textarea},
        *,
    },
    leptos_router::Redirect,
    libp2p::futures::{future::join_all, FutureExt},
    rand::{rngs::OsRng, Rng},
    std::{
        future::Future,
        ops::Deref,
        sync::atomic::{AtomicBool, Ordering},
    },
    wasm_bindgen_futures::wasm_bindgen::JsValue,
};

fn is_at_bottom(messages_div: HtmlElement<leptos::html::Div>) -> bool {
    let scroll_bottom = messages_div.scroll_top();
    let scroll_height = messages_div.scroll_height();
    let client_height = messages_div.client_height();

    let prediction = 200;
    scroll_height - client_height <= scroll_bottom + prediction
}

fn resize_input(mi: HtmlElement<Textarea>) -> Result<(), JsValue> {
    let outher_height = window()
        .get_computed_style(&mi)?
        .ok_or("element does not have computed stile")?
        .get_property_value("height")?
        .strip_suffix("px")
        .ok_or("height is not in pixels")?
        .parse::<i32>()
        .map_err(|e| format!("height is not a number: {e}"))?;
    let diff = outher_height - mi.client_height();
    mi.deref().style().set_property("height", "0px")?;
    mi.deref().style().set_property("height", format!("{}px", mi.scroll_height() + diff).as_str())
}

#[leptos::component]
pub fn Chat(state: crate::State) -> impl IntoView {
    let Some(keys) = state.keys.get_untracked() else {
        return view! { <Redirect path="/login"/> }.into_view();
    };

    let my_name = keys.name;
    let my_id = keys.identity_hash();
    let my_enc = keys.enc.into_bytes();
    let requests = move || state.requests.get_value().unwrap();
    let selected: Option<ChatName> = leptos_router::use_query_map()
        .with_untracked(|m| m.get("id").and_then(|v| v.as_str().try_into().ok()))
        .filter(|v| state.vault.with_untracked(|vl| vl.chats.contains_key(v)));

    #[derive(Clone)]
    enum Cursor {
        Normal(chat_spec::Cursor),
        Hardened(db::MessageCursor),
    }

    let (current_chat, set_current_chat) = create_signal(selected);
    let (show_chat, set_show_chat) = create_signal(false);
    let (is_hardened, set_is_hardened) = create_signal(false);
    let messages = create_node_ref::<leptos::html::Div>();
    let (cursor, set_cursor) = create_signal(Cursor::Normal(chat_spec::Cursor::INIT));
    let (red_all_messages, set_red_all_messages) = create_signal(false);

    let hide_chat = move |_| set_show_chat(false);
    let message_view = move |username: UserName, content: MessageContent| {
        let my_message = my_name == username;
        let justify = if my_message { "right" } else { "left" };
        let color = if my_message { "hc" } else { "pc" };
        view! {
            <div class="tbm flx" style=("justify-content", justify)>
                <div class=format!("bp flx {color}")>
                    <div class=color>{username.to_string()}:</div>
                    <div class=format!("lbp {color}")>{content}</div>
                </div>
            </div>
        }
    };
    let append_message = move |username: UserName, content: MessageContent| {
        let messages = messages.get_untracked().expect("layout invariants");
        let message = message_view(username, content);
        messages.append_child(&message).unwrap();
    };
    let prepend_message = move |username: UserName, content: MessageContent| {
        let messages = messages.get_untracked().expect("layout invariants");
        let message = message_view(username, content);
        messages
            .insert_before(&message, messages.first_child().as_ref())
            .expect("layout invariants");
    };

    let fetch_messages = handled_async_closure("fetching mesages", move || async move {
        static FETCH_LOCK: AtomicBool = AtomicBool::new(false);
        if FETCH_LOCK.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        DropFn::new(|| FETCH_LOCK.store(false, Ordering::Relaxed));

        let mut requests = requests();

        let Some(chat) = current_chat.get_untracked() else {
            return Ok(());
        };

        if red_all_messages.get_untracked() {
            return Ok(());
        }

        if is_hardened.get_untracked() && matches!(cursor.get_untracked(), Cursor::Normal(_)) {
            let cursor =
                db::MessageCursor::new(chat, my_name).await.context("opening message cursor")?;
            set_cursor(Cursor::Hardened(cursor));
        }

        match cursor.get_untracked() {
            Cursor::Normal(cursor) => {
                let (new_cursor, Reminder(messages)) =
                    requests.dispatch::<FetchMessages>((chat, cursor)).await?;
                let secret = state.chat_secret(chat).context("getting chat secret")?;
                for message in chat_spec::unpack_messages(messages.to_vec().as_mut_slice()) {
                    let Some(chat_spec::Message { content: Reminder(content), .. }) =
                        <_>::decode(&mut &*message)
                    else {
                        log::error!("server gave us undecodable message");
                        continue;
                    };

                    let mut message = content.to_vec();
                    let Some(decrypted) = crypto::decrypt(&mut message, secret) else {
                        log::error!("message from server is corrupted");
                        continue;
                    };
                    let Some(RawChatMessage { sender, content }) =
                        RawChatMessage::decode(&mut &*decrypted)
                    else {
                        log::error!("failed to decode fetched message");
                        continue;
                    };
                    prepend_message(sender, content.into());
                }
                set_red_all_messages(new_cursor == chat_spec::Cursor::INIT);
                set_cursor(Cursor::Normal(new_cursor));
            }
            Cursor::Hardened(cursor) => {
                let messages = cursor.next(30).await?;
                if messages.is_empty() {
                    set_red_all_messages(true);
                }
                for message in messages.into_iter() {
                    prepend_message(message.sender, message.content);
                }
            }
        }

        Ok(())
    });

    let subscription_owner = store_value(None::<SubsOwner<ChatName>>);
    create_effect(move |_| {
        let Some(chat) = current_chat() else {
            return;
        };

        if is_hardened.get_untracked() {
            return;
        }

        let Some(secret) = state.chat_secret(chat) else {
            log::warn!("received message for chat we are not part of");
            return;
        };

        handled_spawn_local("reading chat messages", async move {
            let (mut sub, owner) = requests().subscribe(chat)?;
            log::info!("subscribed to chat: {:?}", chat);
            subscription_owner.set_value(Some(owner)); // drop old subscription
            while let Some(ChatEvent::Message(proof, Reminder(message))) = sub.next().await {
                if !proof.verify() {
                    log::warn!("received message with invalid proof");
                    continue;
                }

                let mut message = message.to_vec();
                let Some(message) = crypto::decrypt(&mut message, secret) else {
                    log::warn!("message cannot be decrypted: {:?} {:?}", message, secret);
                    continue;
                };

                let Some(RawChatMessage { sender, content }) =
                    RawChatMessage::decode(&mut &*message)
                else {
                    log::warn!("message cannot be decoded: {:?}", message);
                    continue;
                };

                append_message(sender, content.into());
                log::info!("received message: {:?}", content);
            }

            Ok(())
        })
    });

    create_effect(move |_| {
        state.hardened_messages.track();
        let Some(Some((chat, message))) =
            state.hardened_messages.try_update_untracked(|m| m.take())
        else {
            return;
        };

        if current_chat.get_untracked() != Some(chat) {
            log::debug!("received message for chat we are not currently in");
            return;
        };

        append_message(message.sender, message.content);
    });

    let side_chat = move |chat: ChatName, hardened: bool| {
        let select_chat = move |_| {
            set_show_chat(true);
            set_red_all_messages(false);
            set_cursor(Cursor::Normal(chat_spec::Cursor::INIT));
            set_current_chat(Some(chat));
            set_is_hardened(hardened);
            crate::navigate_to(format_args!("/chat/{chat}"));
            let messages = messages.get_untracked().expect("universe to work");
            messages.set_inner_html("");
            fetch_messages();
        };
        let selected = move || current_chat.get() == Some(chat);
        view! { <div class="sb tac bp toe" class:hc=selected class:hov=crate::not(selected) on:click=select_chat> {chat.to_string()} </div> }
    };

    let (create_normal_chat_button, create_normal_chat_poppup) = popup(
        PoppupStyle {
            placeholder: "chat name...",
            button_style: "hov sf pc lsm",
            button: "+",
            confirm: "create",
            maxlength: 32,
        },
        move |chat| async move {
            let Ok(chat) = ChatName::try_from(chat.as_str()) else {
                anyhow::bail!("invalid chat name");
            };

            if state.vault.with_untracked(|v| v.chats.contains_key(&chat)) {
                anyhow::bail!("chat already exists");
            }

            requests().dispatch::<CreateChat>((chat, my_id)).await.context("creating chat")?;

            let meta = node::ChatMeta::new();
            state.vault.update(|v| _ = v.chats.insert(chat, meta));

            Ok(())
        },
    );

    let (create_hardened_chat_button, create_hardened_chat_poppup) = popup(
        PoppupStyle {
            placeholder: "hardened chat name...",
            button_style: "hov sf pc lsm",
            button: "+",
            confirm: "create",
            maxlength: 32,
        },
        move |chat| async move {
            let Ok(chat) = ChatName::try_from(chat.as_str()) else {
                anyhow::bail!("invalid chat name");
            };

            if state.vault.with_untracked(|v| v.hardened_chats.contains_key(&chat)) {
                anyhow::bail!("chat already exists");
            }

            state.vault.update(|v| _ = v.hardened_chats.insert(chat, Default::default()));

            Ok(())
        },
    );

    let add_normal_user = move |name: String| async move {
        let Ok(name) = UserName::try_from(name.as_str()) else {
            anyhow::bail!("invalid user name");
        };

        let Some(chat) = current_chat.get_untracked() else {
            anyhow::bail!("no chat selected");
        };

        let client = crate::chain::node(my_name).await?;
        let invitee = match client
            .get_profile_by_name(crate::chain::user_contract(), username_to_raw(name))
            .await
        {
            Ok(Some(u)) => u,
            Ok(None) => anyhow::bail!("user {name} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        };

        let mut requests = requests();
        requests.dispatch_chat_action(state, chat, invitee.sign).await.context("adding user")?;

        let user_data = requests
            .dispatch::<FetchProfile>(invitee.sign)
            .await
            .context("fetching user profile")?;

        let Some(secret) = state.chat_secret(chat) else {
            anyhow::bail!("we are not part of this chat");
        };

        let cp = enc::Keypair::from_bytes(my_enc)
            .encapsulate_choosen(enc::PublicKey::from_ref(&user_data.enc), secret, OsRng)
            .into_bytes();
        let invite = Mail::ChatInvite { chat, cp }.to_bytes();
        requests
            .dispatch_mail((invitee.sign, Reminder(invite.as_slice())))
            .await
            .context("sending invite")?;

        Ok(())
    };

    let add_hardened_user = move |name: String| async move {
        let Ok(name) = UserName::try_from(name.as_str()) else {
            anyhow::bail!("invalid user name");
        };

        if name == my_name {
            anyhow::bail!("you cannot invite yourself");
        }

        let Some(chat) = current_chat.get_untracked() else {
            anyhow::bail!("no chat selected");
        };

        let Some(members) = state.vault.with_untracked(|v| {
            v.hardened_chats.get(&chat).map(|m| m.members.keys().copied().collect::<Vec<_>>())
        }) else {
            anyhow::bail!("we are not part of this chat");
        };

        let client = crate::chain::node(my_name).await?;
        let invitee = match client
            .get_profile_by_name(crate::chain::user_contract(), username_to_raw(name))
            .await
        {
            Ok(Some(u)) => u,
            Ok(None) => anyhow::bail!("user {name} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        };

        let mut requests = requests();
        let user_data = requests
            .dispatch::<FetchProfile>(invitee.sign)
            .await
            .context("fetching user profile")?;

        let (cp, secret) = enc::Keypair::from_bytes(my_enc)
            .encapsulate(enc::PublicKey::from_ref(&user_data.enc), OsRng);

        let mut payload =
            HardenedChatInvitePayload { chat, inviter: my_name, inviter_id: my_id, members }
                .to_bytes();
        crate::encrypt(&mut payload, secret);

        let invite = Mail::HardenedChatInvite { cp: cp.into_bytes(), payload: Reminder(&payload) }
            .to_bytes();

        requests
            .dispatch_mail((invitee.sign, Reminder(&invite)))
            .await
            .context("sending invite")?;

        state.vault.update(|v| {
            let meta = v.hardened_chats.get_mut(&chat).expect("we checked we are part of the chat");
            meta.members.insert(name, node::MemberMeta { secret, identity: invitee.sign });
        });

        Ok(())
    };

    let (invite_user_button, invite_user_poppup) = popup(
        PoppupStyle {
            placeholder: "user to invite...",
            button_style: "fg0 hov pc",
            button: "+",
            confirm: "invite",
            maxlength: 32,
        },
        move |name| {
            if is_hardened.get_untracked() {
                add_hardened_user(name).left_future()
            } else {
                add_normal_user(name).right_future()
            }
        },
    );

    let message_input = create_node_ref::<Textarea>();
    let resize_input = move || {
        let mi = message_input.get_untracked().unwrap();
        _ = resize_input(mi);
    };

    let send_normal_message = move |chat, content: String| {
        let secret =
            state.chat_secret(chat).context("sending to chat you dont have secret key of")?;
        let mut content = RawChatMessage { sender: my_name, content: &content }.to_bytes();
        crate::encrypt(&mut content, secret);
        let proof = state.next_chat_proof(chat, None).expect("we checked we are part of the chat");

        log::info!("sending message: {:?}", proof.nonce);

        handled_spawn_local("sending normal message", async move {
            let mut req = requests();
            req.dispatch_chat_action(state, chat, Reminder(&content)).await?;
            message_input.get_untracked().unwrap().set_value("");
            resize_input();
            Ok(())
        });

        Ok::<_, anyhow::Error>(())
    };

    let send_hardened_message = move |chat, content: String| {
        let Some(members) = state.vault.with_untracked(|v| {
            v.hardened_chats
                .get(&chat)
                .map(|m| m.members.iter().map(|(a, b)| (*a, *b)).collect::<Vec<_>>())
        }) else {
            anyhow::bail!("I dont know what you are doing, but stop");
        };

        handled_spawn_local("sending hardened message", async move {
            join_all(members.into_iter().map(|(name, member)| {
                let mut message = content.as_bytes().to_vec();
                crate::encrypt(&mut message, member.secret);
                let nonce = rand::thread_rng().gen();
                let message = Mail::HardenedChatMessage {
                    nonce,
                    chat: crypto::hash::with_nonce(chat.as_bytes(), nonce),
                    content: Reminder(&message),
                }
                .to_bytes();
                async move {
                    requests()
                        .dispatch_mail((member.identity, Reminder(&message)))
                        .await
                        .with_context(|| format!("sending message to {name}"))
                }
            }))
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()?;

            db::save_messages(vec![db::Message {
                chat,
                sender: my_name,
                owner: my_name,
                content: content.clone(),
            }])
            .await
            .context("saving our message locally")?;
            append_message(my_name, content);
            message_input.get_untracked().unwrap().set_value("");
            resize_input();
            Ok(())
        });

        Ok(())
    };

    let on_input = handled_callback("processing input", move |e: web_sys::KeyboardEvent| {
        if e.key_code() != '\r' as u32 || e.get_modifier_state("Shift") {
            return Ok(());
        }

        e.prevent_default();

        let chat = current_chat.get_untracked().expect("universe to work");

        let content = message_input.get_untracked().unwrap().value();
        anyhow::ensure!(!content.trim().is_empty(), "message is empty");

        if is_hardened.get_untracked() {
            send_hardened_message(chat, content)
        } else {
            send_normal_message(chat, content)
        }
    });

    let message_scroll = create_node_ref::<leptos::html::Div>();
    let on_scroll = move |_| {
        if red_all_messages.get_untracked() {
            return;
        }

        if !is_at_bottom(message_scroll.get_untracked().unwrap()) {
            return;
        }

        fetch_messages();
    };

    let normal_chats_view = move || {
        state.vault.with(|v| v.chats.keys().map(|&chat| side_chat(chat, false)).collect_view())
    };
    let hardened_chats_view = move || {
        state
            .vault
            .with(|v| v.hardened_chats.keys().map(|&chat| side_chat(chat, true)).collect_view())
    };
    let normal_chats_are_empty = move || state.vault.with(|v| v.chats.is_empty());
    let hardened_chats_are_empty = move || state.vault.with(|v| v.hardened_chats.is_empty());
    let chat_selected = move || current_chat.with(Option::is_some);
    let get_chat = move || current_chat.get().unwrap_or_default().to_string();

    view! {
        <crate::Nav my_name/>
        <main id="main" class="tbm flx fg1 jcsb">
            <div id="sidebar" class="bhc fg0 rbm oys pr" class=("off-screen", show_chat)>
                <div class="pa w100">
                    <div class="bp lsp sc sb tac">
                        "chats"
                        {create_normal_chat_button}
                    </div>
                    {normal_chats_view}
                    <div class="tac bm" hidden=crate::not(normal_chats_are_empty)>"no chats yet"</div>
                    <div class="bp lsp sc sb tac">
                        "hardened chats"
                        {create_hardened_chat_button}
                    </div>
                    {hardened_chats_view}
                    <div class="tac bm" hidden=crate::not(hardened_chats_are_empty)>"no chats yet"</div>
                </div>
            </div>
            <div class="sc fg1 flx pb fdc" hidden=crate::not(chat_selected) class=("off-screen", crate::not(show_chat))>
                <div class="fg0 flx bm jcsb">
                    <button class="hov sf pc lsm phone-only" on:click=hide_chat>"<"</button>
                    <div class="phone-only">{get_chat}</div>
                    {invite_user_button}
                </div>
                <div class="fg1 flx fdc sc pr oys fy" on:scroll=on_scroll node_ref=message_scroll>
                    <div class="fg1 flx fdc bp sc fsc fy boa" node_ref=messages></div>
                </div>
                <textarea class="fg0 flx bm bp pc sf" type="text" rows=1 placeholder="mesg..."
                    node_ref=message_input on:keydown=on_input on:resize=move |_| resize_input()
                    on:keyup=move |_| resize_input()/>
            </div>
            <div class="sc fg1 flx pb fdc" hidden=chat_selected class=("off-screen", crate::not(show_chat))>
                <div class="ma">"no chat selected"</div>
            </div>
        </main>
        {create_normal_chat_poppup}
        {create_hardened_chat_poppup}
        {invite_user_poppup}
    }.into_view()
}

struct PoppupStyle {
    placeholder: &'static str,
    button_style: &'static str,
    button: &'static str,
    confirm: &'static str,
    maxlength: usize,
}

fn popup<F: Future<Output = anyhow::Result<()>>>(
    style: PoppupStyle,
    on_confirm: impl Fn(String) -> F + 'static + Copy,
) -> (impl IntoView, impl IntoView) {
    let (hidden, set_hidden) = create_signal(true);
    let (controls_disabled, set_controls_disabled) = create_signal(false);
    let input = create_node_ref::<Input>();
    let input_trigger = create_trigger();

    let show = move |_| {
        input.get_untracked().unwrap().focus().unwrap();
        set_hidden(false);
    };
    let close = move |_| set_hidden(true);
    let on_confirm = move || {
        let content = crate::get_value(input);
        if content.is_empty() || content.len() > style.maxlength {
            return;
        }

        spawn_local(async move {
            set_controls_disabled(true);
            match on_confirm(content).await {
                Ok(()) => set_hidden(true),
                Err(e) => crate::report_validity(input, e),
            }
            set_controls_disabled(false);
        })
    };
    let on_input = move |_| {
        input_trigger.notify();
        crate::report_validity(input, "")
    };
    let on_keydown = move |e: web_sys::KeyboardEvent| {
        if e.key_code() == '\r' as u32 && !e.get_modifier_state("Shift") {
            on_confirm();
        }

        if e.key_code() == 27 && !controls_disabled.get_untracked() {
            set_hidden(true);
        }
    };
    let confirm_disabled = move || {
        input_trigger.track();
        controls_disabled() || !input.get_untracked().unwrap().check_validity()
    };

    let button = view! { <button class=style.button_style on:click=show>{style.button}</button> };
    let popup = view! {
        <div class="fsc flx blr sb" hidden=hidden>
            <div class="sc flx fdc bp ma bsha">
                <input class="pc hov bp" type="text" placeholder=style.placeholder
                    maxlength=style.maxlength required node_ref=input
                    on:input=on_input on:keydown=on_keydown/>
                <input class="pc hov bp tbm" type="button" value=style.confirm
                    disabled=confirm_disabled on:click=move |_| on_confirm() />
                <input class="pc hov bp tbm" type="button" value="cancel"
                    disabled=controls_disabled on:click=close />
            </div>
        </div>
    };
    (button, popup)
}
