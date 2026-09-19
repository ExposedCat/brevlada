use super::*;
use models::sender_action::SenderAction;

impl State {
    pub(super) fn sender_action(
        &self,
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
        if action != SenderAction::MarkRead
            && self.messages.borrow().iter().any(|message| {
                sender.matches(message) && self.selected.borrow().contains(&message.uid)
            })
        {
            self.selected.borrow_mut().clear();
            self.cards.borrow_mut().clear();
            ui::states::select_message(&self.viewer);
        }
        self.refresh_sender_view();
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
        window.close();
    }
}
