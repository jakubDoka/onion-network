use {
    crate::{handled_async_callback, State, UserKeys},
    anyhow::Context,
    chat_spec::{username_to_raw, UserName},
    leptos::{html::Input, *},
    leptos_router::A,
    web_sys::SubmitEvent,
};

#[component]
pub fn Login(state: State) -> impl IntoView {
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();
    let on_login = handled_async_callback("logging in", move |e: SubmitEvent| async move {
        e.prevent_default();

        let username = username.get_untracked().expect("universe to work");
        let password = password.get_untracked().expect("universe to work");
        let username_content =
            UserName::try_from(username.value().as_str()).ok().context("invalid username")?;

        let keys = UserKeys::new(username_content, password.value().as_str());

        state.keys.set(Some(keys));
        Ok(())
    });

    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <form class="flx fdc" on:submit=on_login>
                <input class="pc hov bp tbm" type="text" style:width="250px"
                    node_ref=username required maxlength="32" placeholder="username" />
                <input class="pc hov bp tbm" type="password" style:width="250px"
                    node_ref=password placeholder="password" />
                <input class="pc hov bp tbm" type="submit" value="login" />
            </form>
        </div>
    }
}

#[component]
pub fn Register(state: State) -> impl IntoView {
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();
    let on_register = handled_async_callback("registering", move |e: SubmitEvent| async move {
        e.prevent_default();
        let username = username.get_untracked().expect("universe to work");
        let password = password.get_untracked().expect("universe to work");
        let username_content =
            UserName::try_from(username.value().as_str()).ok().context("invalid username")?;

        let key = UserKeys::new(username_content, password.value().as_str());

        let client =
            crate::chain::node(username_content).await.context("chain is not reachable")?;

        if client
            .user_exists(crate::chain::user_contract(), username_to_raw(username_content))
            .await
            .context("user contract call failed")?
        {
            anyhow::bail!("user with this name already exists");
        }

        client
            .register(
                crate::chain::user_contract(),
                username_to_raw(username_content),
                key.to_identity(),
                0,
            )
            .await
            .context("failed to create user")?;

        state.keys.set(Some(key));
        Ok(())
    });

    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <form class="flx fdc" on:submit=on_register>
                <input class="pc hov bp tbm" type="text" placeholder="new username"
                    maxlength="32" node_ref=username required />
                <input class="pc hov bp tbm" type="password" placeholder="new password"
                    node_ref=password />
                <input class="pc hov bp tbm" type="submit" value="register" />
            </form>
        </div>
    }
}

#[component]
fn Nav() -> impl IntoView {
    view! {
        <nav class="flx jcsb">
            <A class="bf hov bp rsb" href="/login">/login</A>
            <A class="bf hov bp sb" href="/register">/register</A>
        </nav>
    }
}
