use super::*;

impl State {
    pub(super) fn close_sender_pane(&self) {
        search_bar(&self.thread_search).set_search_mode(false);
        search_bar(&self.search).set_search_mode(false);
        self.filter_sender(None);
    }

    pub(super) fn reveal_search_message(&self) {
        let Some((uid, id)) = self.search_message.borrow().clone() else {
            return;
        };
        let position = (0..ui::virtual_list::model(&self.thread_list).n_items()).find(|position| {
            ui::virtual_list::with_item(&self.thread_list, *position, |row| {
                row.messages
                    .iter()
                    .any(|message| message.uid == uid && message.message_id == id)
            })
            .unwrap_or(false)
        });
        if let Some(position) = position {
            ui::virtual_list::reveal(&self.thread_list, position);
            self.search_message.borrow_mut().take();
        } else if !self.sender_pane.borrow().loading() {
            if self.sender_pane.borrow().can_load() {
                self.load_sender_page();
            } else {
                self.search_message.borrow_mut().take();
            }
        }
    }

    pub(super) fn cancel_search(&self, threads: bool) {
        self.search_worker.cancel(threads);
        self.search_busy(threads, false);
        self.search_refresh_again[usize::from(threads)].set(false);
    }

    pub(super) fn search_busy(&self, threads: bool, busy: bool) {
        let lane = usize::from(threads);
        self.search_running[lane].set(busy);
        let stack = if threads {
            &self.thread_stack
        } else {
            &self.list_stack
        };
        self.search_progress[lane].set_visible(
            busy && self.search_show_progress[lane].get()
                && stack.visible_child_name().as_deref() == Some("list"),
        );
    }

    pub(super) fn begin_search(&self, threads: bool, show_progress: bool) {
        self.search_show_progress[usize::from(threads)].set(show_progress);
        let (list, stack) = if threads {
            (&self.thread_list, &self.thread_stack)
        } else {
            (&self.list, &self.list_stack)
        };
        let retain = ui::virtual_list::model(list).n_items() > 0;
        if !retain && show_progress {
            ui::virtual_list::replace(list, 0, None);
            ui::states::list_state(stack, "Searching…", true, false);
        }
        self.search_preserve_results[usize::from(threads)].set(
            retain
                && self.search_published_queries.borrow()[usize::from(threads)]
                    == if threads {
                        self.thread_search.text()
                    } else {
                        self.search.text()
                    }
                    .trim()
                    .to_lowercase(),
        );
        self.search_busy(threads, true);
    }

    pub(super) fn refresh_search_cache(&self, email: &str, folder: &str) {
        if self.sender_search_active()
            && self
                .search_filters
                .folder()
                .is_none_or(|selected| selected == folder)
            && self.search_filters.selected_accounts().contains(email)
        {
            self.search_cache_revision
                .set(self.search_cache_revision.get().wrapping_add(1));
            self.render_or_queue_search(false);
        }
    }

    pub(super) fn search_open(&self, threads: bool) -> bool {
        let entry = if threads {
            &self.thread_search
        } else {
            &self.search
        };
        search_bar(entry).is_search_mode()
    }

    pub(super) fn refresh_account_title(&self) {
        let title = if self.sender_search_active() {
            self.search_filters.account_label()
        } else {
            self.account
                .borrow()
                .as_ref()
                .map(|account| account.email.clone())
                .unwrap_or_default()
        };
        self.account_title.set_label(&title);
        self.account_title
            .set_tooltip_text((!title.is_empty()).then_some(title.as_str()));
        self.account_title
            .set_visible(!self.account_sidebar.get_visible() && !title.is_empty());
    }

    pub(super) fn sender_search_active(&self) -> bool {
        self.search_open(false)
    }

