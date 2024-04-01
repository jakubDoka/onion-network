use {crate::handle_js_err, chat_spec::UserName};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Message {
    pub owner: UserName,
    pub chat: String,
    pub sender: UserName,
    pub content: String,
}

mod ffi {
    use wasm_bindgen_futures::wasm_bindgen::{self, JsValue};

    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[derive(Clone)]
        pub type MessageCursor;

        #[wasm_bindgen(constructor)]
        pub fn new(chat: String, owner: String) -> MessageCursor;

        #[wasm_bindgen(catch, method)]
        pub async fn next(this: &MessageCursor, amount: u32) -> Result<JsValue, JsValue>;
    }

    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = db)]
        pub async fn saveMessages(messages: String) -> Result<(), JsValue>;
    }
}

pub async fn save_messages(messages: &[Message]) -> anyhow::Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let messages = serde_json::to_string(&messages)?;
    ffi::saveMessages(messages).await.map_err(handle_js_err)
}

#[derive(Clone)]
pub struct MessageCursor {
    inner: ffi::MessageCursor,
}

impl MessageCursor {
    pub async fn new(chat: String, owner: UserName) -> anyhow::Result<Self> {
        let inner = ffi::MessageCursor::new(chat.to_string(), owner.to_string());
        Ok(Self { inner })
    }

    pub async fn next(&self, amount: u32) -> anyhow::Result<Vec<Message>> {
        let js_value = self.inner.next(amount).await.map_err(handle_js_err)?;
        let messages: Vec<Message> = serde_json::from_str(&js_value.as_string().unwrap())?;
        Ok(messages)
    }
}
