//use leptos::*;
//
//#[leptos::component]
//pub fn FileUpload() -> impl IntoView {
//    let satelite = create_node_ref::<html::Input>();
//    let password = create_node_ref::<html::Input>();
//    let file = create_node_ref::<html::Input>();
//
//    let upload_file = move |_| {
//        let satelite = crate::get_value(satelite);
//        let password = crate::get_value(password);
//    };
//
//    view! {
//        <form on:submit=upload_file>
//            <input type="text" node_ref=satelite />
//            <input type="password" node_ref=password />
//            <input type="file" node_ref=file />
//            <input type="submit" />
//        </form>
//    }
//}
