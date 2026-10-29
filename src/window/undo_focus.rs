use super::*;
use models::{action_target::ActionTarget, undo::Snapshot};

pub(super) struct Selection {
    senders: Vec<String>,
    messages: Vec<(u32, String)>,
    opened: Option<(u32, String)>,
    focus_threads: bool,
    focus_senders: bool,
}

fn focused(list: &gtk::ListView) -> bool {
    list.root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus == *list.upcast_ref::<gtk::Widget>() || focus.is_ancestor(list))
}

fn select_only(list: &gtk::ListView, positions: &[u32]) {
    let selected = gtk::Bitset::new_empty();
    for position in positions {
        selected.add(*position);
    }
    ui::virtual_list::selection(list).set_selection(
        &selected,
        &gtk::Bitset::new_range(0, ui::virtual_list::model(list).n_items()),
    );
}

impl Selection {
    pub(super) fn rebind(&mut self, original: &Message, replacement: &Message) {
        if let Some((uid, id)) = &mut self.opened
            && *uid == original.uid
            && *id == original.message_id
        {
            *uid = replacement.uid;
        }
        for (uid, id) in &mut self.messages {
            if *uid == original.uid && *id == original.message_id {
                *uid = replacement.uid;
            }
        }
    }

    pub(super) fn restore(&self, state: &State) {
        let positions: Vec<_> = ui::virtual_list::groups(&state.thread_list)
            .iter()
            .enumerate()
            .filter(|(_, group)| {
                group.iter().any(|message| {
                    self.messages
                        .contains(&(message.uid, message.message_id.clone()))
                })
            })
            .map(|(index, _)| index as u32)
            .collect();
        let rendering = state.rendering.replace(true);
        ui::virtual_list::restore_selection(&state.list, &self.senders);
        select_only(&state.thread_list, &positions);
        state.rendering.set(rendering);
        if self.focus_threads {
            let anchor = self.opened.as_ref().and_then(|opened| {
                ui::virtual_list::groups(&state.thread_list)
                    .iter()
                    .position(|group| {
                        group.iter().any(|message| {
                            message.uid == opened.0 && message.message_id == opened.1
                        })
                    })
            });
            if let Some(anchor) = anchor {
                ui::virtual_list::focus_position(&state.thread_list, anchor as u32);
            } else {
                ui::virtual_list::focus_selected(&state.thread_list);
            }
        } else if self.focus_senders {
            ui::virtual_list::focus_selected(&state.list);
        }
    }
}

impl State {
    pub(super) fn capture_undo_selection(&self, originals: &[Message]) -> Selection {
        let senders = ui::virtual_list::selected_keys(&self.list);
        let mut messages: Vec<_> = ui::virtual_list::selected_positions(&self.thread_list)
            .into_iter()
            .filter_map(|position| ui::virtual_list::item(&self.thread_list, position))
            .flat_map(|row| {
                row.messages
                    .into_iter()
                    .map(|message| (message.uid, message.message_id))
            })
            .collect();
        if senders.is_empty() && messages.is_empty() {
            messages.extend(
                self.open_group
                    .borrow()
                    .iter()
                    .filter(|message| {
                        originals.iter().any(|original| {
                            original.uid == message.uid && original.message_id == message.message_id
                        })
                    })
                    .map(|message| (message.uid, message.message_id.clone())),
            );
        }
        Selection {
            opened: self
                .open_group
                .borrow()
                .first()
                .map(|message| (message.uid, message.message_id.clone())),
            senders,
            messages,
            focus_threads: focused(&self.thread_list),
            focus_senders: focused(&self.list),
        }
    }