    pub(super) fn select_search_location(
        self: &Rc<Self>,
        location: Option<&crate::backend::search::Location>,
    ) -> bool {
        let Some(location) = location else {
            return true;
        };
        if self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| account.email == location.account)
            && *self.folder.borrow() == location.folder
        {
            return true;
        }
        let account = self
            .folder_boxes
            .borrow()
            .get(&location.account)
            .map(|(account, _)| account.clone());
        let Some(account) = account else {
            return false;
        };
        let preserve = self.sender_search_active();
        if preserve {
            self.search_filters.keep_selection();
            *self.search_account_order.borrow_mut() = Some(
                (0..ui::virtual_list::model(&self.list).n_items())
                    .filter_map(|position| {
                        ui::virtual_list::with_item(&self.list, position, |row| row.key.clone())
                    })
                    .collect(),
            );
        }
        self.select_mailbox(account, location.folder.clone(), preserve);
        true
    }

    pub(super) fn render_or_queue_search(&self, threads: bool) {
        let entry = if threads {
            &self.thread_search
        } else {
            &self.search
        };
        if !self.search_open(threads) || entry.text().trim().is_empty() {
            self.render_pane(threads);
            return;
        }
        if self.search_running[usize::from(threads)].get() {
            self.search_refresh_again[usize::from(threads)].set(true);
            return;
        }
        let pending = self.search_refresh_pending[usize::from(threads)].clone();
        if pending.replace(true) {
            return;
        }
        let bar = search_bar(entry).downgrade();
        let entry = entry.downgrade();
        let background = self.search_background_refresh[usize::from(threads)].clone();
        // Cache sync can deliver many bodies in a burst. Snapshot their latest
        // state once per burst instead of copying the folder after every body.
        glib::timeout_add_local_once(Duration::from_millis(40), move || {
            pending.set(false);
            if let Some(entry) = entry.upgrade()
                && bar.upgrade().is_some_and(|bar| bar.is_search_mode())
            {
                background.set(true);
                entry.emit_by_name::<()>("search-changed", &[]);
            }
        });
    }

    pub(super) fn connect_search(
        self: &Rc<Self>,
        results: async_channel::Receiver<crate::backend::search::Response>,
    ) {
        let weak = Rc::downgrade(self);
        self.search_filters.connect_changed(move |change| {
            if let Some(state) = weak.upgrade() {
                if !state.sender_search_active() {
                    return;
                }
                state.refresh_account_title();
                *state.search_account_order.borrow_mut() = match change {
                    ui::search_filters::Change::Accounts => Some(
                        (0..ui::virtual_list::model(&state.list).n_items())
                            .filter_map(|position| {
                                ui::virtual_list::with_item(&state.list, position, |row| {
                                    row.key.clone()
                                })
                            })
                            .collect(),
                    ),
                    ui::search_filters::Change::Criteria => None,
                };
                state.cancel_search(false);
                state.rendering.set(true);
                state.render_pane(false);
                state.rendering.set(false);
            }
        });
        for (threads, entry) in [(false, &self.search), (true, &self.thread_search)] {
            let weak = Rc::downgrade(self);
            search_bar(entry).connect_search_mode_enabled_notify(move |bar| {
                if let Some(state) = weak.upgrade() {
                    state.cancel_search(threads);
                    if !threads {
                        if bar.is_search_mode() && state.account.borrow().is_some() {
                            state.search_filters.reset_scope();
                        }
                        state.refresh_account_title();
                    }
                    if !bar.is_search_mode() {
                        let entry = if threads {
                            &state.thread_search
                        } else {
                            &state.search
                        };
                        entry.set_text("");
                        if !threads {
                            *state.search_account_order.borrow_mut() = None;
                            state.filter_sender(None);
                        }
                    }
                    state.rendering.set(true);
                    state.render_pane(threads);
                    state.rendering.set(false);
                }
            });
            let weak = Rc::downgrade(self);
            entry.connect_changed(move |entry| {
                // Invalidate immediately, before SearchEntry's debounce expires.
                if let Some(state) = weak.upgrade() {
                    state.cancel_search(threads);
                    if !threads {
                        *state.search_account_order.borrow_mut() = None;
                    }
                    if state.search_open(threads) && !entry.text().trim().is_empty() {
                        state.begin_search(threads, true);
                    } else {
                        state.search_busy(threads, false);
                        state.render_pane(threads);
                    }
                }
            });
            let weak = Rc::downgrade(self);
            entry.connect_search_changed(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.rendering.set(true);
                    let lane = usize::from(threads);
                    let show_progress = !state.search_background_refresh[lane].replace(false)
                        || (state.search_running[lane].get()
                            && state.search_show_progress[lane].get());
                    state.render_pane_with_progress(threads, show_progress);
                    state.rendering.set(false);
                    if threads {
                        state.maybe_load_sender_page();
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(mut response) = results.recv().await {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                if state.search_worker.accepts(&response) {
                    // A read/star update can arrive while a search is scanning an
                    // older snapshot. Never let a partial batch undo that update.
                    {
                        let account = state.account.borrow();
                        let folder_name = state.folder.borrow();
                        let folder = state.messages.borrow();
                        let pane = state.sender_pane.borrow();
                        let opened = state.open_group.borrow();
                        let threads = response.threads;
                        let mut groups =
                            response.groups.iter_mut().enumerate().filter(|(index, _)| {
                                threads
                                    || response
                                        .locations
                                        .get(*index)
                                        .and_then(Option::as_ref)
                                        .is_some_and(|location| {
                                            account.as_ref().is_some_and(|account| {
                                                location.account == account.email
                                                    && location.folder == *folder_name
                                            })
                                        })
                            });
                        models::read_state::merge_into(
                            groups.by_ref().flat_map(|(_, group)| group.iter_mut()),
                            folder.iter().chain(pane.messages()).chain(opened.iter()),
                        );
                    }
                    state.rendering.set(true);
                    state.search_busy(response.threads, !response.complete);
                    if let Some(error) = response.error {
                        ui::states::list_state(
                            &state.list_stack,
                            "Could not search selected accounts",
                            false,
                            false,
                        );
                        state
                            .toast
                            .add_toast(adw::Toast::new(&format!("Search: {error}")));
                    } else {
                        state.publish_search_groups(
                            response.threads,
                            &response.query,
                            response.groups,
                            response.locations,
                        );
                    }
                    state.search_busy(response.threads, !response.complete);
                    state.rendering.set(false);
                    if response.complete
                        && state.search_refresh_again[usize::from(response.threads)].replace(false)
                    {
                        state.render_or_queue_search(response.threads);
                    }
                }
            }
        });
    }
}

