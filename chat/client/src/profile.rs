use {
    chat_client_node::{RequestContext, Theme},
    leptos::*,
};

#[component]
pub fn Profile(state: crate::State) -> impl IntoView {
    let colors = Theme::KEYS;
    let style = web_sys::window()
        .unwrap()
        .get_computed_style(&document().body().unwrap())
        .unwrap()
        .unwrap();
    let elements = colors.iter().map(|&c| {
        let mut value = style.get_property_value(&format!("--{c}-color")).unwrap();
        value.truncate("#000000".len());
        let on_change = move |e: web_sys::Event| {
            let value = event_target_value(&e);
            document()
                .body()
                .unwrap()
                .style()
                .set_property(&format!("--{c}-color"), &format!("{}ff", value))
                .unwrap();
        };
        view! {
            <div class="flx tbm">
                <input type="color" name=c value=value on:input=on_change />
                <span class="lbp">{c}</span>
            </div>
        }
    });

    let on_save = crate::handled_async_callback("saving theme", move |_| async move {
        let them = Theme::from_current().unwrap_or_default();
        state.set_theme(them).await
    });

    view! {
        <crate::Nav keys=state.keys />
        <main class="tbm fg1 sc bp">
            <div class="flx">
                <form id="theme-form" class="flx fg0 fdc bp pc">
                    <span class="lbp">theme</span>
                    {elements.into_iter().collect_view()}
                    <input class="sc hov bp sf tbm" type="button" value="save" on:click=on_save />
                </form>
            </div>
        </main>
    }
    .into_view()
}
