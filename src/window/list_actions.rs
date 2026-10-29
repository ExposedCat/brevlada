use super::*;
use crate::backend::search::Location;
use models::{action_target::ActionTarget, sender_action::SenderAction};

#[derive(Clone)]
struct Target {
    action: ActionTarget,
    location: Location,
    messages: Vec<Message>,
}

impl State {
    pub(super) fn connect_list_actions(self: &Rc<Self>) {
        for threads in [false, true] {
            let lookup = Rc::downgrade(self);
            let activate = Rc::downgrade(self);
            let trust_lookup = Rc::downgrade(self);
            let trust_toggle = Rc::downgrade(self);
            let menu_lookup = Rc::downgrade(self);
            ui::sender_menu::attach_view(
                if threads {
                    &self.thread_list
                } else {
                    &self.list
                },
                if threads {
                    &self.thread_scroll
                } else {
                    &self.list_scroll
                },
                move |index| {
                    let state = lookup.upgrade()?;
                    let targets = state.list_targets(threads, index.try_into().ok()?);
                    let bulk = targets
                        .iter()
                        .any(|target| matches!(target.action, ActionTarget::Sender(_)));
                    (!targets.is_empty()).then_some((targets, bulk))
                },
                move |targets, action| {
                    if let Some(state) = activate.upgrade() {
                        state.request_list_action(targets, action);
                    }
                },
                move |targets| {
                    trust_lookup.upgrade().is_some_and(|state| {
                        targets.iter().all(|target| match &target.action {
                            ActionTarget::Sender(sender) => state
                                .trusted_senders
                                .borrow()
                                .get(&target.location.account)
                                .is_some_and(|senders| senders.contains(sender)),
                            _ => false,
                        })
                    })
                },
                move |targets| {
                    if let Some(state) = trust_toggle.upgrade() {
                        let trust = !targets.iter().all(|target| match &target.action {
                            ActionTarget::Sender(sender) => state
                                .trusted_senders
                                .borrow()
                                .get(&target.location.account)
                                .is_some_and(|senders| senders.contains(sender)),
                            _ => false,
                        });
                        for target in targets {
                            if let ActionTarget::Sender(sender) = target.action
                                && state.select_search_location(Some(&target.location))
                                && state.is_sender_trusted(&sender) != trust
                            {
                                state.toggle_trust(&sender);
                            }
                        }
                    }
                },
                move |targets| {
                    menu_lookup.upgrade().map_or_else(Vec::new, |state| {
                        let mut actions = Vec::new();
                        for target in targets {
                            let current =
                                state.account.borrow().as_ref().is_some_and(|account| {
                                    account.email == target.location.account
                                }) && *state.folder.borrow() == target.location.folder;
                            let available = if current {
                                state.menu_actions(&target.action)
                            } else {
                                sender_actions::menu_actions_for_messages(
                                    &target.location.folder,
                                    &target.messages,
                                )
                            };
                            for action in available {
                                if !actions.contains(&action) {
                                    actions.push(action);
                                }
                            }
                        }
                        actions
                    })
                },
            );
        }
    }

    fn list_targets(&self, threads: bool, position: u32) -> Vec<Target> {
        let list = if threads {
            &self.thread_list
        } else {
            &self.list
        };
        let selection = ui::virtual_list::selection(list);
        let positions = if selection.is_selected(position) {
            ui::virtual_list::selected_positions(list)
        } else {
            vec![position]
        };
        positions
            .into_iter()
            .filter_map(|position| {
                let row = ui::virtual_list::item(list, position)?;
                let first = row.messages.first()?;
                let action = if !threads && first.list_recipient.is_none() {
                    ActionTarget::Sender(models::senders::key(first))
                } else {
                    ActionTarget::Messages(
                        row.messages
                            .iter()
                            .map(|message| (message.uid, message.message_id.clone()))
                            .collect(),
                    )
                };
                let location = if threads {
                    None
                } else {
                    self.search_locations.borrow().get(&row.key).cloned()
                }
                .or_else(|| {
                    self.account.borrow().as_ref().map(|account| Location {
                        account: account.email.clone(),
                        folder: self.folder.borrow().clone(),
                    })
                })?;
                Some(Target {
                    action,
                    location,
                    messages: row.messages,
                })
            })
            .collect()
    }

