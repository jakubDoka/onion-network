use {
    crate::{handled_async_closure, State, UserKeys},
    anyhow::Context,
    chat_spec::{username_to_raw, UserName},
    leptos::{html::Input, *},
    leptos_router::A,
};

#[component]
pub fn Login(state: State) -> impl IntoView {
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();
    let on_login = handled_async_closure("logging in", move || async move {
        let username = username.get_untracked().expect("universe to work");
        let password = password.get_untracked().expect("universe to work");
        let username_content =
            UserName::try_from(username.value().as_str()).ok().context("invalid username")?;

        let keys = UserKeys::new(username_content, password.value().as_str());

        state.keys.set(Some(keys));
        Ok(())
    });
    let on_password_keyup = move |e: web_sys::KeyboardEvent| {
        if e.key_code() == '\r' as u32 {
            on_login();
        }
    };

    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <div class="flx fdc">
                <input class="pc hov bp tbm" type="text" style:width="250px"
                    node_ref=username required maxlength="32" placeholder="username" />
                <input class="pc hov bp tbm" type="password" style:width="250px"
                    node_ref=password on:keyup=on_password_keyup placeholder="password" />
                <input class="pc hov bp tbm" type="button" value="login"
                    on:click=move |_| on_login() />
            </div>
        </div>
    }
}

#[component]
pub fn Register(state: State) -> impl IntoView {
    let username = create_node_ref::<Input>();
    let password = create_node_ref::<Input>();

    let on_register = handled_async_closure("registering", move || async move {
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

    let validation_trigger = create_trigger();
    let on_change = move |e: web_sys::KeyboardEvent| {
        validation_trigger.notify();
        if e.key_code() == '\r' as u32 {
            on_register();
        }
    };
    let disabled = move || {
        validation_trigger.track();
        !username.get_untracked().unwrap().check_validity()
    };

    view! {
        <div class="sc flx fdc bp ma">
            <Nav/>
            <div class="flx fdc">
                <input class="pc hov bp tbm" type="text" placeholder="new username" maxlength="32"
                    node_ref=username on:keyup=on_change required />
                <input class="pc hov bp tbm" type="password" placeholder="new password"
                    node_ref=password on:keyup=on_change />
                <input class="pc hov bp tbm" type="button" value="register"
                    on:click=move |_| on_register() disabled=disabled />
            </div>
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
