use super::*;
use models::parcel::{self, Parcel};

impl State {
    pub(super) fn parcel_widgets(
        &self,
        messages: &[(bool, String, Message)],
        group: &[Message],
    ) -> Vec<Vec<gtk::Widget>> {
        let mut widgets = vec![Vec::new(); messages.len()];
        let mut latest = HashMap::new();
        for (_, _, message) in messages
            .iter()
            .filter(|(sent, _, message)| !sent && !message.is_draft)
        {
            let sender = models::senders::key(message);
            for parcel in &message.parcels {
                if let Some(id) = parcel::identifier(parcel)
                    && !sender.is_empty()
                {
                    latest
                        .entry((sender.clone(), id))
                        .or_insert((parcel, message));
                }
            }
        }
        for (index, (_, _, message)) in messages
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, (sent, _, message))| !sent && !message.is_draft)
        {
            let sender = models::senders::key(message);
            for parcel in &message.parcels {
                let (parcel, message) = if let Some(id) = parcel::identifier(parcel)
                    && !sender.is_empty()
                {
                    let Some(latest) = latest.remove(&(sender.clone(), id)) else {
                        continue;
                    };
                    latest
                } else {
                    (parcel, message)
                };
                let sequence = self.delivery_sequence(parcel, message, group);
                widgets[index].push(ui::parcel::card(&sequence, message.timestamp).upcast());
            }
        }
        widgets
    }

    pub(super) fn delivery_cache_changed(self: &Rc<Self>, message: &Message) {
        let sender = models::senders::key(message);
        let mut loaded = message.clone();
        if loaded.parcels.is_empty() {
            loaded.parcels = parcel::parse_message(message);
        }
        let mut changed = false;
        for update in &loaded.parcels {
            let Some(id) = parcel::identifier(update) else {
                continue;
            };
            if let Some(history) = self
                .parcel_history
                .borrow_mut()
                .get_mut(&(sender.clone(), id))
            {
                if history.iter().any(|old| old == &loaded) {
                    continue;
                }
                history.retain(|old| old.uid != loaded.uid || old.message_id != loaded.message_id);
                history.push(loaded.clone());
                changed = true;
            }
        }
        if changed {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                self.render_conversation()
            });
        }
    }

    pub(super) fn delivery_sequence(
        &self,
        parcel: &Parcel,
        source: &Message,
        group: &[Message],
    ) -> Parcel {
        let Some(id) = parcel::identifier(parcel) else {
            return parcel.clone();
        };
        let sender = models::senders::key(source);
        if sender.is_empty() {
            return parcel.clone();
        }
        let key = (sender.clone(), id.clone());
        if !self.parcel_history.borrow().contains_key(&key)
            && let Some(account) = self.account.borrow().clone()
        {
            self.parcel_history
                .borrow_mut()
                .insert(key.clone(), Vec::new());
            if let Err(error) = self.sender.send(Command::ParcelHistory {
                account,
                folder: self.folder.borrow().clone(),
                sender,
                identifier: id.clone(),
                generation: self.generation.get(),
                selection: self.selection.get(),
            }) {
                eprintln!("Could not request delivery history: {error}");
            }
        }
        let history = self.parcel_history.borrow();
        let cached = history.get(&key).into_iter().flatten();
        let messages = self.messages.borrow();
        let sender_pane = self.sender_pane.borrow();
        parcel::sequence(
            parcel,
            source,
            cached
                .chain(group.iter())
                .chain(messages.iter())
                .chain(sender_pane.messages().iter()),
        )
    }

    pub(super) fn delivery_history_loaded(
        self: &Rc<Self>,
        sender: String,
        id: parcel::Identifier,
        mut messages: Vec<Message>,
    ) {
        let key = (sender, id);
        // A cache update may arrive while the worker is still reading history.
        // Preserve those updates when an older partial result arrives afterward.
        if let Some(previous) = self.parcel_history.borrow().get(&key) {
            for message in previous {
                if !messages.iter().any(|incoming| {
                    incoming.uid == message.uid && incoming.message_id == message.message_id
                }) {
                    messages.push(message.clone());
                }
            }
        }
        messages.sort_by_key(|message| (message.timestamp, message.uid));
        if self.parcel_history.borrow().get(&key) == Some(&messages) {
            return;
        }
        self.parcel_history.borrow_mut().insert(key, messages);
        ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
            self.render_conversation()
        });
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn message(uid: u32, order: Option<&str>, status: &str) -> Message {
        Message {
            uid,
            message_id: format!("update-{uid}"),
            timestamp: uid as i64,
            sender: "Shop <shop@example.com>".into(),
            subject: format!("Update {uid}"),
            is_read: true,
            body_loaded: true,
            body_html: format!(
                "<script type='application/ld+json'>{}</script>",
                serde_json::json!({
                    "@type":"ParcelDelivery", "partOfOrder":{"orderNumber": order}, "deliveryStatus":status
                })
            ),
            ..Default::default()
        }
    }

    fn statuses(widget: &gtk::Widget) -> Vec<String> {
        let mut result = Vec::new();
        if (widget.has_css_class("parcel-step-name")
            || widget.has_css_class("parcel-single-step-name"))
            && let Some(label) = widget.downcast_ref::<gtk::Label>()
        {
            result.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            result.extend(statuses(&widget));
            child = widget.next_sibling();
        }
        result
    }

    #[test]
    #[ignore = "Requires a graphical session for the delivery card"]
    fn requests_history_only_with_an_id_and_refreshes_sequence_from_other_emails() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaDeliveryDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            ui::shell::Shell::new(&app),
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "me@example.com".into(),
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
        let missing = message(1, None, "Sent");
        let parcel = parcel::parse_message(&missing).remove(0);
        assert_eq!(state.delivery_sequence(&parcel, &missing, &[]), parcel);
        assert!(commands.try_recv().is_err());

        state.new_selection();
        let shipped = Message {
            uid: 10,
            message_id: "shipped".into(),
            timestamp: 10,
            sender: "UGREEN <service.eu@ugreen.com>".into(),
            subject: "Your order KVM Switch has already been shipped.".into(),
            body_html: "<div>Tracking number: CC996635912DE</div>".into(),
            body_loaded: true,
            is_read: true,
            ..Default::default()
        };
        let delivered = Message {
            uid: 11,
            message_id: "delivered".into(),
            timestamp: 11,
            subject: "Your order KVM Switch has been delivered.".into(),
            ..shipped.clone()
        };
        *state.messages.borrow_mut() = vec![delivered.clone(), shipped.clone()];
        *state.open_group.borrow_mut() = vec![delivered, shipped];
        state.render_conversation();
        assert_eq!(
            statuses(state.viewer.upcast_ref()),
            ["In transit", "Delivered"]
        );
        let mut cards = 0;
        let mut child = state.viewer.first_child();
        while let Some(widget) = child {
            if widget.has_css_class("parcel-card") {
                cards += 1;
            }
            child = widget.next_sibling();
        }
        assert_eq!(cards, 1);
        match commands.try_recv().unwrap() {
            Command::ParcelHistory { identifier, .. } => {
                assert_eq!(
                    identifier,
                    parcel::Identifier::Tracking("CC996635912DE".into())
                );
            }
            _ => panic!("Expected a tracking number lookup"),
        }
        assert!(commands.try_recv().is_err());
        let source = message(2, Some("123"), "In transit");
        *state.messages.borrow_mut() = vec![source.clone()];
        *state.open_group.borrow_mut() = vec![source.clone()];
        state.render_conversation();
        match commands.try_recv().unwrap() {
            Command::ParcelHistory {
                account,
                folder,
                sender,
                identifier,
                ..
            } => {
                assert_eq!(account.email, "me@example.com");
                assert_eq!(folder, "INBOX");
                assert_eq!(sender, "shop@example.com");
                assert_eq!(identifier, parcel::Identifier::Order("123".into()));
            }
            _ => panic!("Expected delivery lookup"),
        }
        assert_eq!(statuses(state.viewer.upcast_ref()), ["In transit"]);
        state.event(Event::ParcelHistory(
            state.generation.get(),
            state.selection.get() + 1,
            "shop@example.com".into(),
            parcel::Identifier::Order("123".into()),
            vec![message(3, Some("123"), "Delivered")],
        ));
        assert_eq!(statuses(state.viewer.upcast_ref()), ["In transit"]);
        state.event(Event::ParcelHistory(
            state.generation.get(),
            state.selection.get(),
            "shop@example.com".into(),
            parcel::Identifier::Order("123".into()),
            vec![message(1, Some("123"), "Sent")],
        ));
        assert_eq!(statuses(state.viewer.upcast_ref()), ["Sent", "In transit"]);
        state.event(Event::CacheBody(
            "me@example.com".into(),
            "INBOX".into(),
            message(3, Some("123"), "Delivered"),
        ));
        assert_eq!(
            statuses(state.viewer.upcast_ref()),
            ["Sent", "In transit", "Delivered"]
        );
        state.event(Event::ParcelHistory(
            state.generation.get(),
            state.selection.get(),
            "shop@example.com".into(),
            parcel::Identifier::Order("123".into()),
            vec![message(1, Some("123"), "Sent")],
        ));
        assert_eq!(
            statuses(state.viewer.upcast_ref()),
            ["Sent", "In transit", "Delivered"]
        );
        assert!(commands.try_recv().is_err());
    }
}
