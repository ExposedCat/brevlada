use super::*;

use models::{action_target::ActionTarget, sender_action::SenderAction};

pub(super) struct DeferredAction {
    key: u64,
    undo: Option<u64>,
    account: Account,
    folder: String,
    target: ActionTarget,
    action: SenderAction,
    messages: Vec<Message>,
}

impl State {
    pub(super) fn defer_restored_action(
        self: &Rc<Self>,
        account: &Account,
        folder: &str,
        target: &ActionTarget,
        action: &SenderAction,
        messages: &[Message],
        advance: bool,
    ) -> bool {
        if !self
            .sender_actions
            .borrow()
            .restoring(&account.email, folder, target)
        {
            return false;
        }
        let key = self.sender.next_request();
        let undo = self.record_undo(account, folder, target, action, messages.to_vec());
        self.sender_actions.borrow_mut().defer(
            key,
            &account.email,
            folder,
            target.clone(),
            action.clone(),
        );
        self.deferred_actions.borrow_mut().push(DeferredAction {
            key,
            undo,
            account: account.clone(),
            folder: folder.into(),
            target: target.clone(),
            action: action.clone(),
            messages: messages.to_vec(),
        });
        self.show_pending_action_with_navigation(target, action, advance);
        true
    }

    pub(super) fn cancel_deferred_undo(&self, id: u64) {
        self.deferred_actions.borrow_mut().retain(|request| {
            if request.undo != Some(id) {
                return true;
            }
            self.sender_actions
                .borrow_mut()
                .finish_deferred(request.key);
            self.undo_history.borrow_mut().cancel_pending(
                id,
                &request.account.email,
                &request.folder,
                &request.target,
            );
            false
        });
    }

    pub(super) fn rebind_deferred_actions(
        &self,
        account: &str,
        folder: &str,
        originals: &[Message],
        restored: &[Message],
        messages: Option<&[Message]>,
    ) {
        self.deferred_actions.borrow_mut().retain_mut(|request| {
            if request.account.email != account || request.folder != folder {
                return true;
            }
            let before = request.messages.clone();
            let old_target = request.target.clone();
            for message in &mut request.messages {
                let Some(original) = originals.iter().find(|original| {
                    original.uid == message.uid && original.message_id == message.message_id
                }) else {
                    continue;
                };
                let replacement = restored
                    .iter()
                    .any(|item| item.uid == original.uid && item.message_id == original.message_id)
                    .then(|| {
                        messages.unwrap_or_default().iter().find(|candidate| {
                            candidate.message_id == original.message_id
                                && models::senders::key(candidate) == models::senders::key(original)
                                && candidate.subject == original.subject
                                && candidate.timestamp == original.timestamp
                        })
                    })
                    .flatten();
                let Some(replacement) = replacement else {
                    self.sender_actions
                        .borrow_mut()
                        .finish_deferred(request.key);
                    if let Some(id) = request.undo {
                        self.undo_history.borrow_mut().cancel_pending(
                            id,
                            &request.account.email,
                            &request.folder,
                            &old_target,
                        );
                    }
                    return false;
                };
                if let ActionTarget::Messages(targets) = &mut request.target {
                    for (uid, id) in targets {
                        if *uid == message.uid && *id == message.message_id {
                            *uid = replacement.uid;
                        }
                    }
                }
                message.uid = replacement.uid;
                message.uid_validity = replacement.uid_validity;
                message.is_read = replacement.is_read;
                message.read_revision = replacement.read_revision;
                message.is_flagged = replacement.is_flagged;
                message.flag_revision = replacement.flag_revision;
            }
            self.sender_actions.borrow_mut().defer(
                request.key,
                account,
                folder,
                request.target.clone(),
                request.action.clone(),
            );
            if let Some(id) = request.undo {
                self.undo_history.borrow_mut().retarget(
                    id,
                    (account, folder),
                    &old_target,
                    &request.target,
                    &before,
                    &request.messages,
                );
            }
            true
        });
    }

