use super::*;

impl State {
    pub(super) fn advance_message(self: &Rc<Self>) {
        let Some((sender_index, thread_index)) = self.next_message.get() else {
            return;
        };
        let groups = ui::virtual_list::groups(&self.thread_list);
        if !groups.is_empty() {
            let index = (thread_index as usize).min(groups.len() - 1);
            ui::virtual_list::select(&self.thread_list, index as u32);
            self.show_thread(groups[index].clone());
            return;
        }
        if self.sender_pane.borrow().loading() && !self.sender_pane.borrow().awaiting_first_page() {
            return;
        }
        if self.sender_pane.borrow().can_load() {
            self.load_sender_page();
            return;
        }
        let current_sender = self.sender_pane.borrow().sender().map(str::to_owned);
        let candidates: Vec<_> = ui::virtual_list::groups(&self.list)
            .into_iter()
            .enumerate()
            .filter_map(|(index, group)| {
                let message = group.first()?;
                (current_sender.as_deref() != Some(models::senders::list_key(message).as_str()))
                    .then_some((index, message.clone()))
            })
            .collect();
        let next = candidates
            .iter()
            .find(|(index, _)| *index >= sender_index as usize)
            .or_else(|| candidates.last());
        if let Some((index, message)) = next {
            if message.search_match {
                self.activate_sender(*index as u32);
            } else {
                self.filter_sender(Some(message.clone()));
                self.next_message.set(Some((*index as u32, 0)));
                self.advance_message();
            }
        } else if !self.sender_pane.borrow().loading() {
            self.next_message.set(None);
            self.filter_sender(None);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::{action_target::ActionTarget, sender_action::SenderAction};

    fn target(message: &Message) -> ActionTarget {
        ActionTarget::Messages(vec![(message.uid, message.message_id.clone())])
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn deleting_a_sender_loads_the_selected_neighbors_messages() {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaSenderLoadDiagnostic",
            worker,
        );
        let window = state.toast.root().and_downcast::<gtk::Window>().unwrap();
        window.present();
        while gtk::glib::MainContext::default().pending() {
            gtk::glib::MainContext::default().iteration(false);
        }
        let messages: Vec<_> = (1..=3)
            .map(|uid| Message {
                uid,
                timestamp: 10 - i64::from(uid),
                sender: format!("sender{uid}@example.com"),
                subject: format!("Conversation {uid}"),
                message_id: uid.to_string(),
                is_read: true,
                ..Default::default()
            })
            .collect();
        for open_sender in [false, true] {
            for position in 0..messages.len() {
                state.filter_sender(None);
                *state.sender_actions.borrow_mut() = Default::default();
                *state.messages.borrow_mut() = messages.clone();
                state.render_list();
                if open_sender {
                    state.activate_sender(position as u32);
                    let ticket = state.sender_pane.borrow().ticket();
                    state.event(Event::SenderPage(
                        state.generation.get(),
                        ticket,
                        vec![messages[position].clone()],
                        false,
                    ));
                } else {
                    ui::virtual_list::select(&state.list, position as u32);
                }
                state.list.grab_focus();
                while commands.try_recv().is_ok() {}
                state.sender_action(
                    models::senders::key(&messages[position]).into(),
                    SenderAction::Delete,
                );
                let next = &messages[if position + 1 < messages.len() {
                    position + 1
                } else {
                    position - 1
                }];
                assert_eq!(
                    state.sender_pane.borrow().sender(),
                    Some(next.sender.as_str())
                );
                assert_eq!(
                    ui::virtual_list::selected_keys(&state.list),
                    vec![next.sender.clone()]
                );
                assert!(commands.try_iter().any(|command| matches!(
                    command,
                    Command::LoadSenderPage { sender, before: None, .. } if sender == next.sender
                )));
                let ticket = state.sender_pane.borrow().ticket();
                let older = Message {
                    uid: 4,
                    timestamp: 0,
                    message_id: "older".into(),
                    subject: "Older conversation".into(),
                    ..next.clone()
                };
                state.event(Event::SenderPage(
                    state.generation.get(),
                    ticket,
                    vec![next.clone(), older],
                    false,
                ));
                assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 2);
                assert!(state.selected.borrow().is_empty());
            }
        }
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn removing_older_mail_advances_in_order_and_opens_cached_next_sender_immediately() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaNavigationDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let (worker, _commands) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "account".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let messages: Vec<_> = (1..=3)
            .rev()
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                sender: "a@example.com".into(),
                subject: format!("Conversation {uid}"),
                message_id: uid.to_string(),
                body_loaded: true,
                is_read: true,
                ..Default::default()
            })
            .collect();
        let elsewhere = Message {
            uid: 4,
            timestamp: 4,
            sender: "b@example.com".into(),
            subject: "Elsewhere".into(),
            message_id: "elsewhere".into(),
            body_loaded: true,
            is_read: true,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![messages[0].clone(), elsewhere.clone()];
        state.render_list();
        state.filter_sender(Some(messages[0].clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            messages.clone(),
            true,
        ));
        state.load_sender_page();
        let late_ticket = state.sender_pane.borrow().ticket();
        state.show_thread(vec![messages[1].clone()]);
        state.sender_action(target(&messages[1]), SenderAction::Archive);
        assert_eq!(*state.selected.borrow(), vec![messages[2].uid]);
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target(&messages[1]),
            messages: Some(vec![messages[0].clone(), elsewhere.clone()]),
            removed: vec![messages[1].clone()],
            error: None,
        });
        state.event(Event::SenderPage(
            state.generation.get(),
            late_ticket,
            vec![messages[1].clone(), messages[2].clone()],
            false,
        ));
        assert_eq!(*state.selected.borrow(), vec![messages[2].uid]);
        assert!(
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .all(|message| message.uid != messages[1].uid)
        );
        state.sender_action(target(&messages[0]), SenderAction::Delete);
        state.sender_action(target(&messages[2]), SenderAction::Delete);
        assert_eq!(*state.selected.borrow(), vec![elsewhere.uid]);
        assert!(state.cards.borrow()[&elsewhere.uid].is_expanded());
        assert_eq!(state.sender_pane.borrow().sender(), Some("b@example.com"));
        assert!(state.sender_pane.borrow().loading());
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![elsewhere.clone()],
            false,
        ));
        assert_eq!(*state.selected.borrow(), vec![elsewhere.uid]);
        assert!(state.cards.borrow()[&elsewhere.uid].is_expanded());
        state.event(Event::CacheList(
            "account".into(),
            "INBOX".into(),
            vec![messages[0].clone(), messages[1].clone(), elsewhere.clone()],
        ));
        assert!(
            state
                .visible_messages()
                .iter()
                .all(|message| message.uid != messages[1].uid)
        );
        state.sender_action(target(&elsewhere), SenderAction::Delete);
        assert!(state.selected.borrow().is_empty());
        assert!(state.cards.borrow().is_empty());
        assert!(state.sender_pane.borrow().sender().is_none());
        window.close();
    }
}
