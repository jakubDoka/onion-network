use {
    chat_client_node::RequestContext,
    chat_spec::{ChatError, ChatName},
    libp2p::futures::{
        future::{join, join_all, try_join},
        StreamExt,
    },
};

async fn setup_clients<const COUNT: usize>(
    name: [&str; COUNT],
) -> [chat_client_node::TrivialContext; COUNT] {
    join_all(name.map(|name| async move {
        let keys =
            chat_client_node::UserKeys::new(name.try_into().unwrap(), "", "ws://localhost:9944");
        chat_client_node::TrivialContext::new(keys).await.unwrap()
    }))
    .await
    .try_into()
    .ok()
    .unwrap()
}

#[tokio::test]
async fn message_block_finalization() {
    let [jakub, kubo] = setup_clients(["jakub", "kubo"]).await;
    let chat = ChatName::from("test2").unwrap();

    match jakub.create_and_save_chat(chat).await {
        Err(e)
            if let Some(ChatError::AlreadyExists) = e.root_cause().downcast_ref::<ChatError>() => {}
        e => e.unwrap(),
    }

    jakub.flush_vault().await.unwrap();
    let mem = jakub
        .subscription_for(chat)
        .await
        .unwrap()
        .fetch_my_member(jakub.keys.identity())
        .await
        .unwrap();
    jakub.vault.borrow_mut().chats.get_mut(&chat).unwrap().action_no = mem.action + 1;
    let message = "spam ".repeat(150);
    for _ in 0..100 {
        jakub.send_message(chat, message.clone()).await.unwrap();
    }

    let mut sub = kubo.profile_subscription().await.unwrap().subscribe().await.unwrap();

    let (res, foo) =
        join(jakub.invite_member(chat, kubo.keys.name, chat_spec::Member::best()), sub.next())
            .await;

    res.unwrap();

    foo.unwrap().handle(&kubo).await.unwrap();

    for _ in 0..100 {
        try_join(
            jakub.send_message(chat, message.clone()),
            kubo.send_message(chat, message.clone()),
        )
        .await
        .unwrap();
    }
}
