use {
    crate::{
        db, handled_async_callback, handled_async_closure, handled_callback, handled_spawn_local,
        node::{self, HardenedChatInvitePayload, Mail, MessageContent, RawChatMessage},
    },
    anyhow::Context,
    chat_spec::{ChatError, ChatEvent, ChatName, Identity, Member, Permissions, Rank, UserName},
    component_utils::{Codec, DropFn, Reminder},
    crypto::{enc, SharedSecret, TransmutationCircle},
    leptos::{
        html::{Input, Textarea},
        *,
    },
    leptos_router::Redirect,
    libp2p::futures::{future::join_all, FutureExt},
    rand::{rngs::OsRng, Rng},
    std::{
        future::Future,
        sync::atomic::{AtomicBool, Ordering},
    },
    web_sys::{js_sys::eval, SubmitEvent},
};

fn is_at_bottom(messages_div: HtmlElement<leptos::html::AnyElement>) -> bool {
    let scroll_bottom = messages_div.scroll_top();
    let scroll_height = messages_div.scroll_height();
    let client_height = messages_div.client_height();

    let prediction = 200;
    scroll_height - client_height <= scroll_bottom + prediction
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

    let current_chat = create_rw_signal(selected);
    let (current_member, set_current_member) = create_signal(Member::best());
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
        let message = message_view(username, content);
        let messages = messages.get_untracked().expect("layout invariants");
        messages.append_child(&message).unwrap();
    };
    let prepend_message = move |username: UserName, content: MessageContent| {
        let message = message_view(username, content);
        let messages = messages.get_untracked().expect("layout invariants");
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
                    requests.fetch_messages(chat, cursor).await?;
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

    on_cleanup(move || {
        if let Some(chat) = current_chat.get_untracked() {
            requests().unsubscribe(chat);
        }
    });

    let handle_event = move |event: ChatEvent, secret: SharedSecret| match event {
        ChatEvent::Message(sender, Reminder(message)) => {
            _ = sender; // TODO: verify this matches with the username

            let mut message = message.to_vec();
            let Some(message) = crypto::decrypt(&mut message, secret) else {
                log::warn!("message cannot be decrypted: {:?} {:?}", message, secret);
                return;
            };

            let Some(RawChatMessage { sender, content }) = RawChatMessage::decode(&mut &*message)
            else {
                log::warn!("message cannot be decoded: {:?}", message);
                return;
            };

            append_message(sender, content.into());
            log::info!("received message: {:?}", content);
        }
        ChatEvent::Member(identiy, member) => {
            if identiy == my_id {
                set_current_member(member);
            }

            log::info!("received member: {:?}", member);

            let elem = member_view(state, identiy, member, current_member, current_chat);
            if let Some(member_elem) = document().get_element_by_id(&hex::encode(identiy)) {
                member_elem.replace_with_with_node_1(&elem).unwrap();
            } else {
                document().get_element_by_id("members").unwrap().append_child(&elem).unwrap();
            }
        }
        ChatEvent::MemberRemoved(identity) => {
            if identity == my_id {
                if let Some(chat) = current_chat.get_untracked() {
                    state.vault.update(|v| _ = v.chats.remove(&chat));
                    current_chat.set(None);
                }
            }

            log::info!("member removed: {:?}", identity);

            if let Some(member_elem) = document().get_element_by_id(&hex::encode(identity)) {
                member_elem.remove();
            }
        }
    };

    create_effect(move |prev_chat| {
        if let Some(Some(prev_chat)) = prev_chat {
            requests().unsubscribe(prev_chat);
        }

        let Some(chat) = current_chat() else {
            messages.get_untracked().unwrap().set_inner_html("");
            return None;
        };

        if is_hardened.get_untracked() {
            return None;
        }
        let secret = state.chat_secret(chat)?;

        handled_spawn_local("reading chat messages", async move {
            let mut sub = requests().subscribe(chat).await?;
            log::info!("subscribed to chat: {:?}", chat);
            while let Some(bytes) = sub.next().await {
                let Some(event) = ChatEvent::decode(&mut &*bytes) else {
                    log::warn!("received message that cannot be decoded");
                    continue;
                };

                handle_event(event, secret);
            }

            Ok(())
        });

        Some(chat)
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
        let select_chat = handled_async_callback("switching chat", move |_| async move {
            let my_user = if hardened {
                Member::best()
            } else {
                let m = requests()
                    .fetch_my_member(chat, my_id)
                    .await
                    .inspect_err(|e| {
                        if !matches!(e, ChatError::NotFound | ChatError::NotMember) {
                            return;
                        }
                        state.vault.update(|v| _ = v.chats.remove(&chat));
                    })
                    .context("fetching our member")?;
                state.set_chat_nonce(chat, m.action + 1);
                m
            };
            set_current_member(my_user);
            set_show_chat(true);
            set_red_all_messages(false);
            set_cursor(Cursor::Normal(chat_spec::Cursor::INIT));
            current_chat.set(Some(chat));
            set_is_hardened(hardened);
            crate::navigate_to(format_args!("/chat/{chat}"));
            let messages = messages.get_untracked().expect("universe to work");
            messages.set_inner_html("");
            fetch_messages();

            Ok(())
        });
        let selected = move || current_chat.get() == Some(chat);
        let shortcut_prefix = if hardened { "sh" } else { "sn" };
        view! {
            <div class="sb tac bp toe" class:hc=selected
                shortcut=format!("{shortcut_prefix}{chat}")
                class:hov=crate::not(selected) on:click=select_chat>
                {chat.to_string()} </div>
        }
    };

    let (create_normal_chat_button, create_normal_chat_poppup) = popup(
        PoppupStyle {
            placeholder: "chat name...",
            button_style: "hov sf pc lsm",
            button: "+",
            shortcut: "cn",
            confirm: "create",
            maxlength: 32,
        },
        move || true,
        move |chat| async move {
            let Ok(chat) = ChatName::try_from(chat.as_str()) else {
                anyhow::bail!("invalid chat name");
            };

            if state.vault.with_untracked(|v| v.chats.contains_key(&chat)) {
                anyhow::bail!("chat already exists");
            }

            requests().create_chat(chat, my_id).await.context("creating chat")?;

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
            shortcut: "ch",
            confirm: "create",
            maxlength: 32,
        },
        move || true,
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
        let name = UserName::try_from(name.as_str()).ok().context("invalid username")?;
        let chat = current_chat.get_untracked().context("no chat selected")?;
        let secret = state.chat_secret(chat).context("we are nor part of this chat")?;
        let invitee = crate::chain::fetch_profile(my_name, name).await?;

        let mut requests = requests();
        requests
            .add_member(state, chat, invitee.sign, Member::worst())
            .await
            .context("adding user")?;

        let user_data = requests.fetch_keys(invitee.sign).await.context("fetching user profile")?;

        let cp = enc::Keypair::from_bytes(my_enc)
            .encapsulate_choosen(enc::PublicKey::from_ref(&user_data.enc), secret, OsRng)
            .into_bytes();
        requests
            .send_mail(invitee.sign, Mail::ChatInvite { chat, cp })
            .await
            .context("sending invite")?;

        Ok(())
    };

    let add_hardened_user = move |name: String| async move {
        let name = UserName::try_from(name.as_str()).ok().context("invalid username")?;
        let chat = current_chat.get_untracked().context("no chat selected")?;
        let members = state
            .vault
            .with_untracked(|v| {
                v.hardened_chats.get(&chat).map(|m| m.members.keys().copied().collect::<Vec<_>>())
            })
            .context("we are not part of this chat")?;

        let invitee = crate::chain::fetch_profile(my_name, name).await?;

        let mut requests = requests();
        let user_data = requests.fetch_keys(invitee.sign).await.context("fetching user profile")?;

        let (cp, secret) = enc::Keypair::from_bytes(my_enc)
            .encapsulate(enc::PublicKey::from_ref(&user_data.enc), OsRng);

        let mut payload =
            HardenedChatInvitePayload { chat, inviter: my_name, inviter_id: my_id, members }
                .to_bytes();
        crate::encrypt(&mut payload, secret);
        requests
            .send_mail(invitee.sign, Mail::HardenedChatInvite {
                cp: cp.into_bytes(),
                payload: Reminder(&payload),
            })
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
            shortcut: "am",
            confirm: "invite",
            maxlength: 32,
        },
        move || current_member.get().permissions.contains(chat_spec::Permissions::INVITE),
        move |name| {
            if is_hardened.get_untracked() {
                add_hardened_user(name).left_future()
            } else {
                add_normal_user(name).right_future()
            }
        },
    );

    let (member_list_button, member_list_popup) =
        member_list_poppup(state, current_member, current_chat);

    let message_input = create_node_ref::<Textarea>();
    let clear_input = move || {
        message_input.get_untracked().unwrap().set_value("");
        message_input
            .get_untracked()
            .unwrap()
            .dispatch_event(&web_sys::Event::new("input").unwrap())
            .unwrap();
    };

    let send_normal_message = move |chat, content: String| {
        let secret =
            state.chat_secret(chat).context("sending to chat you dont have secret key of")?;
        let mut content = RawChatMessage { sender: my_name, content: &content }.to_bytes();
        crate::encrypt(&mut content, secret);

        handled_spawn_local("sending normal message", async move {
            let mut req = requests();
            req.send_message(state, chat, &content).await?;
            clear_input();
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
                async move {
                    requests()
                        .send_mail(member.identity, Mail::HardenedChatMessage {
                            nonce,
                            chat: crypto::hash::with_nonce(chat.as_bytes(), nonce),
                            content: Reminder(&message),
                        })
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
            clear_input();
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

        if !is_at_bottom(message_scroll.get_untracked().unwrap().into_any()) {
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
    let can_send = move || current_member.get().permissions.contains(chat_spec::Permissions::SEND);

    request_idle_callback(|| _ = eval("setup_resizable_textarea()").unwrap());

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
                <div class="fg0 flx bm">
                    <button class="hov sf pc lsm phone-only" on:click=hide_chat>"<"</button>
                    <div class="phone-only">{get_chat}</div>
                    {invite_user_button}
                    {member_list_button}
                </div>
                <div class="fg1 flx fdc sc pr oys fy" on:scroll=on_scroll node_ref=message_scroll>
                    <div class="fg1 flx fdc bp sc fsc fy boa" node_ref=messages></div>
                </div>
                <textarea class="fg0 flx bm bp pc sf" id="message-input" type="text" rows=1
                    placeholder="mesg..." node_ref=message_input on:keydown=on_input 
                    disabled=crate::not(can_send) shortcut="i"/>
            </div>
            <div class="sc fg1 flx pb fdc" hidden=chat_selected class=("off-screen", crate::not(show_chat))>
                <div class="ma">"no chat selected"</div>
            </div>
        </main>
        {create_normal_chat_poppup}
        {create_hardened_chat_poppup}
        {invite_user_poppup}
        {member_list_popup}
    }.into_view()
}

fn member_list_poppup(
    state: crate::State,
    me: ReadSignal<Member>,
    name_sig: RwSignal<Option<ChatName>>,
) -> (impl IntoView, impl IntoView) {
    let (hidden, set_hidden) = create_signal(true);
    let members = create_node_ref::<html::Table>();
    let cursor = store_value(Identity::default());
    let (exhausted, set_exhausted) = create_signal(false);

    let requests = move || state.requests.get_value().unwrap();
    let show = move |_| _ = (set_hidden(false), set_exhausted(false));
    let hide = move |_| set_hidden(true);
    let load_members = handled_async_closure("loading members", move || async move {
        let name = name_sig.get_untracked().context("no chat selected")?;
        let last_identity = cursor.get_value();
        let fetched_members = requests().fetch_members(name, last_identity, 31).await?;
        set_exhausted(fetched_members.len() < 31);

        let members = members.get_untracked().expect("layout invariants");
        for &(identity, member) in
            fetched_members.iter().skip((last_identity != Identity::default()) as usize)
        {
            let member = member_view(state, identity, member, me, name_sig);
            members.append_child(&member).unwrap();
        }

        Ok::<_, anyhow::Error>(())
    });

    let handle_scroll = move |_| {
        let members = members.get_untracked().expect("layout invariants");
        if is_at_bottom(members.into_any()) && !exhausted() {
            load_members();
        }
    };

    create_effect(move |_| {
        if name_sig.get().is_none() {
            return;
        }

        members.get_untracked().unwrap().set_inner_html("");
        load_members();
    });

    let button = view! {
        <button class="hov sf pc lsm" shortcut="lm"
            on:click=show>"members"</button>
    };
    let popup = view! {
        <div class="fsc flx blr sb" hidden=hidden>
            <div class="sc flx fdc bp bm bsha w100 bgps">
                <div class="pr fg1 oys pc flx fdc"  on:scroll=handle_scroll>
                    <table class="tlf w100">
                        <tr class="sp"><th>rank</th><th>perm</th><th>rtlm</th></tr>
                    </table>
                    <table class="tlf w100" id="members" node_ref=members></table>
                </div>
                <input class="pc hov bp" type="button" value="cancel" on:click=hide shortcut="<esc>" />
            </div>
        </div>
    };
    (button, popup)
}

fn member_view(
    state: crate::State,
    identity: Identity,
    m: Member,
    me: ReadSignal<Member>,
    name: RwSignal<Option<ChatName>>,
) -> HtmlElement<html::Tr> {
    let my_id = state.keys.get_untracked().unwrap().identity_hash();
    let can_modify = me.get_untracked().rank < m.rank || my_id == identity;
    let root = create_node_ref::<html::Tr>();
    let start_edit = handled_callback("opening member edit", move |_| {
        if !can_modify {
            return Ok(());
        }

        root.get_untracked()
            .unwrap()
            .replace_with_with_node_1(&editable_member(state, identity, m, me, name))
            .unwrap();

        Ok(())
    });

    view! {
        <tr class:hov=can_modify class="sp" node_ref=root on:click=start_edit
            id=hex::encode(identity)>
            <th>{m.rank}</th>
            <th>{m.permissions.to_string()}</th>
            <th>{m.action_cooldown_ms}</th>
        </tr>
    }
}

fn editable_member(
    state: crate::State,
    identity: Identity,
    m: Member,
    me: ReadSignal<Member>,
    name_sig: RwSignal<Option<ChatName>>,
) -> HtmlElement<html::Tbody> {
    let rank = create_node_ref::<Input>();
    let permissions = create_node_ref::<Input>();
    let action_cooldown_ms = create_node_ref::<Input>();
    let root = create_node_ref::<html::Tbody>();

    let ctx = "commiting member changes";
    let is_me = state.keys.get_untracked().unwrap().identity_hash() == identity;
    let kick_label = if is_me { "leave" } else { "kick" };
    let kick_ctx = if is_me { "leaving chat" } else { "kicking member" };

    let cancel = move || {
        root.get_untracked()
            .unwrap()
            .replace_with_with_node_1(&member_view(state, identity, m, me, name_sig))
            .unwrap();
    };

    let kick = handled_async_callback(kick_ctx, move |_| async move {
        let name = name_sig.get_untracked().context("no chat selected")?;
        let mut requests = state.requests.get_value().unwrap();
        requests.kick_member(state, name, identity).await?;
        root.get_untracked().unwrap().remove();
        Ok(())
    });
    let save = handled_async_callback(ctx, move |e: SubmitEvent| async move {
        e.prevent_default();

        let name = name_sig.get_untracked().context("no chat selected")?;

        let rank = crate::get_value(rank).parse::<Rank>().context("parsing rank")?;
        let permissions = crate::get_value(permissions)
            .parse::<chat_spec::Permissions>()
            .context("parsing permissions")?;
        if permissions & me.get_untracked().permissions != permissions {
            anyhow::bail!("you cannot give more permissions than you have");
        }
        let action_cooldown_ms =
            crate::get_value(action_cooldown_ms).parse::<u32>().context("parsing cooldown")?;
        let updated_member = Member { rank, permissions, action_cooldown_ms, ..m };

        let mut requests = state.requests.get_value().unwrap();
        requests.add_member(state, name, identity, updated_member).await?;

        cancel();

        Ok(())
    });

    view! {
        <tbody class="br sb sc sp" node_ref=root id=hex::encode(identity)>
            <form id="member-form" on:submit=save></form>
            <tr class="sbb">
                <th><input class="hov pc tac sf" type="number" value=m.rank form="member-form"
                    min=me.get_untracked().rank max=Rank::MAX node_ref=rank required /></th>
                <th><input class="hov pc tac sf" type="text" value=m.permissions.to_string()
                    node_ref=permissions maxlength=Permissions::COUNT form="member-form"
                    required /></th>
                <th><input class="hov pc tac sf" type="number" value=m.action_cooldown_ms
                    form="member-form" min=me.get_untracked().action_cooldown_ms max=u32::MAX
                    node_ref=action_cooldown_ms required /></th>
            </tr>
            <tr class="sc sb"><td colspan=3><div class="flx jcc sgps">
                <input class="hov pc sf" type="submit" value="save" form="member-form" />
                <input class="hov pc sf" type="button" value=kick_label on:click=kick />
                <input class="hov pc sf" type="button" value="cancel" on:click=move |_| cancel() />
            </div></td></tr>
        </tbody>
    }
}

struct PoppupStyle {
    placeholder: &'static str,
    button_style: &'static str,
    button: &'static str,
    confirm: &'static str,
    shortcut: &'static str,
    maxlength: usize,
}

fn popup<F: Future<Output = anyhow::Result<()>>>(
    style: PoppupStyle,
    enabled: impl Fn() -> bool + 'static + Copy,
    on_confirm: impl Fn(String) -> F + 'static + Copy,
) -> (impl IntoView, impl IntoView) {
    let (hidden, set_hidden) = create_signal(true);
    let (controls_disabled, set_controls_disabled) = create_signal(false);
    let input = create_node_ref::<Input>();

    let show = move |_| {
        input.get_untracked().unwrap().focus().unwrap();
        set_hidden(false);
    };
    let hide = move |_| set_hidden(true);
    let on_confirm = move |e: SubmitEvent| {
        e.prevent_default();
        handled_spawn_local(style.button, async move {
            set_controls_disabled(true);
            let res = on_confirm(crate::get_value(input)).await;
            set_controls_disabled(false);
            if res.is_ok() {
                set_hidden(true);
            }
            res
        })
    };

    let button = view! {
        <button class=style.button_style shortcut=style.shortcut disabled=crate::not(enabled)
            on:click=show> {style.button}</button>
    };
    let popup = view! {
        <div class="fsc flx blr sb" hidden=hidden>
            <form class="sc flx fdc bp ma bsha" on:submit=on_confirm>
                <input class="pc hov bp" type="text" placeholder=style.placeholder
                    maxlength=style.maxlength required node_ref=input shortcut="i 1"/>
                <input class="pc hov bp tbm" type="submit" value=style.confirm
                    disabled=controls_disabled />
                <input class="pc hov bp tbm" type="button" value="cancel"
                    on:click=hide shortcut="<esc>" />
            </form>
        </div>
    };
    (button, popup)
}