    pub(super) fn selected_list_action(self: &Rc<Self>, action: SenderAction) -> bool {
        for threads in [true, false] {
            let list = if threads {
                &self.thread_list
            } else {
                &self.list
            };
            let selected = ui::virtual_list::selected_positions(list);
            if selected.len() > 1 {
                self.request_list_action(self.list_targets(threads, selected[0]), action);
                return true;
            }
        }
        false
    }

    fn request_list_action(
        self: &Rc<Self>,
        targets: Vec<Target>,
        action: SenderAction,
    ) -> Option<adw::MessageDialog> {
        if targets.len() < 2 {
            self.apply_list_action(&targets, action);
            return None;
        }
        let parent = self.toast.root().and_downcast::<gtk::Window>();
        if matches!(&action, SenderAction::MoveTo(folder) if folder.is_empty()) {
            let mut common: Option<Vec<String>> = None;
            for target in &targets {
                let folders: Vec<_> = self
                    .folder_names
                    .borrow()
                    .get(&target.location.account)
                    .into_iter()
                    .flatten()
                    .filter(|folder| !folder.eq_ignore_ascii_case(&target.location.folder))
                    .cloned()
                    .collect();
                if let Some(common) = &mut common {
                    common.retain(|folder| folders.contains(folder));
                } else {
                    common = Some(folders);
                }
            }
            let mut folders = common.unwrap_or_default();
            folders.sort_by_key(|folder| folder.to_lowercase());
            folders.dedup();
            if folders.is_empty() {
                self.toast.add_toast(adw::Toast::new(
                    "No shared destination folders available. Refresh the accounts and retry.",
                ));
                return None;
            }
            let generation = self.generation.get();
            let weak = Rc::downgrade(self);
            return Some(ui::action_dialog::move_to(
                parent.as_ref(),
                folders,
                move |folder| {
                    if let Some(state) = weak.upgrade()
                        && state.list_action_current(generation)
                    {
                        state.apply_list_action(&targets, SenderAction::MoveTo(folder.into()));
                    }
                },
            ));
        }
        let verb = match action {
            SenderAction::Delete => "Delete",
            SenderAction::Archive => "Archive",
            _ => {
                self.apply_list_action(&targets, action);
                return None;
            }
        };
        let mut messages = HashSet::new();
        let mut senders = HashSet::new();
        for target in &targets {
            for message in &target.messages {
                messages.insert((
                    target.location.clone(),
                    message.uid,
                    message.message_id.clone(),
                ));
                senders.insert(models::senders::key(message));
            }
        }
        let question = format!(
            "{verb} {} messages from {} {}?",
            messages.len(),
            senders.len(),
            if senders.len() == 1 {
                "sender"
            } else {
                "senders"
            }
        );
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        Some(ui::action_dialog::confirm(
            parent.as_ref(),
            &question,
            verb,
            action == SenderAction::Delete,
            move || {
                if let Some(state) = weak.upgrade()
                    && state.list_action_current(generation)
                {
                    state.apply_list_action(&targets, action.clone());
                }
            },
        ))
    }

    fn list_action_current(&self, generation: u64) -> bool {
        if self.generation.get() == generation {
            return true;
        }
        self.toast.add_toast(adw::Toast::new(
            "The active mailbox changed. Choose the messages again.",
        ));
        false
    }

