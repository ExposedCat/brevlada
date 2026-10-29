use super::*;
use models::{action_target::ActionTarget, sender_action::SenderAction};

impl State {
    pub(super) fn record_undo(
        self: &Rc<Self>,
        account: &Account,
        folder: &str,
        target: &ActionTarget,
        action: &SenderAction,
        messages: Vec<Message>,
    ) -> Option<u64> {
        if !action.undoable_move() {
            return None;
        }
        let group = self.undo_group.get();
        let id = group.unwrap_or_else(|| self.undo_history.borrow_mut().begin(action.clone()));
        self.undo_history
            .borrow_mut()
            .register(id, &account.email, folder, target);
        self.undo_history.borrow_mut().remember(
            id,
            models::undo::Snapshot {
                opened: self
                    .open_group
                    .borrow()
                    .iter()
                    .filter(|message| target.matches(message))
                    .map(|message| (message.uid, message.message_id.clone()))
                    .collect(),
                target: target.clone(),
                account: account.email.clone(),
                folder: folder.into(),
                messages,
            },
        );
        if group.is_none() {
            self.show_undo_toast(id, action);
        }
        Some(id)
    }

    pub(super) fn show_undo_toast(self: &Rc<Self>, id: u64, action: &SenderAction) {
        if let Some((_, toast)) = self.undo_toast.borrow_mut().take() {
            toast.dismiss();
        }
        let weak = Rc::downgrade(self);
        let toast = ui::undo_toast::new(
            match action {
                SenderAction::Delete => "Messages deleted",
                SenderAction::Archive => "Messages archived",
                _ => "Messages moved",
            },
            move || {
                if let Some(state) = weak.upgrade() {
                    state.undo_action(Some(id));
                }
            },
        );
        self.toast.add_toast(toast.clone());
        *self.undo_toast.borrow_mut() = Some((id, toast));
    }

    pub(super) fn undo_action(self: &Rc<Self>, id: Option<u64>) -> bool {
        let Some(id) = self.undo_history.borrow_mut().request(id) else {
            return false;
        };
        let dismiss = self
            .undo_toast
            .borrow()
            .as_ref()
            .is_some_and(|(current, _)| *current == id);
        if dismiss && let Some((_, toast)) = self.undo_toast.borrow_mut().take() {
            toast.dismiss();
        }
        let snapshots = self.undo_history.borrow().snapshots(id);
        for snapshot in &snapshots {
            self.sender_actions.borrow_mut().restore_optimistically(
                &snapshot.account,
                &snapshot.folder,
                &snapshot.messages,
            );
        }
        self.cancel_deferred_undo(id);
        self.focus_undo_restoration(&snapshots);
        self.dispatch_undo();
        true
    }

