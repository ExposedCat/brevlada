use super::*;
use models::sender_action::SenderAction;

impl State {
    pub(super) fn sender_action(
        self: &Rc<Self>,
        sender: models::action_target::ActionTarget,
        action: SenderAction,
    ) {
        if matches!(&action, SenderAction::MoveTo(folder) if folder.is_empty()) {
            self.move_dialog(sender);
            return;
        }
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let folder = self.folder.borrow().clone();
        if action == SenderAction::Unspam {
            let originals =
                self.sender_actions
                    .borrow_mut()
                    .undo_spam(&account.email, &folder, &sender);
            if !originals.is_empty() {
                if let Err(error) = self.sender.send(Command::UndoSpam {
                    account: account.clone(),
                    folder: folder.clone(),
                    originals: originals.clone(),
                }) {
                    self.sender_actions.borrow_mut().finish_undo_spam(
                        &account.email,
                        &folder,
                        &originals,
                        &[],
                    );
                    self.toast.add_toast(adw::Toast::new(&error.to_string()));
                }
                self.refresh_sender_view();
                return;
            }
            if self
                .visible_messages()
                .iter()
                .any(|message| sender.matches(message) && self.retained_spam(message))
            {
                return;
            }
        }
        let spam_messages = if action == SenderAction::Spam {
            let mut messages = self.messages.borrow().clone();
            messages.extend_from_slice(self.sender_pane.borrow().messages());
            messages.extend_from_slice(&self.open_group.borrow());
            messages.retain(|message| sender.matches(message));
            messages
                .iter()
                .filter_map(|message| self.visible_message(message))
                .collect()
        } else {
            Vec::new()
        };
        if !self
            .sender_actions
            .borrow_mut()
            .begin(&account.email, &folder, &sender, action.clone())
        {
            return;
        }
        if action == SenderAction::Spam {
            self.sender_actions
                .borrow_mut()
                .retain_spam(&account.email, &folder, &spam_messages);
        }
        if let Err(error) = self.sender.send(Command::SenderAction {
            account: account.clone(),
            folder: folder.clone(),
            sender: sender.clone(),
            action: action.clone(),
        }) {
            self.sender_actions
                .borrow_mut()
                .finish(&account.email, &folder, &sender);
            if action == SenderAction::Spam {
                self.sender_actions
                    .borrow_mut()
                    .finish_spam(&account.email, &folder, &sender, &[]);
            }
            self.toast.add_toast(adw::Toast::new(&error.to_string()));
            return;
        }
        let affects_open_message = self
            .open_group
            .borrow()
            .iter()
            .any(|message| sender.matches(message));
        let replace_open_message =
            action.removes_messages() && action != SenderAction::Spam && affects_open_message;
        let close_unread = action == SenderAction::MarkUnread && affects_open_message;
        if replace_open_message {
            let sender_index = ui::virtual_list::groups(&self.list)
                .iter()
                .position(|group| {
                    group.first().is_some_and(|message| {
                        self.sender_pane.borrow().sender()
                            == Some(models::senders::key(message).as_str())
                    })
                })
                .unwrap_or(0);
            let thread_index = ui::virtual_list::groups(&self.thread_list)
                .iter()
                .position(|group| super::rendering::overlaps(group, &self.open_group.borrow()))
                .unwrap_or(0);
            self.next_message
                .set(Some((sender_index as u32, thread_index as u32)));
        }
        // Close explicitly unread mail so expanded cards do not immediately mark it read again.
        if replace_open_message || close_unread {
            self.new_selection();
            self.deferred_read_sort.borrow_mut().clear();
            self.selected.borrow_mut().clear();
            self.cards.borrow_mut().clear();
            self.sent_cards.borrow_mut().clear();
            self.related_sent.borrow_mut().clear();
            self.open_group.borrow_mut().clear();
            ui::states::select_message(&self.viewer);
        }
        self.refresh_sender_view();
        if replace_open_message {
            self.advance_message();
        }
    }

    pub(super) fn menu_actions(
        &self,
        target: &models::action_target::ActionTarget,
    ) -> Vec<SenderAction> {
        let messages = self.visible_messages();
        let pane = self.sender_pane.borrow();
        let targeted: Vec<_> = messages
            .iter()
            .chain(pane.messages())
            .filter_map(|message| self.visible_message(message))
            .filter(|message| target.matches(message))
            .collect();
        if !targeted.is_empty() && targeted.iter().all(|message| self.retained_spam(message)) {
            return vec![SenderAction::Unspam];
        }
        menu_actions_for_messages(&self.folder.borrow(), &targeted)
    }

