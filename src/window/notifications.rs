use super::search::search_bar;
use super::*;

impl State {
    pub(super) fn notification_action_started(
        self: &Rc<Self>,
        email: &str,
        validity: u32,
        uid: u32,
        action: models::sender_action::SenderAction,
    ) {
        let current = self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| account.email == email)
            && *self.folder.borrow() == "INBOX";
        let target = |message: &Message| {
            (message.uid == uid && message.uid_validity == Some(validity)).then(|| {
                models::action_target::ActionTarget::Messages(vec![(
                    uid,
                    message.message_id.clone(),
                )])
            })
        };
        let selected = current
            .then(|| {
                self.messages
                    .borrow()
                    .iter()
                    .chain(self.sender_pane.borrow().messages())
                    .chain(self.open_group.borrow().iter())
                    .find_map(target)
            })
            .flatten();
        let selected = selected.or_else(|| {
            self.folder_cache
                .borrow()
                .get(&(email.into(), "INBOX".into()))?
                .iter()
                .find_map(target)
        });
        let Some(target) = selected else { return };
        if !self
            .sender_actions
            .borrow_mut()
            .begin(email, "INBOX", &target, action.clone())
        {
            return;
        }
        self.notification_actions
            .borrow_mut()
            .insert((email.into(), validity, uid), target.clone());
        if current {
            self.show_pending_action(&target, &action);
        }
    }

    pub(super) fn notification_action_finished(&self, email: &str, validity: u32, uid: u32) {
        let Some(target) =
            self.notification_actions
                .borrow_mut()
                .remove(&(email.into(), validity, uid))
        else {
            return;
        };
        // Failures before IMAP execution do not produce SenderActionFinished.
        // Clear their projection here so the original cached state is restored.
        self.sender_actions
            .borrow_mut()
            .finish(email, "INBOX", &target);
        if self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| account.email == email)
            && *self.folder.borrow() == "INBOX"
        {
            self.refresh_sender_view();
        }
    }

    pub(super) fn open_notification(self: &Rc<Self>, email: &str, mut message: Message) {
        let account = self
            .folder_boxes
            .borrow()
            .get(email)
            .map(|(account, _)| account.clone());
        let Some(account) = account else {
            self.toast
                .add_toast(adw::Toast::new("This account is no longer available"));
            return;
        };
        let current = self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| account.email == email)
            && *self.folder.borrow() == "INBOX";
        search_bar(&self.search).set_search_mode(false);
        search_bar(&self.thread_search).set_search_mode(false);
        self.search.set_text("");
        self.thread_search.set_text("");
        if !current {
            self.select(account, "INBOX".into());
        }
        self.merge_read_states(std::slice::from_mut(&mut message));
        {
            let mut messages = self.messages.borrow_mut();
            if let Some(existing) = messages.iter_mut().find(|existing| {
                existing.uid == message.uid && existing.message_id == message.message_id
            }) {
                *existing = message.clone();
            } else {
                messages.push(message.clone());
            }
        }
        self.filter_sender(Some(message.clone()));
        let sender = models::senders::key(&message);
        let cached: Vec<_> = self
            .messages
            .borrow()
            .iter()
            .filter(|item| models::senders::key(item) == sender)
            .map(|item| item.list_header(true))
            .collect();
        if let Some(group) = models::threads(&cached, "").into_iter().find(|group| {
            group
                .iter()
                .any(|item| item.uid == message.uid && item.message_id == message.message_id)
        }) {
            self.sender_pane.borrow_mut().include(group.iter().copied());
        }
        self.render_list();
        let target = (message.uid, message.message_id.clone());
        *self.notification_message.borrow_mut() = Some(target.clone());
        if !self.select_notification_message() {
            self.show_thread(vec![message]);
            *self.notification_message.borrow_mut() = Some(target);
        }
    }

    pub(super) fn select_notification_message(self: &Rc<Self>) -> bool {
        let Some((uid, id)) = self.notification_message.borrow().clone() else {
            return false;
        };
        let target =
            (0..ui::virtual_list::model(&self.thread_list).n_items()).find_map(|position| {
                ui::virtual_list::with_item(&self.thread_list, position, |row| {
                    row.messages
                        .iter()
                        .any(|message| message.uid == uid && message.message_id == id)
                        .then(|| (position, row.messages.clone()))
                })
                .flatten()
            });
        if let Some((position, group)) = target {
            self.notification_message.borrow_mut().take();
            ui::virtual_list::selection(&self.thread_list).set_selected(position);
            self.show_thread(group);
            if let Some(card) = self.cards.borrow().get(&uid) {
                card.expand();
            }
            ui::virtual_list::reveal(&self.thread_list, position);
            return true;
        }
        if !self.sender_pane.borrow().loading() && self.sender_pane.borrow().can_load() {
            self.load_sender_page();
        }
        false
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "requires a graphical session; uses recording workers without mail access"]
    fn notification_actions_update_immediately_and_reconcile_success_or_failure() {
        use models::{action_target::ActionTarget, sender_action::SenderAction};

        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.ExposedCat.Brevlada.NotificationFeedbackDiagnostics")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        for action in [
            SenderAction::MarkRead,
            SenderAction::Archive,
            SenderAction::Delete,
        ] {
            let shell = ui::shell::Shell::new(&app);
            let window = shell.window.clone();
            let (worker, commands) = worker::Worker::recording();
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
                    uid_validity: Some(7),
                    timestamp: i64::from(uid),
                    message_id: uid.to_string(),
                    subject: format!("Message {uid}"),
                    sender: "sender@example.com".into(),
                    body_loaded: true,
                    is_read: action.removes_messages(),
                    ..Message::default()
                })
                .collect();
            let target = ActionTarget::Messages(vec![(3, "3".into())]);
            *state.messages.borrow_mut() = messages.clone();
            state
                .folder_cache
                .borrow_mut()
                .insert(("account".into(), "INBOX".into()), messages.clone());
            state.render_list();
            state.filter_sender(Some(messages[0].clone()));
            let ticket = state.sender_pane.borrow().ticket();
            state.event(Event::SenderPage(
                state.generation.get(),
                ticket,
                messages.clone(),
                false,
            ));
            if action.removes_messages() {
                state.show_thread(vec![messages[0].clone()]);
            }
            // A stale notification and an unrelated account must not project a change.
            state.event(Event::NotificationActionStarted {
                account: "account".into(),
                validity: 8,
                uid: 3,
                action: action.clone(),
            });
            state.event(Event::NotificationActionStarted {
                account: "other".into(),
                validity: 7,
                uid: 3,
                action: action.clone(),
            });
            assert!(state.notification_actions.borrow().is_empty());
            state.event(Event::NotificationActionStarted {
                account: "account".into(),
                validity: 7,
                uid: 3,
                action: action.clone(),
            });
            assert_eq!(state.notification_actions.borrow().len(), 1);
            let check_pending = || {
                let visible = state.visible_messages();
                if action.removes_messages() {
                    assert!(visible.iter().all(|message| message.uid != 3));
                    assert!(!state.selected.borrow().contains(&3));
                    assert!(state.selected.borrow().contains(&2));
                    assert!(
                        ui::virtual_list::groups(&state.thread_list)
                            .iter()
                            .flatten()
                            .all(|message| message.uid != 3)
                    );
                } else {
                    assert!(
                        visible
                            .iter()
                            .find(|message| message.uid == 3)
                            .unwrap()
                            .is_read
                    );
                    assert!(
                        visible
                            .iter()
                            .filter(|message| message.uid != 3)
                            .all(|message| !message.is_read)
                    );
                    assert!(
                        ui::virtual_list::groups(&state.thread_list)
                            .iter()
                            .flatten()
                            .find(|message| message.uid == 3)
                            .unwrap()
                            .is_read
                    );
                }
            };
            check_pending();
            state.event(Event::CacheList(
                "account".into(),
                "INBOX".into(),
                messages.clone(),
            ));
            check_pending();
            // Authentication and stale-target failures happen before SenderActionFinished.
            state.event(Event::NotificationActionFinished(
                "account".into(),
                7,
                3,
                Some("Connection failed".into()),
            ));
            assert!(state.notification_actions.borrow().is_empty());
            assert_eq!(
                state
                    .visible_messages()
                    .iter()
                    .find(|message| message.uid == 3)
                    .unwrap()
                    .is_read,
                messages[0].is_read
            );
            state.event(Event::NotificationActionStarted {
                account: "account".into(),
                validity: 7,
                uid: 3,
                action: action.clone(),
            });
            let confirmed = if action.removes_messages() {
                messages[1..].to_vec()
            } else {
                let mut confirmed = messages.clone();
                confirmed[0].is_read = true;
                confirmed[0].read_revision = 10;
                confirmed
            };
            state.event(Event::SenderActionFinished {
                account: "account".into(),
                folder: "INBOX".into(),
                sender: target,
                messages: Some(confirmed),
                error: None,
                removed: if action.removes_messages() {
                    vec![messages[0].clone()]
                } else {
                    Vec::new()
                },
            });
            state.event(Event::NotificationActionFinished(
                "account".into(),
                7,
                3,
                None,
            ));
            assert!(state.notification_actions.borrow().is_empty());
            state.event(Event::CacheList(
                "account".into(),
                "INBOX".into(),
                messages.clone(),
            ));
            check_pending();
            assert!(!commands.try_iter().any(|command| matches!(
                command,
                Command::SenderAction { .. } | Command::NotificationAction { .. }
            )));
            window.close();
        }
    }

    #[test]
    #[ignore = "requires a graphical session; no mail workers"]
    fn notification_selects_its_message_in_the_full_folder_and_sender_history() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.ExposedCat.Brevlada.NotificationNavigationDiagnostics")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        let account = Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        state.event(Event::Accounts(vec![account.clone()]));
        let mut messages: Vec<_> = (1..=4)
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                uid_validity: Some(1),
                message_id: uid.to_string(),
                subject: format!("Message {uid}"),
                is_read: uid != 1,
                body_loaded: true,
                sender: if uid == 4 {
                    "another@example.com"
                } else {
                    "sender@example.com"
                }
                .into(),
                ..Message::default()
            })
            .collect();
        messages[1].references = vec![messages[0].message_id.clone()];
        messages[1].in_reply_to = Some(messages[0].message_id.clone());
        state
            .folder_cache
            .borrow_mut()
            .insert((account.email.clone(), "INBOX".into()), messages.clone());
        state.event(Event::NotificationOpened(
            account.email.clone(),
            Ok(Some(messages[1].clone())),
        ));
        assert!(state.search_sender.borrow().is_none());
        assert!(!state.sender_search_active());
        assert_eq!(state.messages.borrow().len(), 4);
        assert_eq!(*state.selected.borrow(), [2, 1]);
        assert!(ui::virtual_list::selected(&state.thread_list).is_some());
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![messages[2].clone()],
            true,
        ));
        assert!(state.notification_message.borrow().is_none());
        state.load_sender_page();
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![messages[1].clone(), messages[0].clone()],
            false,
        ));
        assert!(state.notification_message.borrow().is_none());
        let groups = ui::virtual_list::groups(&state.thread_list);
        assert_eq!(groups.iter().map(Vec::len).sum::<usize>(), 3);
        assert_eq!(*state.selected.borrow(), [2, 1]);
        assert!(state.cards.borrow()[&2].is_expanded());
        assert_eq!(
            groups[ui::virtual_list::selected(&state.thread_list).unwrap() as usize][0].uid,
            2
        );
        let card = state.cards.borrow()[&2].widget.clone();
        let selection = state.selection.get();
        state
            .viewer_scroll
            .vadjustment()
            .configure(180.0, 0.0, 1500.0, 10.0, 100.0, 400.0);
        state.pending.borrow_mut().insert(2);
        commands.try_iter().for_each(drop);
        state.event(Event::NotificationOpened(
            account.email.clone(),
            Ok(Some(messages[1].clone())),
        ));
        assert_eq!(state.cards.borrow()[&2].widget, card);
        assert_eq!(state.selection.get(), selection);
        assert_eq!(state.viewer_scroll.vadjustment().value(), 180.0);
        assert!(state.pending.borrow().contains(&2));
        assert!(commands.try_recv().is_err());
        state.show_thread(
            groups[ui::virtual_list::selected(&state.thread_list).unwrap() as usize].clone(),
        );
        assert_eq!(state.cards.borrow()[&2].widget, card);
        assert_eq!(state.selection.get(), selection);
        assert_eq!(state.viewer_scroll.vadjustment().value(), 180.0);
        assert!(state.pending.borrow().contains(&2));
        assert!(commands.try_recv().is_err());
        assert!(
            state
                .messages
                .borrow()
                .iter()
                .any(|message| message.uid == 1 && !message.is_read)
        );
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            Message {
                is_read: true,
                read_revision: 10,
                ..messages[0].clone()
            },
        ));
        assert!(
            state
                .sender_pane
                .borrow()
                .messages()
                .iter()
                .all(|message| message.is_read)
        );
        assert!(
            ui::virtual_list::groups(&state.list)
                .iter()
                .flatten()
                .all(|message| message.is_read)
        );
        assert!(!state.sender_pane.borrow().can_load());
        let arrival = Message {
            uid: 5,
            timestamp: 5,
            message_id: "5".into(),
            references: vec!["2".into()],
            subject: "Re: Message 2".into(),
            ..messages[1].clone()
        };
        state.event(Event::NotificationOpened(
            account.email.clone(),
            Ok(Some(arrival.clone())),
        ));
        assert_eq!(*state.selected.borrow(), [5, 2, 1]);
        assert!(state.notification_message.borrow().is_none());
        assert!(!state.sender_pane.borrow().can_load());
        let position = ui::virtual_list::selected(&state.thread_list).unwrap();
        assert_eq!(
            ui::virtual_list::item(&state.thread_list, position)
                .unwrap()
                .messages[0]
                .uid,
            5
        );
        assert!(state.cards.borrow()[&5].is_expanded());
        state.filter_sender(None);
        let position = ui::virtual_list::position(&state.list, "sender@example.com").unwrap();
        state.activate_sender(position);
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![
                arrival.clone(),
                messages[2].clone(),
                messages[1].clone(),
                messages[0].clone(),
            ],
            false,
        ));
        let groups = ui::virtual_list::groups(&state.thread_list);
        let position = groups
            .iter()
            .position(|group| group.iter().any(|message| message.uid == 5))
            .unwrap() as u32;
        ui::virtual_list::selection(&state.thread_list).set_selected(position);
        state
            .thread_list
            .emit_by_name::<()>("activate", &[&position]);
        assert_eq!(*state.selected.borrow(), [5, 2, 1]);
        let next = Message {
            uid: 7,
            timestamp: 7,
            message_id: "7".into(),
            subject: "A new conversation".into(),
            ..messages[2].clone()
        };
        let mut latest = state.messages.borrow().clone();
        latest.push(next);
        state.event(Event::Messages(state.generation.get(), latest, false));
        assert!(
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .any(|message| message.uid == 7)
        );
        assert_eq!(*state.selected.borrow(), [5, 2, 1]);
        assert!(!state.sender_pane.borrow().can_load());
        let older = Message {
            uid: 8,
            timestamp: 8,
            message_id: "8".into(),
            sender: "new-sender@example.com".into(),
            subject: "Another topic".into(),
            is_read: false,
            ..messages[2].clone()
        };
        let newest = Message {
            uid: 9,
            timestamp: 9,
            message_id: "9".into(),
            references: vec!["8".into()],
            is_read: true,
            ..older.clone()
        };
        state.event(Event::NotificationOpened(
            account.email,
            Ok(Some(newest.clone())),
        ));
        assert_eq!(*state.selected.borrow(), [9]);
        assert!(ui::virtual_list::selected(&state.thread_list).is_some());
        let selection = state.selection.get();
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![newest, older],
            false,
        ));
        assert_eq!(*state.selected.borrow(), [9, 8]);
        assert_eq!(state.selection.get(), selection);
        assert!(state.cards.borrow()[&9].is_expanded());
        window.destroy();
    }
}
