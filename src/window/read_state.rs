use super::*;

impl State {
    pub(super) fn merge_read_states(&self, messages: &mut [Message]) {
        let folder = self.messages.borrow();
        let pane = self.sender_pane.borrow();
        let opened = self.open_group.borrow();
        models::read_state::merge(
            messages,
            folder.iter().chain(pane.messages()).chain(opened.iter()),
        );
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::{action_target::ActionTarget, sender_action::SenderAction};

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn deleting_mail_keeps_the_next_message_read_despite_late_snapshots() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaReadDiagnostic")
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
                uid_validity: Some(7),
                timestamp: uid as i64,
                sender: "sender@example.com".into(),
                message_id: uid.to_string(),
                subject: format!("Conversation {uid}"),
                body_loaded: true,
                is_read: false,
                read_revision: 1,
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages.clone();
        state.render_list();
        state.filter_sender(Some(messages[0].clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            messages.clone(),
            false,
        ));
        state.show_thread(vec![messages[0].clone()]);
        state.sender_action(
            ActionTarget::Messages(vec![(3, "3".into())]),
            SenderAction::Delete,
        );
        assert_eq!(*state.selected.borrow(), vec![2]);
        assert!(state.pending.borrow().contains(&2));
        let initial = messages[1].clone();
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            initial.clone(),
        ));
        assert!(state.pending.borrow().contains(&2));
        let read = Message {
            is_read: true,
            read_revision: 3,
            ..initial.clone()
        };
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            read.clone(),
        ));
        assert!(!state.pending.borrow().contains(&2));
        let stale = Message {
            read_revision: 2,
            ..initial.clone()
        };
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: ActionTarget::Messages(vec![(3, "3".into())]),
            messages: Some(vec![stale.clone(), messages[2].clone()]),
            removed: vec![messages[0].clone()],
            error: None,
        });
        state.event(Event::CacheList(
            "account".into(),
            "INBOX".into(),
            vec![stale.clone(), messages[2].clone()],
        ));
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            stale,
        ));
        assert!(!state.pending.borrow().contains(&2));
        assert!(
            state
                .messages
                .borrow()
                .iter()
                .find(|message| message.uid == 2)
                .unwrap()
                .is_read
        );
        assert!(
            state
                .sender_pane
                .borrow()
                .messages()
                .iter()
                .find(|message| message.uid == 2)
                .unwrap()
                .is_read
        );
        assert!(state.open_group.borrow()[0].is_read);
        assert!(
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .find(|message| message.uid == 2)
                .unwrap()
                .is_read
        );
        let fresh = Message {
            read_revision: 4,
            ..initial
        };
        state.event(Event::CacheList(
            "account".into(),
            "INBOX".into(),
            vec![fresh, messages[2].clone()],
        ));
        assert!(
            !state
                .messages
                .borrow()
                .iter()
                .find(|message| message.uid == 2)
                .unwrap()
                .is_read
        );
        assert!(state.pending.borrow().contains(&2));
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            read,
        ));
        assert!(!state.open_group.borrow()[0].is_read);
        assert!(state.pending.borrow().contains(&2));
        state.event(Event::BodyError(
            state.generation.get(),
            state.selection.get(),
            2,
            "Read update failed".into(),
        ));
        assert!(!state.pending.borrow().contains(&2));
        assert!(!state.open_group.borrow()[0].is_read);
        window.close();
    }
}
