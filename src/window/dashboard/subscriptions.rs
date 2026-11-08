use super::*;

impl State {
    pub(super) fn unsubscribe_dashboard(self: &Rc<Self>, entry: &Entry) {
        if self.bulk_unsubscribe_active()
            || self
                .dashboard
                .unsubscribing
                .borrow()
                .contains_key(&entry.id)
        {
            return;
        }
        let Some(account) = self
            .folder_boxes
            .borrow()
            .get(&entry.source.account)
            .map(|(account, _)| account.clone())
        else {
            return;
        };
        if let Err(error) = self.sender.send(Command::Unsubscribe {
            account,
            folder: entry.source.folder.clone(),
            uid: entry.source.message.uid,
            message_id: entry.source.message.message_id.clone(),
        }) {
            self.toast.add_toast(adw::Toast::new(&error.to_string()));
            return;
        }
        self.dashboard
            .unsubscribing
            .borrow_mut()
            .insert(entry.id.clone(), entry.source.clone());
        self.dashboard.view.unsubscribe_pending(&entry.id, true);
    }

    pub(in crate::window) fn dashboard_unsubscribe_finished(
        self: &Rc<Self>,
        account: &str,
        folder: &str,
        uid: u32,
        message_id: &str,
        result: &Result<crate::backend::unsubscribe::Outcome, String>,
    ) {
        let done = matches!(result, Ok(crate::backend::unsubscribe::Outcome::Done));
        let id = self
            .dashboard
            .unsubscribing
            .borrow()
            .iter()
            .find(|(_, source)| {
                source.account == account
                    && source.folder == folder
                    && source.message.uid == uid
                    && source.message.message_id == message_id
            })
            .map(|(id, _)| id.clone());
        let Some(id) = id else { return };
        self.dashboard.unsubscribing.borrow_mut().remove(&id);
        let status = match result {
            Ok(crate::backend::unsubscribe::Outcome::Done) => ui::subscriptions::Status::Done,
            Ok(crate::backend::unsubscribe::Outcome::Open(url)) => {
                ui::subscriptions::Status::Manual(url.clone())
            }
            Err(error) => ui::subscriptions::Status::Failed(error.clone()),
        };
        self.subscriptions.status(&id, status);
        if done {
            self.dashboard.dismissed.borrow_mut().insert(id);
            self.save_dashboard_dismissed();
            if self.dashboard_visible() {
                self.render_dashboard();
            }
        } else {
            self.dashboard.view.unsubscribe_pending(&id, false);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::dashboard::{Content, Source};

    #[test]
    #[ignore = "Requires a private graphical session and fixture XDG directories; uses a recording worker"]
    fn bulk_unsubscribe_selects_virtual_rows_tracks_outcomes_and_stops_the_queue() {
        let (worker, commands) = worker::Worker::recording();
        let state = crate::window::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaBulkUnsubscribeDiagnostic",
            worker,
        );
        if std::env::var_os("BREVLADA_DIAGNOSTIC_DARK").is_some() {
            adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        }
        let css = gtk::CssProvider::new();
        css.load_from_string(crate::theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let account = state.account.borrow().clone().unwrap();
        state.event(Event::Accounts(vec![account.clone()]));
        state.dashboard_button.emit_clicked();
        let request = commands
            .try_iter()
            .find_map(|command| match command {
                Command::Dashboard { request, .. } => Some(request),
                _ => None,
            })
            .unwrap();
        let entries: Vec<_> = (0..1000)
            .map(|index| Entry {
                recent_emails: (if index < 4 {
                    100 - index
                } else if index == 999 {
                    96
                } else {
                    index % 20 + 1
                }) as usize,
                id: format!("bulk-fixture-{index}"),
                source: Source {
                    account: account.email.clone(),
                    folder: "Newsletters".into(),
                    message: Message {
                        uid: index + 1,
                        message_id: format!("bulk-{index}"),
                        sender: format!("Newsletter {index} <news{index}@example.com>"),
                        unsubscribe: Some(models::Unsubscribe {
                            url: "https://example.com/unsubscribe".into(),
                            one_click: Some(true),
                        }),
                        ..Default::default()
                    },
                },
                content: Content::Subscription,
                series: Vec::new(),
            })
            .collect();
        for entry in &entries {
            state.dashboard.dismissed.borrow_mut().remove(&entry.id);
        }
        state.event(Event::Dashboard(request, Ok(entries.clone())));
        let window = state.toast.root().and_downcast::<gtk::Window>().unwrap();
        window.present();
        let settle = || {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(150)))
        };
        fn find<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget) -> Option<T> {
            if let Ok(found) = widget.clone().downcast::<T>() {
                return Some(found);
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(found) = find::<T>(&widget) {
                    return Some(found);
                }
            }
            None
        }
        let popup = || {
            gtk::Window::list_toplevels()
                .into_iter()
                .filter(|widget| widget.is_visible())
                .find_map(|widget| widget.downcast::<adw::MessageDialog>().ok())
                .unwrap()
        };
        settle();
        state.dashboard.view.unsubscribe_all.emit_clicked();
        settle();
        assert_eq!(
            state.content_stack.visible_child_name().as_deref(),
            Some("subscriptions")
        );
        let screen = &state.subscriptions;
        let opened = Rc::new(RefCell::new(Vec::<String>::new()));
        screen.record_links(opened.clone());
        let list = &screen.list;
        let model = list.model().and_downcast::<gtk::MultiSelection>().unwrap();
        let position = |index: usize| {
            (0..model.n_items())
                .find(|&position| {
                    model
                        .item(position)
                        .and_downcast::<glib::BoxedAnyObject>()
                        .unwrap()
                        .borrow::<Entry>()
                        .id
                        == entries[index].id
                })
                .unwrap()
        };
        let counts: Vec<_> = (0..model.n_items())
            .map(|position| {
                model
                    .item(position)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .unwrap()
                    .borrow::<Entry>()
                    .recent_emails
            })
            .collect();
        assert!(counts.windows(2).all(|pair| pair[0] >= pair[1]));
        assert_eq!(model.n_items(), 1000);
        assert_eq!(model.selection().size(), 1000);
        assert_eq!(
            screen.bottom.unsubscribe.label().as_deref(),
            Some("Unsubscribe 1000")
        );
        let first = find::<gtk::CheckButton>(list.upcast_ref()).unwrap();
        first.set_active(false);
        assert_eq!(model.selection().size(), 999);
        assert_eq!(
            screen.bottom.unsubscribe.label().as_deref(),
            Some("Unsubscribe 999")
        );
        first.set_active(true);
        list.emit_by_name::<()>("activate", &[&0u32]);
        assert_eq!(model.selection().size(), 999);
        list.emit_by_name::<()>("activate", &[&0u32]);
        screen.bottom.all.set_active(false);
        assert!(model.selection().is_empty());
        assert!(!screen.bottom.unsubscribe.is_sensitive());
        screen.bottom.all.set_active(true);
        assert_eq!(model.selection().size(), 1000);
        assert!(list.observe_children().n_items() < 400);
        assert!(find::<adw::Avatar>(list.upcast_ref()).is_some());
        assert!(find::<adw::PreferencesRow>(list.upcast_ref()).is_some());
        model.unselect_all();
        for index in [0, 1, 2, 3, 999] {
            model.select_item(position(index), false);
        }
        assert!(screen.bottom.all.is_inconsistent());
        let first_widget = list.first_child().unwrap();
        let mut refreshed = entries.clone();
        refreshed[0].recent_emails += 1;
        screen.update(refreshed);
        assert_eq!(list.first_child().as_ref(), Some(&first_widget));
        assert_eq!(model.selection().size(), 5);
        let mut reordered = entries.clone();
        reordered[9].recent_emails = 200;
        screen.update(reordered);
        assert_eq!(
            model
                .item(0)
                .and_downcast::<glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<Entry>()
                .id,
            entries[9].id
        );
        assert_eq!(model.selection().size(), 5);
        for index in [0, 1, 2, 3, 999] {
            assert!(model.is_selected(position(index)));
        }
        screen.update(entries.clone());
        settle();
        let grid = find::<gtk::Grid>(list.upcast_ref()).unwrap();
        let first_column = grid.child_at(0, 0).unwrap();
        let email_column = grid.child_at(1, 0).unwrap();
        let count_column = grid.child_at(2, 0).unwrap();
        assert!((first_column.width() - email_column.width()).abs() <= 1);
        assert!((email_column.width() - count_column.width()).abs() <= 1);
        let action_row = find::<adw::PreferencesRow>(list.upcast_ref()).unwrap();
        assert!(f64::from(grid.width()) > f64::from(action_row.width()) * 0.9);
        let previews = state.dashboard_button.prev_sibling().unwrap();
        let sync_preview = previews
            .first_child()
            .and_downcast::<gtk::MenuButton>()
            .unwrap();
        let unsubscribe_preview = sync_preview
            .next_sibling()
            .and_downcast::<gtk::MenuButton>()
            .unwrap();
        let preview = if std::env::var("BREVLADA_PROCESS_PREVIEW").as_deref() == Ok("unsubscribe") {
            &unsubscribe_preview
        } else {
            &sync_preview
        };
        if std::env::var_os("BREVLADA_PROCESS_PREVIEW").is_some() {
            preview.popup();
            settle();
        }
        if std::env::var_os("BREVLADA_DIAGNOSTIC_HOLD").is_some() {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_secs(8)));
        }
        preview.popdown();
        list.scroll_to(999, gtk::ListScrollFlags::NONE, None);
        settle();
        assert_eq!(model.selection().size(), 5);
        screen.bottom.unsubscribe.emit_clicked();
        popup().response("cancel");
        assert!(
            !commands
                .try_iter()
                .any(|command| matches!(command, Command::Unsubscribe { .. }))
        );
        screen.bottom.unsubscribe.emit_clicked();
        popup().response("confirm");
        let sent = || {
            commands
                .try_iter()
                .filter_map(|command| match command {
                    Command::Unsubscribe {
                        uid,
                        folder,
                        account: source,
                        ..
                    } => {
                        assert_eq!(folder, "Newsletters");
                        assert_eq!(source.email, account.email);
                        Some(uid)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(sent(), vec![1]);
        assert!(!state.dashboard.view.unsubscribe_all.is_sensitive());
        state.unsubscribe_dashboard(&entries[10]);
        assert!(sent().is_empty());
        let finish = |index: usize, result| {
            state.event(Event::Unsubscribe {
                account: account.email.clone(),
                folder: "Newsletters".into(),
                uid: entries[index].source.message.uid,
                message_id: entries[index].source.message.message_id.clone(),
                result,
            })
        };
        finish(10, Err("unrelated event".into()));
        assert!(sent().is_empty());
        finish(0, Ok(crate::backend::unsubscribe::Outcome::Done));
        assert_eq!(sent(), vec![2]);
        assert!(state.dashboard.dismissed.borrow().contains(&entries[0].id));
        finish(
            1,
            Ok(crate::backend::unsubscribe::Outcome::Open(
                "https://example.com/manual".into(),
            )),
        );
        assert_eq!(sent(), vec![3]);
        finish(2, Err("fixture failure".into()));
        assert_eq!(
            model
                .item(0)
                .and_downcast::<glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<Entry>()
                .id,
            entries[2].id
        );
        assert!(opened.borrow().is_empty());
        assert_eq!(sent(), vec![4]);
        screen.stop.emit_clicked();
        assert!(state.bulk_unsubscribe_active());
        assert!(!state.dashboard.view.unsubscribe_stop.is_sensitive());
        finish(3, Ok(crate::backend::unsubscribe::Outcome::Done));
        assert!(sent().is_empty());
        assert!(!state.bulk_unsubscribe_active());
        assert!(state.dashboard.view.unsubscribe_all.is_sensitive());
        assert!(!state.dashboard.view.unsubscribe_stop.get_visible());
        assert!(
            !state
                .dashboard
                .dismissed
                .borrow()
                .contains(&entries[999].id)
        );
        model.unselect_all();
        model.select_item(position(1), false);
        screen.bottom.unsubscribe.emit_clicked();
        popup().response("confirm");
        assert_eq!(sent(), vec![2]);
        finish(1, Ok(crate::backend::unsubscribe::Outcome::Done));
        assert!(!state.bulk_unsubscribe_active());
        assert!(state.dashboard.view.unsubscribe_all.is_sensitive());
        list.scroll_to(position(2), gtk::ListScrollFlags::NONE, None);
        settle();
        fn retry(widget: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(button) = widget.downcast_ref::<gtk::Button>()
                && button.label().as_deref() == Some("Retry")
            {
                return Some(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(button) = retry(&widget) {
                    return Some(button);
                }
            }
            None
        }
        retry(list.upcast_ref()).unwrap().emit_clicked();
        assert_eq!(sent(), vec![3]);
        finish(2, Err("retry fixture failure".into()));
        assert_eq!(
            *opened.borrow(),
            vec!["https://example.com/unsubscribe".to_owned()]
        );
        assert!(screen.has_failed(&entries[2].id));
        assert!(!state.bulk_unsubscribe_active());
        finish(2, Err("duplicate completion".into()));
        assert_eq!(opened.borrow().len(), 1);
        screen.back.emit_clicked();
        state.dashboard.view.unsubscribe_all.emit_clicked();
        assert!(screen.has_failed(&entries[2].id));
        list.scroll_to(position(2), gtk::ListScrollFlags::NONE, None);
        settle();
        retry(list.upcast_ref()).unwrap().emit_clicked();
        assert_eq!(sent(), vec![3]);
        finish(2, Ok(crate::backend::unsubscribe::Outcome::Done));
        assert_eq!(opened.borrow().len(), 1);
        assert!(!state.bulk_unsubscribe_active());
        for preview in [&sync_preview, &unsubscribe_preview] {
            assert!(preview.popover().unwrap().has_css_class("background"));
            preview.popup();
            settle();
            let detail = preview.popover().unwrap().child().unwrap();
            let scroll = find::<gtk::ScrolledWindow>(&detail).unwrap();
            assert!(scroll.vadjustment().upper() > scroll.vadjustment().page_size());
            let actions = detail.last_child().unwrap();
            let pause = actions.first_child().and_downcast::<gtk::Button>().unwrap();
            let cancel = actions.last_child().and_downcast::<gtk::Button>().unwrap();
            pause.emit_clicked();
            assert_eq!(pause.label().as_deref(), Some("Resume"));
            pause.emit_clicked();
            assert_eq!(pause.label().as_deref(), Some("Pause"));
            cancel.emit_clicked();
            assert!(!preview.is_visible());
            assert!(sent().is_empty());
        }
        assert!(!previews.is_visible());
        screen.back.emit_clicked();
        assert_eq!(
            state.content_stack.visible_child_name().as_deref(),
            Some("dashboard")
        );
        window.close();
    }

    #[test]
    #[ignore = "Requires a private graphical session and fixture XDG directories; records unsubscribe requests without network access"]
    fn dashboard_unsubscribe_routes_to_source_retries_and_hides_on_success() {
        let (worker, commands) = worker::Worker::recording();
        let state = crate::window::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaDashboardUnsubscribeDiagnostic",
            worker,
        );
        let current = state.account.borrow().clone().unwrap();
        let other = Account {
            email: "work@example.com".into(),
            ..current.clone()
        };
        state.event(Event::Accounts(vec![current.clone(), other.clone()]));
        state.dashboard_button.emit_clicked();
        let request = commands
            .try_iter()
            .find_map(|command| match command {
                Command::Dashboard { request, .. } => Some(request),
                _ => None,
            })
            .unwrap();
        let entry = Entry {
            recent_emails: 0,
            id: "dashboard-subscription-fixture".into(),
            source: Source {
                account: other.email.clone(),
                folder: "Newsletters".into(),
                message: Message {
                    uid: 42,
                    message_id: "newsletter".into(),
                    sender: "News <news@example.com>".into(),
                    unsubscribe: Some(models::Unsubscribe {
                        url: "https://example.com/unsubscribe".into(),
                        one_click: Some(true),
                    }),
                    ..Default::default()
                },
            },
            content: Content::Subscription,
            series: Vec::new(),
        };
        state.dashboard.dismissed.borrow_mut().remove(&entry.id);
        state.event(Event::Dashboard(request, Ok(vec![entry.clone()])));
        fn unsubscribe_button(widget: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(button) = widget.downcast_ref::<gtk::Button>()
                && matches!(
                    button
                        .child()
                        .and_then(|content| content.last_child())
                        .and_downcast::<gtk::Label>()
                        .map(|label| label.text())
                        .as_deref(),
                    Some("Unsubscribe" | "Unsubscribing…")
                )
            {
                return Some(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(button) = unsubscribe_button(&widget) {
                    return Some(button);
                }
            }
            None
        }
        fn assert_no_text_selected(widget: &gtk::Widget) {
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                assert!(
                    label.selection_bounds().is_none(),
                    "Selected text: {}",
                    label.text()
                );
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                assert_no_text_selected(&widget);
            }
        }
        let window = state.toast.root().and_downcast::<gtk::Window>().unwrap();
        window.present();
        let settle = || {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(100)))
        };
        settle();
        let button = || unsubscribe_button(state.dashboard.view.widget.upcast_ref());
        let clicked = button().unwrap();
        assert!(clicked.grab_focus());
        clicked.emit_clicked();
        settle();
        assert_eq!(button().as_ref(), Some(&clicked));
        assert_no_text_selected(state.dashboard.view.widget.upcast_ref());
        assert!(!button().unwrap().is_sensitive());
        assert!(commands.try_iter().any(|command| matches!(command,
            Command::Unsubscribe { account, folder, uid: 42, message_id }
            if account.email == other.email && folder == "Newsletters" && message_id == "newsletter")));
        state.unsubscribe_dashboard(&entry);
        assert!(
            !commands
                .try_iter()
                .any(|command| matches!(command, Command::Unsubscribe { .. }))
        );
        state.event(Event::Unsubscribe {
            account: other.email.clone(),
            folder: "Newsletters".into(),
            uid: 42,
            message_id: "newsletter".into(),
            result: Err("fixture failure".into()),
        });
        assert_no_text_selected(state.dashboard.view.widget.upcast_ref());
        assert!(button().unwrap().is_sensitive());
        assert!(!state.dashboard.dismissed.borrow().contains(&entry.id));
        button().unwrap().emit_clicked();
        state.event(Event::Unsubscribe {
            account: current.email,
            folder: "Newsletters".into(),
            uid: 42,
            message_id: "newsletter".into(),
            result: Ok(crate::backend::unsubscribe::Outcome::Done),
        });
        assert!(!button().unwrap().is_sensitive());
        state.event(Event::Unsubscribe {
            account: other.email,
            folder: "Newsletters".into(),
            uid: 42,
            message_id: "newsletter".into(),
            result: Ok(crate::backend::unsubscribe::Outcome::Done),
        });
        settle();
        assert_no_text_selected(state.dashboard.view.widget.upcast_ref());
        assert!(button().is_none());
        assert!(state.dashboard.dismissed.borrow().contains(&entry.id));
        let saved: HashSet<String> =
            serde_json::from_slice(&std::fs::read(super::super::dismissed_path()).unwrap())
                .unwrap();
        assert!(saved.contains(&entry.id));
        state
            .toast
            .root()
            .and_downcast::<gtk::Window>()
            .unwrap()
            .close();
    }
}