    pub(super) fn dispatch_undo(self: &Rc<Self>) {
        let ready = self.undo_history.borrow_mut().take_ready();
        for action in ready {
            if let Err(error) = self.sender.send(Command::UndoAction(action.clone())) {
                for batch in &action.batches {
                    self.sender_actions.borrow_mut().finish_optimistic_restore(
                        &batch.account.email,
                        &batch.folder,
                        &batch.messages,
                    );
                }
                self.refresh_sender_view();
                self.undo_history.borrow_mut().retry(action.clone());
                self.show_undo_toast(action.id, &action.action);
                self.toast.add_toast(adw::Toast::new(&error.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn bulk_delete_and_archive_support_undo_toasts_and_ctrl_z() {
        check_undo(false);
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn gmail_undo_reveals_restored_messages_with_unchanged_inbox_uids() {
        check_undo(true);
    }

    fn check_undo(keep_uid: bool) {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaUndoDiagnostic",
            worker,
        );
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.set_title(Some("Brevlada Undo diagnostic"));
        let account = state.account.borrow().clone().unwrap();
        let messages: Vec<_> = (1..=4)
            .map(|uid| Message {
                uid,
                uid_validity: Some(7),
                message_id: uid.to_string(),
                sender: if uid <= 2 {
                    "alice@example.com"
                } else {
                    "bob@example.com"
                }
                .into(),
                subject: format!("Message {uid}"),
                timestamp: i64::from(uid),
                is_read: true,
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages.clone();
        state.render_list();
        window.present();
        while glib::MainContext::default().iteration(false) {}
        ui::virtual_list::selection(&state.list).select_range(0, 2, true);
        assert!(state.selected_list_action(SenderAction::Delete));
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<adw::MessageDialog>().ok())
            .find(|dialog| dialog.is_visible())
            .unwrap();
        dialog.response("confirm");
        let targets: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
            .filter_map(|command| match command {
                Command::SenderAction {
                    sender,
                    action: SenderAction::Delete,
                    ..
                } => Some(sender),
                _ => None,
            })
            .collect();
        assert_eq!(targets.len(), 2);
        let toast = state.undo_toast.borrow().as_ref().unwrap().1.clone();
        assert_eq!(toast.button_label().as_deref(), Some("Undo"));
        toast.emit_by_name::<()>("button-clicked", &[]);
        assert_eq!(state.visible_messages().len(), 4);
        assert_eq!(ui::virtual_list::groups(&state.list).len(), 2);
        assert!(!state.undo_action(None));
        assert!(
            !std::iter::from_fn(|| commands.try_recv().ok())
                .any(|command| matches!(command, Command::UndoAction(_)))
        );
        for (index, target) in targets.iter().enumerate() {
            let removed: Vec<_> = messages
                .iter()
                .filter(|message| target.matches(message))
                .cloned()
                .collect();
            state.event(Event::SenderActionFinished {
                account: account.email.clone(),
                folder: "INBOX".into(),
                sender: target.clone(),
                messages: Some(Vec::new()),
                removed: removed.clone(),
                error: None,
            });
            state.event(Event::ActionUndoReady {
                account: account.clone(),
                folder: "INBOX".into(),
                sender: target.clone(),
                destination: Some("Trash".into()),
                removed,
            });
            assert_eq!(state.visible_messages().len(), 4);
            assert_eq!(ui::virtual_list::groups(&state.list).len(), 2);
            if index == 0 {
                assert!(
                    !std::iter::from_fn(|| commands.try_recv().ok())
                        .any(|command| matches!(command, Command::UndoAction(_)))
                );
            }
        }
        let mut undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(undo.batches.len(), 2);
        assert_eq!(
            undo.batches
                .iter()
                .map(|batch| batch.messages.len())
                .sum::<usize>(),
            4
        );
        assert!(
            undo.batches
                .iter()
                .all(|batch| batch.folder == "INBOX" && batch.destination == "Trash")
        );
        let restored: Vec<_> = messages
            .iter()
            .map(|message| Message {
                uid: if keep_uid {
                    message.uid
                } else {
                    message.uid + 100
                },
                ..message.clone()
            })
            .collect();
        state.event(Event::MessagesRestored {
            account: account.email.clone(),
            folder: "INBOX".into(),
            originals: messages.clone(),
            restored: messages.clone(),
            messages: Some(restored.clone()),
            error: None,
        });
        undo.batches.clear();
        state.event(Event::UndoActionFinished(undo));
        assert_eq!(state.visible_messages().len(), 4);
        assert_eq!(
            state
                .visible_messages()
                .iter()
                .map(|message| message.uid)
                .collect::<HashSet<_>>(),
            restored
                .iter()
                .map(|message| message.uid)
                .collect::<HashSet<_>>()
        );
        let target =
            ActionTarget::Messages(vec![(restored[0].uid, restored[0].message_id.clone())]);
        state.sender_action(target.clone(), SenderAction::Archive);
        let toast = state.undo_toast.borrow().as_ref().unwrap().1.clone();
        assert_eq!(toast.title().as_deref(), Some("Messages archived"));
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(restored[1..].to_vec()),
            removed: vec![restored[0].clone()],
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Custom archive".into()),
            removed: vec![restored[0].clone()],
        });
        *state.folder.borrow_mut() = "Work".into();
        let keys = window
            .observe_controllers()
            .iter::<glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|controller| controller.name().as_deref() == Some("mail-undo-shortcut"))
            .unwrap();
        let press = |modifiers| {
            keys.emit_by_name::<bool>("key-pressed", &[&gtk::gdk::Key::z, &0u32, &modifiers])
        };
        assert!(state.search.grab_focus());
        assert!(!press(gtk::gdk::ModifierType::CONTROL_MASK));
        state.refresh.grab_focus();
        assert!(!press(
            gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK
        ));
        assert!(press(gtk::gdk::ModifierType::CONTROL_MASK));
        let undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(undo.batches.len(), 1);
        assert_eq!(undo.batches[0].folder, "INBOX");
        assert_eq!(undo.batches[0].destination, "Custom archive");
        *state.folder.borrow_mut() = "INBOX".into();
        state.refresh_sender_view();
        assert_eq!(state.visible_messages().len(), 4);
        state.event(Event::CacheList(
            account.email.clone(),
            "INBOX".into(),
            restored[1..].to_vec(),
        ));
        assert_eq!(state.visible_messages().len(), 4);
        assert!(!press(gtk::gdk::ModifierType::CONTROL_MASK));
        window.close();
    }
}