    pub(super) fn focus_undo_restoration(self: &Rc<Self>, snapshots: &[Snapshot]) {
        let Some(first) = snapshots
            .iter()
            .find(|snapshot| !snapshot.messages.is_empty())
        else {
            self.refresh_sender_view();
            return;
        };
        let account = self
            .account
            .borrow()
            .as_ref()
            .filter(|account| account.email == first.account)
            .cloned()
            .or_else(|| {
                self.folder_boxes
                    .borrow()
                    .get(&first.account)
                    .map(|(account, _)| account.clone())
            });
        let Some(account) = account else {
            self.refresh_sender_view();
            return;
        };
        if self
            .account
            .borrow()
            .as_ref()
            .is_none_or(|current| current.email != account.email)
            || *self.folder.borrow() != first.folder
        {
            self.select_mailbox(account, first.folder.clone(), false);
        }
        let local: Vec<_> = snapshots
            .iter()
            .filter(|snapshot| snapshot.account == first.account && snapshot.folder == first.folder)
            .collect();
        let restored: Vec<_> = local
            .iter()
            .flat_map(|snapshot| snapshot.messages.iter().cloned())
            .collect();
        let identities: HashSet<_> = restored
            .iter()
            .map(|message| (message.uid, message.message_id.clone()))
            .collect();
        let sender_keys: HashSet<_> = restored.iter().map(models::senders::list_key).collect();
        let threads = local
            .iter()
            .all(|snapshot| matches!(snapshot.target, ActionTarget::Messages(_)))
            && sender_keys.len() == 1;
        self.next_message.set(None);
        if self.sender_search_active() {
            self.cancel_search(false);
            super::search::search_bar(&self.search).set_search_mode(false);
        }
        ui::virtual_list::selection(&self.list).unselect_all();
        ui::virtual_list::selection(&self.thread_list).unselect_all();
        self.filter_sender(Some(restored[0].clone()));
        self.thread_search.set_text("");
        self.sender_pane.borrow_mut().include(&restored);
        self.refresh_sender_view();
        let groups = ui::virtual_list::groups(&self.thread_list);
        let positions: Vec<_> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| {
                group
                    .iter()
                    .any(|message| identities.contains(&(message.uid, message.message_id.clone())))
            })
            .map(|(index, _)| index as u32)
            .collect();
        let opened: HashSet<_> = local
            .iter()
            .flat_map(|snapshot| snapshot.opened.iter().cloned())
            .collect();
        let anchor = groups
            .iter()
            .position(|group| {
                group
                    .iter()
                    .any(|message| opened.contains(&(message.uid, message.message_id.clone())))
            })
            .or_else(|| {
                threads
                    .then(|| positions.first().map(|position| *position as usize))
                    .flatten()
            });
        if let Some(position) = anchor {
            self.show_thread(
                groups[position]
                    .iter()
                    .filter(|message| {
                        identities.contains(&(message.uid, message.message_id.clone()))
                    })
                    .cloned()
                    .collect(),
            );
        } else {
            self.new_selection();
            self.selected.borrow_mut().clear();
            self.deferred_read_sort.borrow_mut().clear();
            self.open_group.borrow_mut().clear();
            self.related_sent.borrow_mut().clear();
            self.cards.borrow_mut().clear();
            self.sent_cards.borrow_mut().clear();
            ui::states::select_message(&self.viewer);
            self.refresh_open_styles();
        }
        let rendering = self.rendering.replace(true);
        if threads {
            select_only(&self.list, &[]);
            select_only(&self.thread_list, &positions);
        } else {
            let positions: Vec<_> = ui::virtual_list::groups(&self.list)
                .iter()
                .enumerate()
                .filter(|(_, group)| {
                    group.iter().any(|message| {
                        identities.contains(&(message.uid, message.message_id.clone()))
                    })
                })
                .map(|(index, _)| index as u32)
                .collect();
            select_only(&self.thread_list, &[]);
            select_only(&self.list, &positions);
        }
        self.rendering.set(rendering);
        if threads && let Some(position) = anchor {
            ui::virtual_list::focus_position(&self.thread_list, position as u32);
        } else {
            ui::virtual_list::focus_selected(&self.list);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::sender_action::SenderAction;

    fn message(uid: u32, sender: &str) -> Message {
        Message {
            uid,
            uid_validity: Some(7),
            message_id: uid.to_string(),
            timestamp: 100 - i64::from(uid),
            sender: sender.into(),
            subject: format!("Message {uid}"),
            is_read: true,
            ..Default::default()
        }
    }

    fn settle() {
        let until = std::time::Instant::now() + Duration::from_millis(120);
        while std::time::Instant::now() < until {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn selected_messages(list: &gtk::ListView) -> HashSet<u32> {
        ui::virtual_list::selected_positions(list)
            .into_iter()
            .filter_map(|position| ui::virtual_list::item(list, position))
            .flat_map(|row| row.messages.into_iter().map(|message| message.uid))
            .collect()
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn undo_sender_returns_to_gnome_only_and_survives_reopening_and_stale_cache() {
        check_sender(SenderAction::Archive, false, false);
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn undo_move_to_archive_returns_the_sender_and_selects_only_its_restored_mail() {
        check_sender(SenderAction::MoveTo("Archive".into()), false, false);
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_ctrl_z_restores_four_gnome_messages_and_focuses_only_the_sender() {
        check_sender(SenderAction::Archive, true, false);
    }

    #[test]
    #[ignore = "Requires a private graphical session and fixture XDG data directories"]
    fn undo_sender_from_search_returns_to_restored_mail_without_waiting_for_search_results() {
        check_sender(SenderAction::Archive, false, true);
    }

    fn check_sender(action: SenderAction, native: bool, search: bool) {
        let (worker, _commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaUndoSenderFocusDiagnostic",
            worker,
        );
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        let account = state.account.borrow().clone().unwrap();
        state.connect_list_actions();
        let gnome: Vec<_> = (1..=4)
            .map(|uid| message(uid, "GNOME <gnome@example.com>"))
            .collect();
        let other = message(5, "Other <other@example.com>");
        *state.messages.borrow_mut() = [gnome.clone(), vec![other.clone()]].concat();
        state.render_list();
        state.filter_sender(Some(gnome[0].clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            gnome.clone(),
            false,
        ));
        state.show_thread(vec![gnome[2].clone()]);
        ui::virtual_list::select(&state.list, 0);
        let target = ActionTarget::Sender("gnome@example.com".into());
        if search {
            super::super::search::search_bar(&state.search).set_search_mode(true);
            state.search.set_text("GNOME");
        }
        let run = |arguments: &[&str]| {
            let output = std::process::Command::new("/usr/bin/python3")
                .arg(std::env::var("BREVLADA_GUI_HELPER").unwrap())
                .args(["--session", &std::env::var("BREVLADA_GUI_SESSION").unwrap()])
                .args(arguments)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        let settle = || {
            let until = std::time::Instant::now() + Duration::from_millis(120);
            while std::time::Instant::now() < until {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        if native {
            window.set_title(Some("GNOME undo focus diagnostic"));
            window.present();
            settle();
            let windows = run(&["windows"]);
            let id = windows
                .lines()
                .find(|line| line.contains("GNOME undo focus diagnostic"))
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap();
            run(&["input", "--", "windowactivate", "--sync", id]);
            ui::virtual_list::focus_selected(&state.list);
            settle();
            run(&["input", "--", "key", "BackSpace"]);
            settle();
        } else {
            state.sender_action(target.clone(), action);
        }
        if !search {
            assert_eq!(*state.selected.borrow(), vec![other.uid]);
            assert_eq!(
                state.sender_pane.borrow().sender(),
                Some("other@example.com")
            );
        }
        let other_ticket = state.sender_pane.borrow().ticket();
        if native {
            run(&["input", "--", "key", "ctrl+z"]);
            settle();
            assert!(focused(&state.list));
        } else {
            assert!(state.undo_action(None));
        }
        assert!(!state.sender_search_active());
        assert_eq!(
            state.sender_pane.borrow().sender(),
            Some("gnome@example.com")
        );
        assert_eq!(
            ui::virtual_list::selected_keys(&state.list),
            vec!["gnome@example.com"]
        );
        assert!(ui::virtual_list::selected_positions(&state.thread_list).is_empty());
        assert_eq!(*state.selected.borrow(), vec![3]);
        assert!(
            state
                .open_group
                .borrow()
                .iter()
                .all(|message| models::senders::key(message) == "gnome@example.com")
        );
        state.event(Event::SenderPage(
            state.generation.get(),
            other_ticket,
            vec![other.clone()],
            false,
        ));
        assert_eq!(
            state.sender_pane.borrow().sender(),
            Some("gnome@example.com")
        );
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(vec![other.clone()]),
            removed: gnome.clone(),
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Archive".into()),
            removed: gnome.clone(),
        });
        let restored: Vec<_> = gnome
            .iter()
            .map(|message| Message {
                uid: message.uid + 10,
                ..message.clone()
            })
            .collect();
        state.event(Event::MessagesRestored {
            account: account.email.clone(),
            folder: "INBOX".into(),
            originals: gnome.clone(),
            restored: gnome,
            messages: Some([restored.clone(), vec![other.clone()]].concat()),
            error: None,
        });
        assert_eq!(
            ui::virtual_list::selected_keys(&state.list),
            vec!["gnome@example.com"]
        );
        assert!(ui::virtual_list::selected_positions(&state.thread_list).is_empty());
        assert_eq!(*state.selected.borrow(), vec![13]);
        state.filter_sender(None);
        state.filter_sender(Some(restored[0].clone()));
        state.event(Event::CacheList(account.email, "INBOX".into(), vec![other]));
        assert_eq!(
            state
                .visible_messages()
                .iter()
                .filter(|message| models::senders::key(message) == "gnome@example.com")
                .count(),
            4
        );
        window.close();
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn undo_conversation_deselects_automatically_opened_next_conversation() {
        check_conversations(false, 0);
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn undo_bulk_selects_restored_conversations_only_after_uid_rebinding() {
        check_conversations(true, 0);
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn undo_bulk_returns_keyboard_focus_to_the_originally_opened_restored_conversation() {
        check_conversations(true, 2);
    }

    fn check_conversations(bulk: bool, opened: usize) {
        let (worker, _commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaUndoConversationFocusDiagnostic",
            worker,
        );
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        let account = state.account.borrow().clone().unwrap();
        let messages: Vec<_> = (1..=3)
            .map(|uid| message(uid, "gnome@example.com"))
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
        state.show_thread(vec![messages[opened].clone()]);
        ui::virtual_list::select(&state.thread_list, opened as u32);
        window.present();
        settle();
        ui::virtual_list::focus_selected(&state.thread_list);
        let removed = if bulk {
            vec![messages[0].clone(), messages[2].clone()]
        } else {
            vec![messages[0].clone()]
        };
        let target = ActionTarget::Messages(
            removed
                .iter()
                .map(|message| (message.uid, message.message_id.clone()))
                .collect(),
        );
        state.sender_action(target.clone(), SenderAction::Delete);
        assert_eq!(*state.selected.borrow(), vec![2]);
        assert!(state.undo_action(None));
        settle();
        let expected: HashSet<_> = removed.iter().map(|message| message.uid).collect();
        assert_eq!(selected_messages(&state.thread_list), expected);
        assert!(ui::virtual_list::selected_positions(&state.list).is_empty());
        assert_eq!(*state.selected.borrow(), vec![messages[opened].uid]);
        let remaining: Vec<_> = messages
            .iter()
            .filter(|message| !expected.contains(&message.uid))
            .cloned()
            .collect();
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(remaining.clone()),
            removed: removed.clone(),
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account: account.clone(),
            folder: "INBOX".into(),
            sender: target,
            destination: Some("Trash".into()),
            removed: removed.clone(),
        });
        assert_eq!(selected_messages(&state.thread_list), expected);
        let restored: Vec<_> = removed
            .iter()
            .map(|message| Message {
                uid: message.uid + 10,
                ..message.clone()
            })
            .collect();
        state.event(Event::MessagesRestored {
            account: account.email,
            folder: "INBOX".into(),
            originals: removed.clone(),
            restored: removed,
            messages: Some([restored.clone(), remaining].concat()),
            error: None,
        });
        settle();
        assert_eq!(
            selected_messages(&state.thread_list),
            restored.iter().map(|message| message.uid).collect()
        );
        assert!(ui::virtual_list::selected_positions(&state.list).is_empty());
        assert_eq!(*state.selected.borrow(), vec![messages[opened].uid + 10]);
        let row = ui::virtual_list::visible_row(&state.thread_list, opened as u32).unwrap();
        let focus = gtk::prelude::RootExt::focus(&window).unwrap();
        assert!(
            focus == row
                || focus.is_ancestor(&row)
                || (focus.parent().as_ref() == Some(state.thread_list.upcast_ref())
                    && row.is_ancestor(&focus)),
            "focus={} row={}",
            focus.type_().name(),
            row.widget_name()
        );
        window.close();
    }
}