    pub(super) fn dispatch_deferred_actions(self: &Rc<Self>) {
        loop {
            let ready = self.deferred_actions.borrow().iter().position(|request| {
                !self.sender_actions.borrow().restoring(
                    &request.account.email,
                    &request.folder,
                    &request.target,
                )
            });
            let Some(index) = ready else {
                return;
            };
            let request = self.deferred_actions.borrow_mut().remove(index);
            if !self.sender_actions.borrow_mut().begin(
                &request.account.email,
                &request.folder,
                &request.target,
                request.action.clone(),
            ) {
                self.deferred_actions.borrow_mut().insert(index, request);
                return;
            }
            self.sender_actions
                .borrow_mut()
                .finish_deferred(request.key);
            if let Err(error) = self.sender.send(Command::SenderAction {
                account: request.account.clone(),
                folder: request.folder.clone(),
                sender: request.target.clone(),
                action: request.action,
            }) {
                self.sender_actions.borrow_mut().finish(
                    &request.account.email,
                    &request.folder,
                    &request.target,
                );
                if let Some(id) = request.undo {
                    self.undo_history.borrow_mut().cancel_pending(
                        id,
                        &request.account.email,
                        &request.folder,
                        &request.target,
                    );
                }
                self.refresh_sender_view();
                self.toast.add_toast(adw::Toast::new(&error.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn run(arguments: &[&str]) -> String {
        let helper = std::env::var("BREVLADA_GUI_HELPER").unwrap();
        let session = std::env::var("BREVLADA_GUI_SESSION").unwrap();
        let result = std::process::Command::new("/usr/bin/python3")
            .arg(helper)
            .args(["--session", &session])
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }

    fn settle() {
        let until = std::time::Instant::now() + Duration::from_millis(120);
        while std::time::Instant::now() < until {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_archive_delete_repeat_without_waiting_for_mail_server() {
        check_native_repetition(true, false, false);
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_sender_hotkeys_repeat_without_waiting_for_mail_server() {
        check_native_repetition(false, false, false);
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_thread_hotkeys_continue_across_senders_without_server_or_cache_replies() {
        check_native_repetition(true, true, false);
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_hotkeys_keep_working_after_activating_the_undo_toast_button() {
        check_native_repetition(false, false, true);
    }

    fn undo_button(widget: &gtk::Widget) -> Option<gtk::Button> {
        if let Some(button) = widget.downcast_ref::<gtk::Button>()
            && button.label().as_deref() == Some("Undo")
            && button.is_mapped()
        {
            return Some(button.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(button) = undo_button(&widget) {
                return Some(button);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn check_native_repetition(threads: bool, mixed: bool, toast: bool) {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            if threads {
                "org.example.BrevladaRepeatedThreadKeysDiagnostic"
            } else {
                "org.example.BrevladaRepeatedSenderKeysDiagnostic"
            },
            worker,
        );
        state.connect_list_actions();
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.set_title(Some("Brevlada repeated keys diagnostic"));
        let messages: Vec<_> = (1..=6)
            .rev()
            .map(|uid| Message {
                uid,
                uid_validity: Some(7),
                message_id: uid.to_string(),
                timestamp: i64::from(uid),
                sender: if threads && !mixed {
                    "sender@example.com".into()
                } else {
                    format!("sender{uid}@example.com")
                },
                subject: format!("Conversation {uid}"),
                body_loaded: false,
                is_read: true,
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages.clone();
        state.render_list();
        if threads {
            state.filter_sender(Some(messages[0].clone()));
            let ticket = state.sender_pane.borrow().ticket();
            state.event(Event::SenderPage(
                state.generation.get(),
                ticket,
                messages
                    .iter()
                    .filter(|message| message.sender == messages[0].sender)
                    .cloned()
                    .collect(),
                false,
            ));
            state.show_thread(vec![messages[0].clone()]);
        }
        window.present();
        settle();
        let windows = run(&["windows"]);
        let id = windows
            .lines()
            .find(|line| line.contains("Brevlada repeated keys diagnostic"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        run(&["input", "--", "windowactivate", "--sync", id]);
        let list = if threads {
            &state.thread_list
        } else {
            &state.list
        };
        ui::virtual_list::select(list, 0);
        ui::virtual_list::focus_selected(list);
        settle();
        while commands.try_recv().is_ok() {}
        for index in 0..4 {
            run(&[
                "input",
                "--",
                "key",
                if index % 2 == 0 {
                    "BackSpace"
                } else {
                    "Delete"
                },
            ]);
            settle();
            let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
                .filter_map(|command| match command {
                    Command::SenderAction { sender, .. } => Some(sender),
                    _ => None,
                })
                .collect();
            assert_eq!(
                sent.len(),
                usize::from(!(toast && index == 1)),
                "threads={threads} press={index} focus={:?} rows={:?} opened={:?}",
                gtk::prelude::RootExt::focus(&window).map(|widget| widget.type_().name()),
                ui::virtual_list::selected_positions(list),
                state.selected.borrow()
            );
            assert_eq!(
                state.visible_messages().len(),
                5 - index + usize::from(toast && index > 0)
            );
            if toast && index == 0 {
                for _ in 0..3 {
                    settle();
                }
                assert!(undo_button(window.upcast_ref()).unwrap().grab_focus());
                run(&["input", "--", "key", "Return"]);
                settle();
                assert_eq!(state.visible_messages().len(), 6);
            }
        }
        window.close();
        settle();
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_archive_undo_delete_cycles_remain_immediate_with_delayed_server_replies() {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaRepeatedUndoKeysDiagnostic",
            worker,
        );
        state.connect_list_actions();
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.set_title(Some("Brevlada repeated undo keys diagnostic"));
        let account = state.account.borrow().clone().unwrap();
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            sender: "sender@example.com".into(),
            subject: "Message".into(),
            body_loaded: false,
            is_read: true,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![message.clone()];
        state.render_list();
        window.present();
        settle();
        let windows = run(&["windows"]);
        let id = windows
            .lines()
            .find(|line| line.contains("Brevlada repeated undo keys diagnostic"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        run(&["input", "--", "windowactivate", "--sync", id]);
        ui::virtual_list::select(&state.list, 0);
        ui::virtual_list::focus_selected(&state.list);
        settle();
        while commands.try_recv().is_ok() {}
        let mut first_target = None;
        for index in 0..4 {
            run(&[
                "input",
                "--",
                "key",
                if index % 2 == 0 {
                    "BackSpace"
                } else {
                    "Delete"
                },
            ]);
            settle();
            assert!(
                state.visible_messages().is_empty(),
                "press {index} did not remove restored message"
            );
            let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
                .filter_map(|command| match command {
                    Command::SenderAction { sender, .. } => Some(sender),
                    _ => None,
                })
                .collect();
            if index == 0 {
                assert_eq!(sent.len(), 1);
                first_target = Some(sent[0].clone());
            } else {
                assert!(sent.is_empty());
                assert_eq!(state.deferred_actions.borrow().len(), 1);
            }
            run(&["input", "--", "key", "ctrl+z"]);
            settle();
            assert_eq!(
                state.visible_messages().len(),
                1,
                "undo {index} did not restore immediately"
            );
            assert!(state.deferred_actions.borrow().is_empty());
        }
        run(&["input", "--", "key", "Delete"]);
        settle();
        assert!(state.visible_messages().is_empty());
        assert_eq!(state.deferred_actions.borrow().len(), 1);
        let target = first_target.unwrap();
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(Vec::new()),
            removed: vec![message.clone()],
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            destination: Some("Archive".into()),
            removed: vec![message.clone()],
        });
        let mut undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        let restored = Message {
            uid: 101,
            ..message.clone()
        };
        state.event(Event::MessagesRestored {
            account: account.email.clone(),
            folder: "INBOX".into(),
            originals: vec![message.clone()],
            restored: vec![message],
            messages: Some(vec![restored.clone()]),
            error: None,
        });
        undo.batches.clear();
        state.event(Event::UndoActionFinished(undo));
        assert!(state.deferred_actions.borrow().is_empty());
        assert!(state.visible_messages().is_empty());
        let target = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::SenderAction {
                    sender,
                    action: SenderAction::Delete,
                    ..
                } => Some(sender),
                _ => None,
            })
            .unwrap();
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(Vec::new()),
            removed: vec![restored.clone()],
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Trash".into()),
            removed: vec![restored],
        });
        run(&["input", "--", "key", "ctrl+z"]);
        settle();
        assert_eq!(state.visible_messages().len(), 1);
        let undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(undo.batches[0].messages[0].uid, 101);
        window.close();
        settle();
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn repeated_message_actions_wait_for_restore_and_use_the_new_uid() {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaQueuedMessageActionsDiagnostic",
            worker,
        );
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        let account = state.account.borrow().clone().unwrap();
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            sender: "sender@example.com".into(),
            subject: "Message".into(),
            body_loaded: true,
            is_read: true,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![message.clone()];
        state.render_list();
        let target = ActionTarget::Messages(vec![(1, "one".into())]);
        state.sender_action(target.clone(), SenderAction::Archive);
        assert!(state.undo_action(None));
        state.sender_action(target.clone(), SenderAction::Delete);
        assert!(state.visible_messages().is_empty());
        assert_eq!(state.deferred_actions.borrow().len(), 1);
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(Vec::new()),
            removed: vec![message.clone()],
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Archive".into()),
            removed: vec![message.clone()],
        });
        while commands.try_recv().is_ok() {}
        let restored = Message {
            uid: 44,
            ..message.clone()
        };
        state.event(Event::MessagesRestored {
            account: account.email.clone(),
            folder: "INBOX".into(),
            originals: vec![message.clone()],
            restored: vec![message],
            messages: Some(vec![restored.clone()]),
            error: None,
        });
        assert!(state.visible_messages().is_empty());
        let target = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::SenderAction {
                    sender,
                    action: SenderAction::Delete,
                    ..
                } => Some(sender),
                _ => None,
            })
            .unwrap();
        assert_eq!(target, ActionTarget::Messages(vec![(44, "one".into())]));
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(Vec::new()),
            removed: vec![restored.clone()],
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account,
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Trash".into()),
            removed: vec![restored],
        });
        assert!(state.undo_action(None));
        assert_eq!(state.visible_messages()[0].uid, 44);
        let undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(undo.batches[0].messages[0].uid, 44);
        window.close();
    }
}