    fn apply_list_action(self: &Rc<Self>, targets: &[Target], action: SenderAction) {
        let mut batches: Vec<Target> = Vec::new();
        for target in targets {
            if let ActionTarget::Messages(messages) = &target.action
                && let Some(batch) = batches.iter_mut().find(|batch| {
                    batch.location == target.location
                        && matches!(batch.action, ActionTarget::Messages(_))
                })
                && let ActionTarget::Messages(batch_messages) = &mut batch.action
            {
                for message in messages {
                    if !batch_messages.contains(message) {
                        batch_messages.push(message.clone());
                    }
                }
                for message in &target.messages {
                    if !batch.messages.iter().any(|existing| {
                        existing.uid == message.uid && existing.message_id == message.message_id
                    }) {
                        batch.messages.push(message.clone());
                    }
                }
            } else {
                batches.push(target.clone());
            }
        }
        let undo = action
            .undoable_move()
            .then(|| self.undo_history.borrow_mut().begin(action.clone()));
        self.undo_group.set(undo);
        for target in batches {
            if self.select_search_location(Some(&target.location)) {
                self.sender_action_with_navigation(
                    target.action,
                    action.clone(),
                    targets.len() == 1,
                    target.messages,
                );
            }
        }
        self.undo_group.set(None);
        if let Some(id) = undo
            && self.undo_history.borrow_mut().discard_empty(id)
        {
            self.show_undo_toast(id, &action);
        }
        if targets.len() > 1 {
            self.advance_message();
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a private graphical session and fixture XDG data directories"]
    fn undo_captures_search_sender_rows_before_the_mailbox_cache_has_loaded() {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaSearchSenderUndoDiagnostic",
            worker,
        );
        state.connect_list_actions();
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        let account = state.account.borrow().clone().unwrap();
        let messages: Vec<_> = (1..=4)
            .map(|uid| Message {
                uid,
                uid_validity: Some(7),
                message_id: uid.to_string(),
                sender: "GNOME <noreply@gnome.org>".into(),
                subject: format!("GNOME {uid}"),
                is_read: true,
                ..Default::default()
            })
            .collect();
        super::super::search::search_bar(&state.search).set_search_mode(true);
        state.search.set_text("GNOME");
        ui::virtual_list::update_owned(
            &state.list,
            vec![ui::virtual_list::Row {
                key: "lookup-gnome".into(),
                messages: messages.clone(),
            }],
            &[],
            Some(0),
        );
        state.search_locations.borrow_mut().insert(
            "lookup-gnome".into(),
            Location {
                account: account.email.clone(),
                folder: "INBOX".into(),
            },
        );
        assert!(state.messages.borrow().is_empty());
        let targets = state.list_targets(false, 0);
        assert_eq!(targets[0].messages.len(), 4);
        state.request_list_action(targets, SenderAction::Archive);
        assert!(state.visible_messages().is_empty());
        assert!(state.undo_action(None));
        assert_eq!(state.visible_messages().len(), 4);
        assert_eq!(
            ui::virtual_list::selected_keys(&state.list),
            vec!["noreply@gnome.org"]
        );
        assert!(ui::virtual_list::selected_positions(&state.thread_list).is_empty());
        let sender = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::SenderAction {
                    sender,
                    action: SenderAction::Archive,
                    ..
                } => Some(sender),
                _ => None,
            })
            .unwrap();
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: sender.clone(),
            messages: Some(Vec::new()),
            removed: messages.clone(),
            error: None,
        });
        state.event(Event::ActionUndoReady {
            account,
            folder: "INBOX".into(),
            sender,
            destination: Some("Archive".into()),
            removed: messages,
        });
        let undo = std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                Command::UndoAction(action) => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(undo.batches.len(), 1);
        assert_eq!(undo.batches[0].messages.len(), 4);
        window.close();
    }

    #[test]
    #[ignore = "Requires BREVLADA_GUI_HELPER and BREVLADA_GUI_SESSION for native input on a private display"]
    fn native_modifiers_select_senders_and_chats_in_the_styled_window() {
        let helper = std::env::var("BREVLADA_GUI_HELPER").unwrap();
        let session = std::env::var("BREVLADA_GUI_SESSION").unwrap();
        let run = |arguments: &[&str]| {
            let result = std::process::Command::new("/usr/bin/python3")
                .arg(&helper)
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
        };
        let settle = || {
            let until = std::time::Instant::now() + Duration::from_millis(150);
            while std::time::Instant::now() < until {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaNativeMultiSelectionDiagnostic",
            worker,
        );
        state.connect_list_actions();
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.set_title(Some("Brevlada multi-selection diagnostic"));
        let messages: Vec<_> = (1..=7)
            .map(|uid| Message {
                uid,
                message_id: uid.to_string(),
                subject: format!("Conversation {uid}"),
                sender: match uid {
                    1..=4 => "a@example.com",
                    5 => "b@example.com",
                    6 => "c@example.com",
                    _ => "d@example.com",
                }
                .into(),
                timestamp: 100 - i64::from(uid),
                is_read: true,
                body_loaded: false,
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
            messages[..4].to_vec(),
            false,
        ));
        window.present();
        settle();
        let windows = run(&["windows"]);
        let id = windows
            .lines()
            .find(|line| line.contains("Brevlada multi-selection diagnostic"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        run(&["input", "--", "windowactivate", "--sync", id]);
        let click = |list: &gtk::ListView, position, modifier: Option<&str>| {
            let row = ui::virtual_list::visible_row(list, position).unwrap();
            let point = row
                .compute_point(
                    &window,
                    &gtk::graphene::Point::new((row.width() / 2) as f32, (row.height() / 2) as f32),
                )
                .unwrap();
            let x = (point.x() as i32).to_string();
            let y = (point.y() as i32).to_string();
            let mut arguments = vec!["input", "--"];
            if let Some(modifier) = modifier {
                arguments.extend(["keydown", modifier]);
            }
            arguments.extend(["mousemove", "--window", id, &x, &y, "click", "1"]);
            if let Some(modifier) = modifier {
                arguments.extend(["keyup", modifier]);
            }
            run(&arguments);
            settle();
        };
        for threads in [false, true] {
            let list = if threads {
                &state.thread_list
            } else {
                &state.list
            };
            click(list, 0, None);
            assert_eq!(ui::virtual_list::selected_positions(list), vec![0]);
            click(list, 2, Some("ctrl"));
            assert_eq!(ui::virtual_list::selected_positions(list), vec![0, 2]);
            click(list, 3, Some("shift"));
            assert_eq!(ui::virtual_list::selected_positions(list), vec![2, 3]);
            click(list, 2, Some("ctrl"));
            assert_eq!(ui::virtual_list::selected_positions(list), vec![3]);
            click(list, 0, None);
            click(list, 3, Some("shift"));
            assert_eq!(ui::virtual_list::selected_positions(list), vec![0, 1, 2, 3]);
            if let Ok(path) = std::env::var("BREVLADA_GUI_ARTIFACTS") {
                let path = format!(
                    "{path}/{}-selection.png",
                    if threads { "chat" } else { "sender" }
                );
                run(&["screenshot", &path]);
            }
        }
        while commands.try_recv().is_ok() {}
        let targets = state.list_targets(true, 0);
        let dialog = state
            .request_list_action(targets, SenderAction::Archive)
            .unwrap();
        settle();
        assert_eq!(
            dialog.heading().as_deref(),
            Some("Archive 4 messages from 1 sender?")
        );
        dialog.response("confirm");
        assert!(std::iter::from_fn(|| commands.try_recv().ok()).any(|command| matches!(command,
            Command::SenderAction { action: SenderAction::Archive, sender: ActionTarget::Messages(messages), .. } if messages.len() == 4)));
        window.close();
        settle();
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses a recording mail worker"]
    fn bulk_actions_confirm_counts_and_keep_the_selected_scope() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaMultiSelectionDiagnostic")
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
        state.connect_list_actions();
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
        let messages: Vec<_> = (1..=5)
            .map(|uid| Message {
                uid,
                message_id: uid.to_string(),
                subject: format!("Chat {uid}"),
                sender: match uid {
                    1..=3 => "a@example.com",
                    4 => "b@example.com",
                    _ => "c@example.com",
                }
                .into(),
                timestamp: 10 - i64::from(uid),
                is_read: true,
                body_loaded: true,
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages.clone();
        state.render_list();
        ui::virtual_list::selection(&state.list).select_item(0, true);
        ui::virtual_list::selection(&state.list).select_item(2, false);
        let targets = state.list_targets(false, 0);
        assert_eq!(targets.len(), 2);
        assert_eq!(state.list_targets(false, 1).len(), 1);
        let dialog = state
            .request_list_action(targets.clone(), SenderAction::Delete)
            .unwrap();
        assert_eq!(
            dialog.heading().as_deref(),
            Some("Delete 4 messages from 2 senders?")
        );
        assert!(commands.try_recv().is_err());
        dialog.response("cancel");
        assert!(commands.try_recv().is_err());
        let keys = state
            .list
            .observe_controllers()
            .iter::<glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|controller| controller.name().as_deref() == Some("mail-row-shortcuts"))
            .unwrap();
        assert!(keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::Delete,
                &0u32,
                &gtk::gdk::ModifierType::empty()
            ]
        ));
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<adw::MessageDialog>().ok())
            .find(|dialog| dialog.is_visible())
            .unwrap();
        assert_eq!(
            dialog.heading().as_deref(),
            Some("Delete 4 messages from 2 senders?")
        );
        assert!(commands.try_recv().is_err());
        dialog.response("cancel");
        state.request_list_action(targets.clone(), SenderAction::Star);
        for target in &targets {
            assert!(
                matches!(commands.try_recv().unwrap(), Command::SenderAction { sender, action: SenderAction::Star, .. } if sender == target.action)
            );
        }
        assert!(commands.try_recv().is_err());
        assert_eq!(
            ui::virtual_list::selected_keys(&state.list),
            vec!["a@example.com", "c@example.com"]
        );
        assert!(
            state
                .visible_messages()
                .iter()
                .filter(|message| message.uid != 4)
                .all(|message| message.is_flagged)
        );
        assert!(
            !state
                .visible_messages()
                .iter()
                .find(|message| message.uid == 4)
                .unwrap()
                .is_flagged
        );
        *state.sender_actions.borrow_mut() = Default::default();
        state
            .folder_names
            .borrow_mut()
            .insert("account".into(), vec!["INBOX".into(), "Work".into()]);
        let dialog = state
            .request_list_action(targets.clone(), SenderAction::MoveTo(String::new()))
            .unwrap();
        assert!(commands.try_recv().is_err());
        dialog.response("move");
        for target in &targets {
            assert!(
                matches!(commands.try_recv().unwrap(), Command::SenderAction { sender, action: SenderAction::MoveTo(folder), .. } if sender == target.action && folder == "Work")
            );
        }
        assert_eq!(
            state
                .visible_messages()
                .iter()
                .map(|message| message.uid)
                .collect::<Vec<_>>(),
            vec![4]
        );
        *state.sender_actions.borrow_mut() = Default::default();
        state.render_list();
        state.filter_sender(Some(messages[0].clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            messages[..3].to_vec(),
            false,
        ));
        state.show_thread(vec![messages[0].clone()]);
        ui::virtual_list::selection(&state.thread_list).select_range(0, 2, true);
        assert!(ui::virtual_list::selected_positions(&state.list).is_empty());
        let chats = state.list_targets(true, 0);
        assert_eq!(chats.len(), 2);
        let dialog = state
            .request_list_action(chats.clone(), SenderAction::Archive)
            .unwrap();
        assert_eq!(
            dialog.heading().as_deref(),
            Some("Archive 2 messages from 1 sender?")
        );
        while commands.try_recv().is_ok() {}
        dialog.response("confirm");
        let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok())
            .filter_map(|command| match command {
                Command::SenderAction {
                    sender,
                    action: SenderAction::Archive,
                    ..
                } => Some(sender),
                _ => None,
            })
            .collect();
        assert_eq!(
            sent,
            vec![ActionTarget::Messages(vec![
                (1, "1".into()),
                (2, "2".into())
            ])]
        );
        assert_eq!(
            state
                .visible_messages()
                .iter()
                .map(|message| message.uid)
                .collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(*state.selected.borrow(), vec![3]);
        *state.sender_actions.borrow_mut() = Default::default();
        state.render_list();
        ui::virtual_list::selection(&state.list).select_range(0, 2, true);
        let dialog = state
            .request_list_action(state.list_targets(false, 0), SenderAction::Delete)
            .unwrap();
        state.generation.set(state.generation.get() + 1);
        while commands.try_recv().is_ok() {}
        dialog.response("confirm");
        assert!(commands.try_recv().is_err());
        window.close();
    }
}