    fn move_dialog(
        self: &Rc<Self>,
        target: models::action_target::ActionTarget,
    ) -> Option<adw::MessageDialog> {
        let account = self.account.borrow().as_ref()?.email.clone();
        let folder = self.folder.borrow().clone();
        let mut folders: Vec<_> = self
            .folder_names
            .borrow()
            .get(&account)
            .into_iter()
            .flatten()
            .filter(|destination| !destination.eq_ignore_ascii_case(&folder))
            .cloned()
            .collect();
        folders.sort_by_key(|folder| folder.to_lowercase());
        folders.dedup();
        if folders.is_empty() {
            self.toast.add_toast(adw::Toast::new(
                "No destination folders available. Refresh the account and retry.",
            ));
            return None;
        }
        let parent = self.toast.root().and_downcast::<gtk::Window>();
        let dialog = adw::MessageDialog::new(
            parent.as_ref(),
            Some("Move to…"),
            Some("Choose a destination folder."),
        );
        let names: Vec<_> = folders.iter().map(String::as_str).collect();
        let destination = gtk::DropDown::from_strings(&names);
        destination.set_enable_search(true);
        destination.update_property(&[gtk::accessible::Property::Label("Destination folder")]);
        dialog.set_extra_child(Some(&destination));
        dialog.add_responses(&[("cancel", "Cancel"), ("move", "Move")]);
        dialog.set_response_appearance("move", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("move"));
        dialog.set_close_response("cancel");
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if response != "move" {
                return;
            }
            let Some(state) = weak.upgrade() else {
                return;
            };
            if state
                .account
                .borrow()
                .as_ref()
                .map(|account| account.email.as_str())
                != Some(account.as_str())
                || *state.folder.borrow() != folder
            {
                state.toast.add_toast(adw::Toast::new(
                    "The active mailbox changed. Choose the messages again.",
                ));
                return;
            }
            if let Some(folder) = folders.get(destination.selected() as usize) {
                state.sender_action(target.clone(), SenderAction::MoveTo(folder.clone()));
            }
        });
        dialog.present();
        Some(dialog)
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
        self.visible_group(&self.messages.borrow())
    }

    pub(super) fn visible_group(&self, messages: &[Message]) -> Vec<Message> {
        let account = self.account.borrow();
        account.as_ref().map_or_else(
            || messages.to_vec(),
            |account| {
                self.sender_actions.borrow().visible(
                    &account.email,
                    &self.folder.borrow(),
                    messages,
                )
            },
        )
    }

    pub(super) fn visible_list_group(&self, messages: &[Message], threads: bool) -> Vec<Message> {
        let headers: Vec<_> = messages
            .iter()
            .map(|message| message.list_header(threads))
            .collect();
        self.visible_group(&headers)
            .into_iter()
            .map(|message| message.list_header(threads))
            .collect()
    }

    pub(super) fn search_documents(&self, messages: &[Message]) -> Vec<Message> {
        let documents: Vec<_> = messages.iter().map(Message::search_document).collect();
        self.visible_group(&documents)
    }

    pub(super) fn retained_spam(&self, message: &Message) -> bool {
        self.account.borrow().as_ref().is_some_and(|account| {
            self.sender_actions.borrow().is_retained_spam(
                &account.email,
                &self.folder.borrow(),
                message,
            )
        })
    }

    pub(super) fn refresh_sender_view(&self) {
        let mut messages = self.messages.borrow().clone();
        messages.extend_from_slice(self.sender_pane.borrow().messages());
        for message in self.visible_group(&messages) {
            if let Some(card) = self.cards.borrow().get(&message.uid) {
                card.update(&message);
            }
        }
        self.render_list();
    }
}