fn search_bar(entry: &gtk::SearchEntry) -> gtk::SearchBar {
    entry
        .ancestor(gtk::SearchBar::static_type())
        .and_downcast::<gtk::SearchBar>()
        .expect("Search entries belong to a search bar")
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn opening_unloaded_search_results_requests_read_updates_and_preserves_them() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSearchReadDiagnostic")
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
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let matched = Message {
            uid: 1,
            message_id: "search-match".into(),
            sender: "sender@example.com".into(),
            body_loaded: true,
            search_match: true,
            ..Default::default()
        };
        window.set_default_size(1400, 450);
        window.present();
        ui::virtual_list::update(
            &state.list,
            &[ui::virtual_list::Row {
                key: "sender@example.com".into(),
                messages: vec![matched.clone()],
            }],
            &[],
            None,
        );
        state.list.emit_by_name::<()>("activate", &[&0u32]);
        assert!(state.messages.borrow().is_empty());
        assert!(state.sender_pane.borrow().messages().is_empty());
        assert!(state.pending.borrow().contains(&matched.uid));
        layout();
        fn unread(widget: &gtk::Widget) -> bool {
            if widget.has_css_class("message-row-sender-unread") {
                return true;
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if unread(&widget) {
                    return true;
                }
                child = widget.next_sibling();
            }
            false
        }
        assert!(unread(
            &ui::virtual_list::visible_row(&state.list, 0).unwrap()
        ));
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            Message {
                is_read: true,
                read_revision: 1,
                ..matched.clone()
            },
        ));
        assert!(state.open_group.borrow()[0].is_read);
        assert!(!state.pending.borrow().contains(&matched.uid));
        layout();
        assert!(!unread(
            &ui::virtual_list::visible_row(&state.list, 0).unwrap()
        ));
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![matched],
            false,
        ));
        assert!(state.sender_pane.borrow().messages()[0].is_read);
        let older = Message {
            uid: 2,
            message_id: "older-match".into(),
            sender: "sender@example.com".into(),
            ..Default::default()
        };
        ui::virtual_list::update(
            &state.thread_list,
            &[ui::virtual_list::Row {
                key: "older-search-match".into(),
                messages: vec![older.clone()],
            }],
            &[],
            None,
        );
        state.thread_list.emit_by_name::<()>("activate", &[&0u32]);
        assert_eq!(*state.selected.borrow(), vec![older.uid]);
        assert!(state.pending.borrow().contains(&older.uid));
        state.pending.borrow_mut().remove(&older.uid);
        state.preview_pending.borrow_mut().insert(older.uid);
        state.open(older.uid);
        assert!(!state.pending.borrow().contains(&older.uid));
        state.event(Event::Preview(
            state.generation.get(),
            state.selection.get(),
            Message {
                body_loaded: true,
                body_text: "Loaded search match".into(),
                ..older.clone()
            },
        ));
        assert!(state.open_group.borrow()[0].body_loaded);
        assert!(state.pending.borrow().contains(&older.uid));
        layout();
        assert!(unread(
            &ui::virtual_list::visible_row(&state.thread_list, 0).unwrap()
        ));
        state.event(Event::Body(
            state.generation.get(),
            state.selection.get(),
            Message {
                body_loaded: true,
                is_read: true,
                read_revision: 1,
                ..older
            },
        ));
        layout();
        assert!(!unread(
            &ui::virtual_list::visible_row(&state.thread_list, 0).unwrap()
        ));
        window.close();
    }

    fn layout() {
        let context = glib::MainContext::default();
        let until = std::time::Instant::now() + Duration::from_millis(120);
        while std::time::Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect search navigation"]
    fn search_activation_keeps_only_matches_as_pages_and_search_results_arrive() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSearchNavigationDiagnostic")
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
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let account = state.account.borrow().clone().unwrap();
        state
            .search_filters
            .set_accounts(std::slice::from_ref(&account));
        state.search_filters.set_current(Some(&account.email));
        state
            .search_filters
            .set_current_folder(Some("INBOX"), false);
        let location = crate::backend::search::Location {
            account: account.email.clone(),
            folder: "INBOX".into(),
        };
        window.set_default_size(1400, 450);
        window.present();
        let messages: Vec<_> = (1..=60)
            .rev()
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                message_id: format!("id-{uid}"),
                sender: "sender@example.com".into(),
                subject: format!("Message {uid}"),
                body_loaded: true,
                is_read: true,
                ..Default::default()
            })
            .collect();
        let mut matched = messages[49].clone();
        matched.search_match = true;
        let mut older = messages[59].clone();
        older.search_match = true;
        ui::virtual_list::update(
            &state.list,
            &[ui::virtual_list::Row {
                key: location.key("sender@example.com"),
                messages: vec![matched.clone(), older.clone()],
            }],
            &[],
            None,
        );
        state.list.emit_by_name::<()>("activate", &[&0u32]);
        assert_eq!(*state.selected.borrow(), vec![matched.uid]);
        assert!(state.cards.borrow()[&matched.uid].is_expanded());
        assert!(state.search_message.borrow().is_none());
        let visible = || {
            let mut uids: Vec<_> = ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .map(|message| message.uid)
                .collect();
            uids.sort_unstable();
            uids
        };
        assert_eq!(visible(), vec![older.uid, matched.uid]);
        layout();
        let row = ui::virtual_list::visible_row(&state.list, 0).unwrap();
        let content = row.first_child().unwrap().next_sibling().unwrap();
        let badge = content.first_child().unwrap().first_child().unwrap();
        assert!(badge.has_css_class("thread-count-container"));
        assert_eq!(
            badge
                .first_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "2"
        );
        let first_ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            first_ticket,
            messages[..25].to_vec(),
            true,
        ));
        assert_eq!(visible(), vec![older.uid, matched.uid]);
        state.load_sender_page();
        let second_ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            second_ticket,
            messages[25..50].to_vec(),
            true,
        ));
        assert_eq!(visible(), vec![older.uid, matched.uid]);
        assert_eq!(*state.selected.borrow(), vec![matched.uid]);
        state.show_thread(vec![older.clone()]);
        state.load_sender_page();
        let last_ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            last_ticket,
            messages[50..].to_vec(),
            false,
        ));
        assert_eq!(visible(), vec![older.uid, matched.uid]);
        assert_eq!(*state.selected.borrow(), vec![older.uid]);
        let mut later_match = messages[35].clone();
        later_match.search_match = true;
        state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap()
            .set_search_mode(true);
        state.publish_search_groups(
            false,
            "message",
            vec![vec![later_match.clone(), matched.clone(), older.clone()]],
            vec![Some(location.clone())],
        );
        assert_eq!(visible(), vec![older.uid, matched.uid, later_match.uid]);
        assert_eq!(*state.selected.borrow(), vec![older.uid]);
        state.publish_search_groups(
            false,
            "message",
            vec![vec![later_match.clone()]],
            vec![Some(location.clone())],
        );
        assert_eq!(visible(), vec![later_match.uid]);
        *state.messages.borrow_mut() = messages.clone();
        state.search.set_text("message 11");
        state.search.emit_by_name::<()>("search-changed", &[]);
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !predicate() && std::time::Instant::now() < deadline {
                glib::MainContext::default().iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(predicate(), "Search did not finish");
        };
        wait(&|| !state.search_running[0].get() && visible() == vec![matched.uid]);
        let extra = Message {
            uid: 61,
            timestamp: 0,
            message_id: "late-match".into(),
            subject: "Message 11 arrived later".into(),
            ..messages[0].clone()
        };
        state.sender_pane.borrow_mut().select(None);
        state
            .sender_pane
            .borrow_mut()
            .select(Some("sender@example.com".into()));
        state.load_sender_page();
        let ticket = state.sender_pane.borrow().ticket();
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![extra.clone()],
            false,
        ));
        wait(&|| !state.search_running[0].get() && visible() == vec![matched.uid, 61]);
        assert_eq!(ui::virtual_list::groups(&state.list)[0].len(), 2);
        state
            .thread_search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap()
            .set_search_mode(true);
        state.thread_search.set_text("arrived");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        wait(&|| !state.search_running[1].get() && visible() == vec![61]);
        state.thread_search.set_text("");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        assert_eq!(visible(), vec![matched.uid, 61]);
        let target = models::action_target::ActionTarget::Messages(vec![(
            extra.uid,
            extra.message_id.clone(),
        )]);
        state.show_thread(vec![extra.clone()]);
        state.sender_action(target.clone(), models::sender_action::SenderAction::Delete);
        state.event(Event::SenderActionFinished {
            account: account.email.clone(),
            folder: "INBOX".into(),
            sender: target,
            messages: Some(messages.clone()),
            removed: vec![extra],
            error: None,
        });
        wait(&|| {
            !state.search_running[0].get()
                && visible() == vec![matched.uid]
                && ui::virtual_list::groups(&state.list)[0].len() == 1
        });
        state.filter_sender(Some(messages[0].clone()));
        assert!(state.search_sender.borrow().is_none());
        assert!(visible().is_empty());
        window.close();
    }
}
