use super::*;
use models::sender_action::SenderAction;

impl State {
    pub(super) fn sender_action(
        self: &Rc<Self>,
        sender: models::action_target::ActionTarget,
        action: SenderAction,
    ) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let folder = self.folder.borrow().clone();
        if !self
            .sender_actions
            .borrow_mut()
            .begin(&account.email, &folder, &sender, action)
        {
            return;
        }
        if let Err(error) = self.sender.send(Command::SenderAction {
            account: account.clone(),
            folder: folder.clone(),
            sender: sender.clone(),
            action,
        }) {
            self.sender_actions
                .borrow_mut()
                .finish(&account.email, &folder, &sender);
            self.toast.add_toast(adw::Toast::new(&error.to_string()));
            return;
        }
        self.new_selection();
        let replace_open_message = action != SenderAction::MarkRead
            && self.messages.borrow().iter().any(|message| {
                sender.matches(message) && self.selected.borrow().contains(&message.uid)
            });
        if replace_open_message {
            self.deferred_read_sort.borrow_mut().clear();
            self.selected.borrow_mut().clear();
            self.cards.borrow_mut().clear();
            ui::states::select_message(&self.viewer);
        }
        self.refresh_sender_view();
        if replace_open_message {
            self.open_first_remaining();
        }
    }

    fn open_first_remaining(self: &Rc<Self>) {
        if self.thread_groups.borrow().is_empty() || self.selected_sender.borrow().is_none() {
            let first = self
                .groups
                .borrow()
                .first()
                .and_then(|group| group.first())
                .cloned();
            let Some(first) = first else {
                return;
            };
            if let Some(row) = self.list.row_at_index(0) {
                self.list.select_row(Some(&row));
            }
            self.filter_sender(Some(first));
        }
        let group = self.thread_groups.borrow().first().cloned();
        if let Some(group) = group {
            if let Some(row) = self.thread_list.row_at_index(0) {
                self.thread_list.select_row(Some(&row));
            }
            self.show_thread(group);
        }
    }

    pub(super) fn visible_message(&self, message: &Message) -> Option<Message> {
        let account = self.account.borrow();
        let Some(account) = account.as_ref() else {
            return Some(message.clone());
        };
        self.sender_actions
            .borrow()
            .project(&account.email, &self.folder.borrow(), message)
    }

    pub(super) fn visible_messages(&self) -> Vec<Message> {
        self.messages
            .borrow()
            .iter()
            .filter_map(|message| self.visible_message(message))
            .collect()
    }

    pub(super) fn refresh_sender_view(&self) {
        for message in self.visible_messages() {
            if let Some(card) = self.cards.borrow().get(&message.uid) {
                card.update(&message);
            }
        }
        self.render_list();
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn sender_action_feedback_is_immediate_and_failure_restores_server_state() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSenderDiagnostic")
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
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let message = Message {
            uid: 1,
            sender: "sender@example.com".into(),
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![message.clone()];
        state.render_list();
        for action in [
            SenderAction::MarkRead,
            SenderAction::Archive,
            SenderAction::Delete,
            SenderAction::Spam,
        ] {
            state.sender_action("sender@example.com".into(), action);
            assert!(
                matches!(commands.try_recv().unwrap(), Command::SenderAction { action: sent, .. } if sent == action)
            );
            if action == SenderAction::MarkRead {
                assert!(state.groups.borrow()[0][0].is_read);
            } else {
                assert!(state.groups.borrow().is_empty());
            }
            state.event(Event::Messages(
                state.generation.get(),
                vec![message.clone()],
                false,
            ));
            if action == SenderAction::MarkRead {
                assert!(state.groups.borrow()[0][0].is_read);
            } else {
                assert!(state.groups.borrow().is_empty());
            }
            state.event(Event::SenderActionFinished {
                account: "account".into(),
                folder: "INBOX".into(),
                sender: "sender@example.com".into(),
                messages: None,
                error: Some("Server refused action".into()),
            });
            assert_eq!(state.groups.borrow()[0][0], message);
        }
        state.sender_action("sender@example.com".into(), SenderAction::Delete);
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: "sender@example.com".into(),
            messages: Some(Vec::new()),
            error: None,
        });
        assert!(state.messages.borrow().is_empty());
        assert!(state.groups.borrow().is_empty());
        let other = Message {
            uid: 2,
            subject: "Other conversation".into(),
            message_id: "other".into(),
            ..message.clone()
        };
        *state.messages.borrow_mut() = vec![message.clone(), other.clone()];
        state.filter_sender(Some(message.clone()));
        let target = models::action_target::ActionTarget::Messages(vec![(
            message.uid,
            message.message_id.clone(),
        )]);
        for action in [
            SenderAction::MarkRead,
            SenderAction::Archive,
            SenderAction::Delete,
            SenderAction::Spam,
        ] {
            state.sender_action(target.clone(), action);
            let visible = state.visible_messages();
            assert_eq!(
                visible.iter().find(|message| message.uid == 2),
                Some(&other)
            );
            if action == SenderAction::MarkRead {
                assert!(
                    visible
                        .iter()
                        .find(|message| message.uid == 1)
                        .unwrap()
                        .is_read
                );
            } else {
                assert!(visible.iter().all(|message| message.uid != 1));
            }
            state.event(Event::Messages(
                state.generation.get(),
                vec![message.clone(), other.clone()],
                false,
            ));
            assert_eq!(state.visible_messages(), visible);
            state.event(Event::SenderActionFinished {
                account: "account".into(),
                folder: "INBOX".into(),
                sender: target.clone(),
                messages: None,
                error: Some("Server refused action".into()),
            });
            assert_eq!(
                state.visible_messages(),
                vec![message.clone(), other.clone()]
            );
        }
        // Reading updates the styling immediately but defers unread-first sorting
        // until another conversation is opened, including across list refreshes.
        let unread = Message {
            uid: 10,
            timestamp: 1,
            subject: "Unread".into(),
            body_loaded: true,
            ..message.clone()
        };
        let newer = Message {
            uid: 11,
            timestamp: 2,
            subject: "Newer read".into(),
            is_read: true,
            ..unread.clone()
        };
        let elsewhere = Message {
            uid: 12,
            timestamp: 3,
            sender: "elsewhere@example.com".into(),
            ..newer.clone()
        };
        *state.messages.borrow_mut() = vec![unread.clone(), newer.clone(), elsewhere.clone()];
        state.filter_sender(Some(unread.clone()));
        let row = state.thread_list.row_at_index(0).unwrap();
        state.thread_list.select_row(Some(&row));
        state.show_thread(vec![unread.clone()]);
        let read = Message {
            is_read: true,
            ..unread.clone()
        };
        state.update_body(&read);
        assert_eq!(state.thread_list.row_at_index(0).unwrap(), row);
        assert!(state.thread_groups.borrow()[0][0].is_read);
        assert_eq!(state.groups.borrow()[0][0].sender, unread.sender);
        let snapshot = state.messages.borrow().clone();
        state.event(Event::Messages(state.generation.get(), snapshot, false));
        state.show_thread(vec![read.clone()]);
        assert_eq!(state.thread_list.row_at_index(0).unwrap(), row);
        state.show_thread(vec![newer.clone()]);
        assert_eq!(state.thread_list.row_at_index(1).unwrap(), row);
        assert_eq!(state.groups.borrow()[0][0].uid, elsewhere.uid);

        // Removing the open conversation selects AND opens its replacement.
        // Removing the last conversation from a sender moves to the next sender.
        for (removed, action, expected) in [
            (newer, SenderAction::Archive, Some(read.clone())),
            (read, SenderAction::Delete, Some(elsewhere.clone())),
            (elsewhere, SenderAction::Delete, None),
        ] {
            assert_eq!(*state.selected.borrow(), vec![removed.uid]);
            state.compose_button.grab_focus();
            let keys = window
                .observe_controllers()
                .iter::<gtk::glib::Object>()
                .filter_map(Result::ok)
                .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
                .find(|controller| controller.name().as_deref() == Some("open-message-shortcuts"))
                .unwrap();
            let key = if action == SenderAction::Archive {
                gtk::gdk::Key::BackSpace
            } else {
                gtk::gdk::Key::Delete
            };
            assert!(keys.emit_by_name::<bool>(
                "key-pressed",
                &[&key, &0u32, &gtk::gdk::ModifierType::empty(),]
            ));
            if let Some(expected) = expected {
                assert_eq!(*state.selected.borrow(), vec![expected.uid]);
                assert!(state.cards.borrow()[&expected.uid].is_expanded());
                assert_eq!(state.thread_list.selected_row().unwrap().index(), 0);
                assert_eq!(state.thread_groups.borrow()[0][0].uid, expected.uid);
            } else {
                assert!(state.selected.borrow().is_empty());
                assert!(state.cards.borrow().is_empty());
                assert!(state.groups.borrow().is_empty());
            }
        }
        window.close();
    }
}
