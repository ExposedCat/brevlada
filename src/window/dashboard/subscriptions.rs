use super::*;

impl State {
    pub(super) fn unsubscribe_dashboard(self: &Rc<Self>, entry: &Entry) {
        if self
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
        done: bool,
    ) {
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
