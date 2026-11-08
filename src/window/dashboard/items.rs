use super::*;

impl State {
    pub(in crate::window) fn render_dashboard(self: &Rc<Self>) {
        let entries: Vec<_> = self
            .dashboard
            .items
            .borrow()
            .iter()
            .filter(|entry| !self.dashboard.dismissed.borrow().contains(&entry.id))
            .cloned()
            .collect();
        let show_accounts = self.dashboard.selected.borrow().is_empty();
        let pending = |entry: &Entry| {
            if let models::dashboard::Content::Event(event) = &entry.content {
                self.calendar_pending.borrow().contains(&(
                    entry.source.account.clone(),
                    event.uid.clone().unwrap_or_default(),
                    event.recurrence_id.clone(),
                ))
            } else if matches!(entry.content, models::dashboard::Content::Subscription) {
                self.dashboard
                    .unsubscribing
                    .borrow()
                    .contains_key(&entry.id)
            } else {
                false
            }
        };
        for tile in self.dashboard.view.render(&entries, show_accounts, pending) {
            if let Some(button) = &tile.unsubscribe {
                let weak = Rc::downgrade(self);
                let entry = tile.entry.clone();
                button.connect_clicked(move |_| {
                    if let Some(state) = weak.upgrade() {
                        state.unsubscribe_dashboard(&entry);
                    }
                });
            }
            let weak = Rc::downgrade(self);
            let entry = tile.entry.clone();
            tile.connect_response(move |response| {
                if let Some(state) = weak.upgrade()
                    && let models::dashboard::Content::Event(event) = &entry.content
                {
                    let account = state
                        .folder_boxes
                        .borrow()
                        .get(&entry.source.account)
                        .map(|(account, _)| account.clone());
                    if let Some(account) = account {
                        state.calendar_reply(
                            account,
                            entry.source.message.clone(),
                            event.clone(),
                            response,
                        );
                    }
                }
            });
            for (button, source) in
                std::iter::once((tile.open, tile.entry.source)).chain(tile.series_open)
            {
                let weak = Rc::downgrade(self);
                button.connect_clicked(move |_| {
                    if let Some(state) = weak.upgrade() {
                        let request = state.sender.next_request();
                        state.dashboard.open_request.set(request);
                        state.send(Command::DashboardOpen {
                            source: Box::new(source.clone()),
                            request,
                        });
                    }
                });
            }
            let weak = Rc::downgrade(self);
            let id = tile.entry.id;
            tile.dismiss.connect_clicked(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.dashboard.dismissed.borrow_mut().insert(id.clone());
                    state.save_dashboard_dismissed();
                    state.render_dashboard();
                    let toast = adw::Toast::new("Hidden from dashboard");
                    toast.set_button_label(Some("Undo"));
                    let weak = Rc::downgrade(&state);
                    let id = id.clone();
                    toast.connect_button_clicked(move |_| {
                        if let Some(state) = weak.upgrade() {
                            state.dashboard.dismissed.borrow_mut().remove(&id);
                            state.save_dashboard_dismissed();
                            state.render_dashboard();
                        }
                    });
                    state.toast.add_toast(toast);
                }
            });
        }
    }

    pub(in crate::window) fn dashboard_source_loaded(
        self: &Rc<Self>,
        request: u64,
        result: Result<Option<Box<models::dashboard::Source>>, String>,
    ) {
        if !self.dashboard_visible() || request != self.dashboard.open_request.get() {
            return;
        }
        match result {
            Ok(Some(source)) => {
                self.open_source_message(&source.account, &source.folder, source.message)
            }
            Ok(None) => {
                self.toast.add_toast(adw::Toast::new(
                    "This message is no longer in its original folder",
                ));
                self.refresh_dashboard();
            }
            Err(error) => self
                .toast
                .add_toast(adw::Toast::new(&format!("Could not open email: {error}"))),
        }
    }

    pub(super) fn save_dashboard_dismissed(&self) {
        let result = (|| -> anyhow::Result<()> {
            let path = dismissed_path();
            std::fs::create_dir_all(path.parent().unwrap())?;
            glib::file_set_contents(
                &path,
                &serde_json::to_vec(&*self.dashboard.dismissed.borrow())?,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            self.toast.add_toast(adw::Toast::new(&format!(
                "Could not save dismissed items: {error}"
            )));
        }
    }
}

pub(super) fn dismissed_path() -> std::path::PathBuf {
    glib::user_config_dir().join("brevlada/dashboard-dismissed.json")
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a private graphical session; uses fixture series and a recording worker"]
    fn series_popup_lists_past_dates_and_opens_their_own_email() {
        let (worker, commands) = worker::Worker::recording();
        let state = crate::window::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaDashboardSeriesDiagnostic",
            worker,
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
        let now = chrono::Utc::now();
        let sources = [-7, 2, 9]
            .into_iter()
            .enumerate()
            .map(|(index, days)| {
                let start = (now + chrono::Duration::days(days)).to_rfc3339();
                let event = models::calendar::Event {
                    uid: Some("weekly".into()),
                    recurrence_id: Some(format!("RECURRENCE-ID:{start}")),
                    name: "Český týdenní přehled".into(),
                    location: Some("Břevnov, Praha".into()),
                    start,
                    ..Default::default()
                };
                models::dashboard::Source {
                    account: account.email.clone(),
                    folder: "Calendar".into(),
                    message: Message {
                        uid: index as u32 + 1,
                        message_id: format!("occurrence-{index}"),
                        sender: "host@example.com".into(),
                        calendar_events: vec![event],
                        display_prepared: true,
                        ..Default::default()
                    },
                }
            })
            .collect();
        let entries =
            models::dashboard::entries(sources, now.timestamp(), models::dashboard::Range::Week);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].series.len(), 3);
        state.event(Event::Dashboard(request, Ok(entries)));
        fn find_card(widget: &gtk::Widget) -> Option<gtk::Widget> {
            if widget.has_css_class("dashboard-card") {
                return Some(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(card) = find_card(&widget) {
                    return Some(card);
                }
            }
            None
        }
        let card = find_card(state.dashboard.view.widget.upcast_ref()).unwrap();
        let header = card.first_child().unwrap();
        let dismiss = header.last_child().and_downcast::<gtk::Button>().unwrap();
        assert_eq!(
            dismiss.tooltip_text().as_deref(),
            Some("Dismiss from dashboard")
        );
        let series = dismiss
            .prev_sibling()
            .and_downcast::<gtk::MenuButton>()
            .unwrap();
        let window = state.toast.root().and_downcast::<gtk::Window>().unwrap();
        window.present();
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(200)));
        series.popup();
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(200)));
        let content = series.popover().unwrap().child().unwrap();
        let dates = content
            .last_child()
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap();
        let mut count = 0;
        let mut child = dates.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        assert_eq!(count, 3);
        if std::env::var_os("BREVLADA_DIAGNOSTIC_HOLD").is_some() {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_secs(8)));
        }
        dates
            .first_child()
            .and_downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert!(commands.try_iter().any(|command| matches!(command,
            Command::DashboardOpen { source, .. }
            if source.message.uid == 1 && source.folder == "Calendar" && source.account == account.email)));
        window.close();
    }

    #[test]
    #[ignore = "Requires a private graphical session; uses fixture invitations and a recording worker"]
    fn shared_invitation_actions_use_the_source_account_and_disable_while_pending() {
        let (worker, commands) = worker::Worker::recording();
        let state = crate::window::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaDashboardRsvpDiagnostic",
            worker,
        );
        let current = state.account.borrow().clone().unwrap();
        let source_account = Account {
            email: "work@example.com".into(),
            ..current.clone()
        };
        state.event(Event::Accounts(vec![current, source_account.clone()]));
        state.dashboard_button.emit_clicked();
        let request = commands
            .try_iter()
            .find_map(|command| match command {
                Command::Dashboard { request, .. } => Some(request),
                _ => None,
            })
            .unwrap();
        let event = models::calendar::Event {
            uid: Some("invite".into()),
            organizer_email: Some("host@example.com".into()),
            name: "Workshop".into(),
            start: (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339(),
            attendees: vec![models::calendar::Attendee {
                email: source_account.email.clone(),
                name: None,
                status: "NEEDS-ACTION".into(),
            }],
            invitation: true,
            method: Some("REQUEST".into()),
            ..Default::default()
        };
        let source = models::dashboard::Source {
            account: source_account.email.clone(),
            folder: "INBOX".into(),
            message: Message {
                uid: 1,
                message_id: "invite".into(),
                sender: "host@example.com".into(),
                calendar_events: vec![event.clone()],
                ..Default::default()
            },
        };
        state.event(Event::Dashboard(
            request,
            Ok(vec![Entry {
                recent_emails: 0,
                id: "event".into(),
                source,
                content: models::dashboard::Content::Event(event),
                series: Vec::new(),
            }]),
        ));
        fn yes_button(widget: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(button) = widget.downcast_ref::<gtk::Button>()
                && button
                    .child()
                    .and_then(|child| child.last_child())
                    .and_downcast::<gtk::Label>()
                    .is_some_and(|label| label.text() == "Yes")
            {
                return Some(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(button) = yes_button(&widget) {
                    return Some(button);
                }
            }
            None
        }
        yes_button(state.dashboard.view.widget.upcast_ref())
            .unwrap()
            .emit_clicked();
        assert!(commands.try_iter().any(|command| matches!(command, Command::CalendarReply { account, response: models::calendar::Response::Accepted, .. } if account.email == source_account.email)));
        assert!(
            !yes_button(state.dashboard.view.widget.upcast_ref())
                .unwrap()
                .is_sensitive()
        );
        assert!(state.dashboard_visible());
        state
            .toast
            .root()
            .and_downcast::<gtk::Window>()
            .unwrap()
            .close();
    }
}
