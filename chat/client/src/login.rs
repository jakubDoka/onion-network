use {
    crate::{handled_async_callback, State, UserKeys},
    anyhow::Context,
    chat_spec::UserName,
    leptos::{html::Input, *},
    leptos_router::A,
    web_sys::SubmitEvent,
};

#[component]
pub fn Register(state: State) -> impl IntoView {
    form(state, true)
}

#[component]
pub fn Login(state: State) -> impl IntoView {
    form(state, false)
}

fn form(state: State, register: bool) -> impl IntoView {
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();
    let on_login = handled_async_callback("logging in", move |e: SubmitEvent| async move {
        e.prevent_default();

        let username = crate::get_value(username);
        let password = crate::get_value(password);
        let username = UserName::try_from(username.as_str()).ok().context("invalid username")?;

        let keys = UserKeys::new(username, password.as_str());

        if register {
            keys.register().await?;
        }

        state.keys.set(Some(keys));
        Ok(())
    });

    let action = if register { "register" } else { "login" };
    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <form class="flx fdc" on:submit=on_login>
                <input attr:shortcut="i" class="pc hov bp tbm" type="text" style:width="250px"
                    node_ref=username required maxlength="32" placeholder="username" />
                <input class="pc hov bp tbm" type="password" style:width="250px"
                    node_ref=password placeholder="password" />
                <input class="pc hov bp tbm" type="submit" value=action />
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
