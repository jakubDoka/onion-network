use {
    chat_client_node::RequestContext,
    chat_spec::{ChatError, ChatName},
};

type Context = chat_client_node::TrivialContext;

async fn setup_client(name: &str) -> Context {
    let keys = chat_client_node::UserKeys::new(name.try_into().unwrap(), "", "ws://localhost:9944");
    Context::new(keys).await.unwrap()
}

#[tokio::test]
async fn message_block_finalization() {
    let client = setup_client("jakub").await;
    let chat = ChatName::from("test2").unwrap();

    match client.create_and_save_chat(chat).await {
        Err(e)
            if let Some(ChatError::AlreadyExists) = e.root_cause().downcast_ref::<ChatError>() => {}
        e => e.unwrap(),
    }

    client.flush_vault().await.unwrap();
    let mem = client
        .subscription_for(chat)
        .await
        .unwrap()
        .fetch_my_member(client.keys.identity())
        .await
        .unwrap();
    client.vault.borrow_mut().chats.get_mut(&chat).unwrap().action_no = mem.action + 1;
    let message = "spam ".repeat(150);
    for _ in 0..1000 {
        client.send_message(chat, message.clone()).await.unwrap();
    }
}