pub(super) fn menu_actions_for_messages(folder: &str, targeted: &[Message]) -> Vec<SenderAction> {
    let mut actions = vec![SenderAction::MarkRead, SenderAction::MarkUnread];
    if targeted.iter().any(|message| !message.is_flagged) {
        actions.push(SenderAction::Star);
    }
    if targeted.iter().any(|message| message.is_flagged) {
        actions.push(SenderAction::Unstar);
    }
    actions.push(SenderAction::MoveTo(String::new()));
    let folder = folder.to_lowercase();
    let leaf = folder.rsplit(['/', '.']).next().unwrap_or(&folder);
    let trash = matches!(leaf, "trash" | "bin" | "deleted items" | "deleted messages");
    let spam = matches!(leaf, "spam" | "junk" | "junk mail" | "junk e-mail");
    if trash {
        actions.push(SenderAction::Restore);
    }
    if trash || spam || targeted.iter().any(|message| message.is_spam) {
        actions.push(SenderAction::Unspam);
    } else {
        actions.push(SenderAction::Spam);
    }
    actions.extend([SenderAction::Archive, SenderAction::Delete]);
    actions
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session to inspect sender scrolling"]
    fn reopening_sender_after_spam_keeps_sender_scroll_position() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSpamScrollDiagnostic")
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
        // Exercise both removing a sender and moving it below the viewport.
        for has_older in [false, true] {
            state.filter_sender(None);
            *state.sender_actions.borrow_mut() = Default::default();
            let messages: Vec<_> = (1..=100)
                .map(|uid| Message {
                    uid,
                    message_id: format!("message-{uid}"),
                    sender: format!("sender{uid}@example.com"),
                    subject: format!("Message {uid}"),
                    timestamp: i64::from(uid),
                    is_read: true,
                    ..Default::default()
                })
                .collect();
            let marked = messages[57].clone();
            let other = messages[56].clone();
            let older = Message {
                uid: 101,
                timestamp: 0,
                message_id: "older".into(),
                ..marked.clone()
            };
            *state.messages.borrow_mut() = messages;
            if has_older {
                state.messages.borrow_mut().push(older.clone());
            }
            state.visible_limit.set(100);
            state.render_list();
            fn layout() {
                let context = gtk::glib::MainContext::default();
                let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
                while std::time::Instant::now() < until {
                    while context.pending() {
                        context.iteration(false);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            fn activate(state: &Rc<State>, message: &Message) {
                let position =
                    ui::virtual_list::position(&state.list, &models::senders::key(message))
                        .unwrap();
                ui::virtual_list::selection(&state.list).set_selected(position);
                state.list.grab_focus();
                state
                    .list
                    .scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
                layout();
                state.list.emit_by_name::<()>("activate", &[&position]);
            }
            window.present();
            layout();
            activate(&state, &marked);
            layout();
            let target = models::action_target::ActionTarget::Messages(vec![(
                marked.uid,
                marked.message_id.clone(),
            )]);
            state.sender_action(target.clone(), SenderAction::Spam);
            let remaining = state
                .messages
                .borrow()
                .iter()
                .filter(|message| message.uid != marked.uid)
                .cloned()
                .collect();
            state.event(Event::SenderActionFinished {
                account: "account".into(),
                folder: "INBOX".into(),
                sender: target,
                messages: Some(remaining),
                removed: vec![marked.clone()],
                error: None,
            });
            layout();
            activate(&state, &other);
            layout();
            let position =
                ui::virtual_list::position(&state.list, &models::senders::key(&marked)).unwrap();
            ui::virtual_list::selection(&state.list).set_selected(position);
            state.list.grab_focus();
            state
                .list
                .scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
            layout();
            let focused = gtk::prelude::RootExt::focus(&window).unwrap();
            assert!(
                focused == *state.list.upcast_ref::<gtk::Widget>()
                    || focused.is_ancestor(&state.list),
                "focus is {}",
                focused.type_().name()
            );
            let before = state.list_scroll.vadjustment().value();
            assert!(
                before > 0.0,
                "upper {} page {} position {position}",
                state.list_scroll.vadjustment().upper(),
                state.list_scroll.vadjustment().page_size()
            );
            state.list.emit_by_name::<()>("activate", &[&position]);
            layout();
            let after = state.list_scroll.vadjustment().value();
            assert!(
                (after - before).abs() < 1.0,
                "sender scroll moved from {before} to {after}"
            );
            assert!(
                !state
                    .visible_messages()
                    .iter()
                    .any(|message| message.uid == marked.uid)
            );
            assert_eq!(
                state
                    .visible_messages()
                    .iter()
                    .any(|message| message.uid == older.uid),
                has_older
            );
        }
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect retained spam and undo"]
    fn spam_rows_stay_open_can_be_undone_and_disappear_when_sender_reopens() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSpamDiagnostic")
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
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let first = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            sender: "sender@example.com".into(),
            subject: "Keep this conversation open".into(),
            body_loaded: true,
            body_text: "Cached message body".into(),
            is_read: true,
            is_flagged: true,
            timestamp: 3,
            ..Default::default()
        };
        let second = Message {
            uid: 2,
            message_id: "two".into(),
            subject: "Another conversation".into(),
            timestamp: 2,
            ..first.clone()
        };
        let other = Message {
            uid: 3,
            message_id: "other".into(),
            sender: "other@example.com".into(),
            timestamp: 1,
            ..first.clone()
        };
        *state.messages.borrow_mut() = vec![first.clone(), second.clone(), other.clone()];
        state.render_list();
        state.filter_sender(Some(first.clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![first.clone(), second.clone()],
            false,
        ));
        state.show_thread(vec![first.clone()]);
        let target = models::action_target::ActionTarget::Messages(vec![(1, "one".into())]);
        let card = state.cards.borrow()[&1].widget.clone();
        fn find(widget: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
            if widget.has_css_class(class) {
                return Some(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(found) = find(&widget, class) {
                    return Some(found);
                }
                child = widget.next_sibling();
            }
            None
        }
        let icon = find(card.upcast_ref(), "message-row-spam-icon").unwrap();
        assert!(!icon.get_visible());
        state.sender_action(target.clone(), SenderAction::Spam);
        assert!(icon.get_visible());
        assert_eq!(*state.selected.borrow(), vec![1]);
        assert_eq!(state.visible_messages().len(), 3);
        assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 2);
        assert_eq!(state.menu_actions(&target), vec![SenderAction::Unspam]);
        while commands.try_recv().is_ok() {}
        // Undo can be queued before the server confirms the move.
        state.sender_action(target.clone(), SenderAction::Unspam);
        assert!(!icon.get_visible());
        assert!(
            matches!(commands.try_recv().unwrap(), Command::UndoSpam { originals, folder, .. } if originals[0].uid == 1 && folder == "INBOX")
        );
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(vec![second.clone(), other.clone()]),
            removed: vec![first.clone()],
            error: None,
        });
        state.event(Event::CacheList(
            "account".into(),
            "INBOX".into(),
            vec![second.clone(), other.clone()],
        ));
        assert_eq!(state.visible_messages().len(), 3);
        assert_eq!(*state.selected.borrow(), vec![1]);
        assert!(!icon.get_visible());
        state.event(Event::SpamUndoFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            originals: vec![first.clone()],
            restored: vec![],
            messages: None,
            error: Some("Retry".into()),
        });
        assert!(icon.get_visible());
        state.sender_action(target.clone(), SenderAction::Unspam);
        let restored = Message {
            uid: 44,
            body_loaded: false,
            body_text: String::new(),
            ..first.clone()
        };
        state.event(Event::SpamUndoFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            originals: vec![first.clone()],
            restored: vec![first.clone()],
            messages: Some(vec![restored.clone(), second.clone(), other.clone()]),
            error: None,
        });
        let visible = state.visible_messages();
        assert_eq!(visible.len(), 3);
        assert!(
            visible
                .iter()
                .all(|message| !message.is_spam && message.uid != 1)
        );
        assert_eq!(*state.selected.borrow(), vec![44]);
        assert_eq!(state.open_group.borrow()[0].body_text, first.body_text);
        assert!(state.cards.borrow().contains_key(&44));
        assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 2);
        assert!(ui::virtual_list::selected(&state.thread_list).is_some());
        state.event(Event::CacheList(
            "account".into(),
            "INBOX".into(),
            vec![second.clone(), other.clone()],
        ));
        assert_eq!(state.visible_messages().len(), 3);
        assert!(
            state
                .visible_messages()
                .iter()
                .any(|message| message.uid == 44 && !message.is_spam)
        );

        let new_target = models::action_target::ActionTarget::Messages(vec![(44, "one".into())]);
        state.sender_action(new_target.clone(), SenderAction::Spam);
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: new_target,
            messages: Some(vec![second.clone(), other.clone()]),
            removed: vec![restored.clone()],
            error: None,
        });
        assert_eq!(state.visible_messages().len(), 3);
        state.filter_sender(None);
        assert_eq!(state.visible_messages().len(), 3);
        state.filter_sender(Some(second.clone()));
        assert_eq!(state.visible_messages().len(), 2);
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![second.clone()],
            false,
        ));
        assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 1);
        // Reopening during an in-flight move must not let its later acknowledgement resurrect the row.
        let target = models::action_target::ActionTarget::Messages(vec![(2, "two".into())]);
        state.sender_action(target.clone(), SenderAction::Spam);
        state.filter_sender(None);
        state.filter_sender(Some(second.clone()));
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target,
            messages: Some(vec![other]),
            removed: vec![second],
            error: None,
        });
        assert_eq!(state.visible_messages().len(), 1);
        assert!(
            state
                .visible_messages()
                .iter()
                .all(|message| !message.is_spam)
        );
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session for message actions and the folder picker"]
    fn stars_unread_and_move_picker_target_the_clicked_mail() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaMessageActionsDiagnostic")
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
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        state.folder_names.borrow_mut().insert(
            "account".into(),
            vec!["INBOX".into(), "Work".into(), "Archive".into()],
        );
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            sender: "sender@example.com".into(),
            message_id: "one".into(),
            subject: "Message actions".into(),
            is_read: true,
            body_loaded: true,
            ..Default::default()
        };
        let target = models::action_target::ActionTarget::Messages(vec![(1, "one".into())]);
        *state.messages.borrow_mut() = vec![message.clone()];
        state.render_list();
        state.filter_sender(Some(message.clone()));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![message.clone()],
            false,
        ));
        state.show_thread(vec![message.clone()]);
        let card = state.cards.borrow()[&1].widget.clone();
        fn star(widget: &gtk::Widget) -> Option<gtk::Widget> {
            if widget.has_css_class("message-row-flag-icon") {
                return Some(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(found) = star(&widget) {
                    return Some(found);
                }
                child = widget.next_sibling();
            }
            None
        }
        let flag = star(card.upcast_ref()).unwrap();
        assert!(!flag.get_visible());
        assert!(state.menu_actions(&target).contains(&SenderAction::Star));
        state.sender_action(target.clone(), SenderAction::Star);
        assert!(flag.get_visible());
        assert!(!state.selected.borrow().is_empty());
        assert!(state.menu_actions(&target).contains(&SenderAction::Unstar));
        assert!(!state.menu_actions(&target).contains(&SenderAction::Star));
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(vec![Message {
                is_flagged: true,
                flag_revision: 10,
                ..message.clone()
            }]),
            removed: vec![],
            error: None,
        });
        assert!(flag.get_visible());
        state.sender_action(target.clone(), SenderAction::Unstar);
        assert!(!flag.get_visible());
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(vec![Message {
                flag_revision: 11,
                ..message.clone()
            }]),
            removed: vec![],
            error: None,
        });
        state.sender_action(target.clone(), SenderAction::MarkUnread);
        assert!(state.selected.borrow().is_empty());
        assert!(!state.visible_messages()[0].is_read);
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: target.clone(),
            messages: Some(vec![Message {
                is_read: false,
                read_revision: 12,
                flag_revision: 11,
                ..message.clone()
            }]),
            removed: vec![],
            error: None,
        });
        assert!(!state.messages.borrow()[0].is_read);
        assert!(state.cards.borrow().is_empty());

        *state.folder.borrow_mut() = "[Gmail]/Trash".into();
        assert!(state.menu_actions(&target).contains(&SenderAction::Restore));
        assert!(state.menu_actions(&target).contains(&SenderAction::Unspam));
        *state.folder.borrow_mut() = "Junk".into();
        assert!(state.menu_actions(&target).contains(&SenderAction::Unspam));
        assert!(!state.menu_actions(&target).contains(&SenderAction::Spam));
        *state.folder.borrow_mut() = "INBOX".into();
        while commands.try_recv().is_ok() {}
        let dialog = state.move_dialog(target.clone()).unwrap();
        let dropdown = dialog
            .extra_child()
            .unwrap()
            .downcast::<gtk::DropDown>()
            .unwrap();
        assert_eq!(dropdown.model().unwrap().n_items(), 2);
        dropdown.set_selected(1); // Work, after Archive.
        dialog.response("move");
        assert!(
            matches!(commands.try_recv().unwrap(), Command::SenderAction { sender, action: SenderAction::MoveTo(folder), .. } if sender == target && folder == "Work")
        );
        assert!(state.visible_messages().is_empty());
        window.close();
    }

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
            smtp: None,
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
            state.sender_action("sender@example.com".into(), action.clone());
            assert!(
                matches!(commands.try_recv().unwrap(), Command::SenderAction { action: sent, .. } if sent == action)
            );
            if action == SenderAction::MarkRead {
                assert!(ui::virtual_list::groups(&state.list)[0][0].is_read);
            } else if action == SenderAction::Spam {
                assert!(ui::virtual_list::groups(&state.list)[0][0].is_spam);
            } else {
                assert!(ui::virtual_list::groups(&state.list).is_empty());
            }
            state.event(Event::Messages(
                state.generation.get(),
                vec![message.clone()],
                false,
            ));
            if action == SenderAction::MarkRead {
                assert!(ui::virtual_list::groups(&state.list)[0][0].is_read);
            } else if action == SenderAction::Spam {
                assert!(ui::virtual_list::groups(&state.list)[0][0].is_spam);
            } else {
                assert!(ui::virtual_list::groups(&state.list).is_empty());
            }
            state.event(Event::SenderActionFinished {
                account: "account".into(),
                folder: "INBOX".into(),
                sender: "sender@example.com".into(),
                messages: None,
                removed: Vec::new(),
                error: Some("Server refused action".into()),
            });
            assert_eq!(ui::virtual_list::groups(&state.list)[0][0], message);
        }
        state.sender_action("sender@example.com".into(), SenderAction::Delete);
        state.event(Event::SenderActionFinished {
            account: "account".into(),
            folder: "INBOX".into(),
            sender: "sender@example.com".into(),
            messages: Some(Vec::new()),
            removed: vec![message.clone()],
            error: None,
        });
        assert!(state.messages.borrow().is_empty());
        assert!(ui::virtual_list::groups(&state.list).is_empty());
        let other = Message {
            uid: 2,
            subject: "Other conversation".into(),
            message_id: "other".into(),
            ..message.clone()
        };
        *state.sender_actions.borrow_mut() = Default::default();
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
            state.sender_action(target.clone(), action.clone());
            let visible = state.visible_messages();
            assert_eq!(
                visible.iter().find(|message| message.uid == 2),
                Some(&other)
            );
            if action == SenderAction::Spam {
                assert!(
                    visible
                        .iter()
                        .find(|message| message.uid == 1)
                        .unwrap()
                        .is_spam
                );
            } else if action == SenderAction::MarkRead {
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
                removed: Vec::new(),
                error: Some("Server refused action".into()),
            });
            assert_eq!(
                state.visible_messages(),
                vec![message.clone(), other.clone()]
            );
        }
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
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![unread.clone(), newer.clone()],
            false,
        ));
        ui::virtual_list::selection(&state.thread_list).set_selected(0);
        state.show_thread(vec![unread.clone()]);
        let read = Message {
            is_read: true,
            ..unread.clone()
        };
        state.update_body(&read);
        assert_eq!(ui::virtual_list::selected(&state.thread_list), Some(0));
        assert!(ui::virtual_list::groups(&state.thread_list)[0][0].is_read);
        assert_eq!(
            ui::virtual_list::groups(&state.list)[0][0].sender,
            unread.sender
        );
        let snapshot = state.messages.borrow().clone();
        state.event(Event::Messages(state.generation.get(), snapshot, false));
        state.show_thread(vec![read.clone()]);
        assert_eq!(ui::virtual_list::selected(&state.thread_list), Some(0));
        state.show_thread(vec![newer.clone()]);
        assert_eq!(ui::virtual_list::selected(&state.thread_list), Some(1));
        assert_eq!(
            ui::virtual_list::groups(&state.list)[0][0].uid,
            elsewhere.uid
        );

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
                if state.sender_pane.borrow().loading() {
                    let ticket = state.sender_pane.borrow().ticket();
                    state.event(Event::SenderPage(
                        state.generation.get(),
                        ticket,
                        vec![expected.clone()],
                        false,
                    ));
                }
                assert_eq!(*state.selected.borrow(), vec![expected.uid]);
                assert!(state.cards.borrow()[&expected.uid].is_expanded());
                assert_eq!(ui::virtual_list::selected(&state.thread_list), Some(0));
                assert_eq!(
                    ui::virtual_list::groups(&state.thread_list)[0][0].uid,
                    expected.uid
                );
            } else {
                assert!(state.selected.borrow().is_empty());
                assert!(state.cards.borrow().is_empty());
                assert!(ui::virtual_list::groups(&state.list).is_empty());
            }
        }
        window.close();
    }
}
