use {
    crate::{handled_async_callback, State, UserKeys},
    anyhow::Context,
    chat_client_node::BootPhase,
    chat_spec::UserName,
    component_utils::DropFn,
    leptos::{
        html::{Div, Input},
        *,
    },
    leptos_router::A,
    web_sys::{js_sys, SubmitEvent},
};

#[component]
pub fn Register(
    set_state: WriteSignal<Option<State>>,
    wboot_phase: WriteSignal<Option<BootPhase>>,
) -> impl IntoView {
    form(set_state, wboot_phase, true)
}

#[component]
pub fn Login(
    set_state: WriteSignal<Option<State>>,
    wboot_phase: WriteSignal<Option<BootPhase>>,
) -> impl IntoView {
    form(set_state, wboot_phase, false)
}

fn form(
    state: WriteSignal<Option<State>>,
    wboot_phase: WriteSignal<Option<BootPhase>>,
    register: bool,
) -> impl IntoView {
    state.update_untracked(|state| {
        if let Some(state) = state.take() {
            state.dispose();
        }
    });

    let action = if register { "register" } else { "login" };

    let prev_account_id = create_rw_signal(chain_api::AccountId::from([0; 32]));
    let hint = create_node_ref::<Div>();
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();
    let processing = create_rw_signal(false);

    let on_login = handled_async_callback(action, move |e: SubmitEvent| async move {
        e.prevent_default();

        if processing.get_untracked() {
            return Ok(());
        }

        processing.set(true);
        let _defer = DropFn::new(|| processing.set(false));

        let username = crate::get_value(username);
        let password = crate::get_value(password);
        let username = UserName::try_from(username.as_str()).ok().context("invalid username")?;

        let url = option_env!("CHAIN_NODES").unwrap_or("ws://localhost:9944");
        let keys = UserKeys::new(username, password.as_str(), url);
        let client = keys.chain_client().await?;

        if register {
            let account_id = client.account_id();
            if account_id != prev_account_id.get_untracked() {
                let msg = format!(
                    "your hot wallet adress is {account_id} \
                    transfere funds here to pay for registration \
                    (TODO: hint amount)"
                );
                hint.get_untracked().unwrap().set_text_content(Some(&msg));
                prev_account_id.set(account_id);
                return Ok(());
            }

            keys.register().await?;
        }

        // TODO: notifi user about the paymant
        let sub = client.get_subscription(keys.identity()).await?;
        let now = js_sys::Date::new_0().get_time() as u64 / 1000;
        if !sub.is_some_and(|sub| chain_api::is_valid_sub(&sub, now)) {
            // TODO: we need better checking here
            let enough_funds =
                client.get_balance().await?.is_some_and(|b| b >= chain_api::MIN_SUB_FUNDS * 2);
            anyhow::ensure!(enough_funds, "insufficient funds to subscribe to the serice");

            client.subscribe(keys.identity(), chain_api::MIN_SUB_FUNDS).await?;
        }

        crate::init_state(keys, wboot_phase, state).await?;

        Ok(())
    });

    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <form class="flx fdc" on:submit=on_login>
                <input attr:shortcut="i" class="pc hov bp tbm" type="text" style:width="250px"
                    node_ref=username required maxlength="32" placeholder="username" />
                <input class="pc hov bp tbm" type="password" style:width="250px"
                    node_ref=password placeholder="password" />
                <input class="pc hov bp tbm" type="submit" disabled=processing value=action />
                <div hidden=!register node_ref=hint>"Registration is not free!"</div>
            </form>
        </div>
    }
}

#[component]
fn Nav() -> impl IntoView {
    view! {
        <nav class="flx jcsb">
            <A class="bf hov bp rsb sc" href="/login">/login</A>
            <A class="bf hov bp sb sc" href="/register">/register</A>
        </nav>
    }
}
