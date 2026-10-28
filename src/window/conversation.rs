use super::*;

impl State {
    pub(super) fn conversation_loaded(self: &Rc<Self>, mut messages: Vec<models::SentMessage>) {
        self.reconcile_drafts(&mut messages);
        if let Some(account) = self.account.borrow().as_ref() {
            messages.retain(|item| {
                !self.sender_actions.borrow().is_removed(
                    &account.email,
                    &item.folder,
                    &item.message,
                )
            });
        }
        for item in &mut messages {
            if !item.message.body_loaded
                && let Some(loaded) = self.related_sent.borrow().iter().find(|old| {
                    old.folder == item.folder
                        && old.message.uid == item.message.uid
                        && old.message.message_id == item.message.message_id
                        && old.message.uid_validity == item.message.uid_validity
                        && old.message.body_loaded
                })
            {
                item.message = loaded.message.clone();
            }
        }
        if *self.related_sent.borrow() != messages || self.cards.borrow().is_empty() {
            let only_drafts_changed = self
                .related_sent
                .borrow()
                .iter()
                .filter(|item| !item.message.is_draft)
                .eq(messages.iter().filter(|item| !item.message.is_draft));
            *self.related_sent.borrow_mut() = messages;
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                if only_drafts_changed {
                    self.update_draft_widgets();
                } else {
                    self.render_conversation();
                }
            });
        }
    }

    pub(super) fn refresh_open_thread(self: &Rc<Self>) {
        let group = ui::virtual_list::selected(&self.thread_list)
            .and_then(|position| ui::virtual_list::item(&self.thread_list, position))
            .filter(|row| {
                row.messages.iter().any(|message| {
                    self.open_group.borrow().iter().any(|opened| {
                        opened.uid == message.uid && opened.message_id == message.message_id
                    })
                })
            })
            .map(|row| row.messages);
        if let Some(group) = group {
            let opened = self.open_group.borrow();
            if group.len() == opened.len()
                && group.iter().all(|message| {
                    opened.iter().any(|old| {
                        old.uid == message.uid
                            && old.message_id == message.message_id
                            && old.uid_validity == message.uid_validity
                    })
                })
            {
                return;
            }
            drop(opened);
            self.deferred_read_sort.borrow_mut().extend(
                group
                    .iter()
                    .filter(|message| !message.is_read)
                    .map(|message| message.uid),
            );
            *self.selected.borrow_mut() = group.iter().map(|message| message.uid).collect();
            *self.open_group.borrow_mut() = group;
            self.request_related_sent();
            self.render_conversation();
            self.refresh_open_styles();
        }
    }

    pub(super) fn render_conversation(self: &Rc<Self>) {
        self.refresh_composers();
        // Sort borrowed metadata first. Opening a long conversation must not copy
        // or create widgets for every cached body before the first card appears.
        let (mut messages, total) = {
            let folder = self.messages.borrow();
            let pane = self.sender_pane.borrow();
            let opened = self.open_group.borrow();
            let sent = self.related_sent.borrow();
            let mut by_id: HashMap<_, _> = folder
                .iter()
                .map(|message| ((message.uid, message.message_id.as_str()), message))
                .collect();
            for message in pane.messages() {
                let slot = by_id
                    .entry((message.uid, message.message_id.as_str()))
                    .or_insert(message);
                if message.body_loaded {
                    *slot = message;
                }
            }
            let mut sources: Vec<_> = opened
                .iter()
                .map(|original| {
                    (
                        false,
                        "",
                        by_id
                            .get(&(original.uid, original.message_id.as_str()))
                            .copied()
                            .unwrap_or(original),
                    )
                })
                .chain(
                    sent.iter()
                        .map(|item| (true, item.folder.as_str(), &item.message)),
                )
                .collect();
            sources.retain(|(sent, folder, message)| {
                let folder = if *sent {
                    (*folder).to_owned()
                } else {
                    self.folder.borrow().clone()
                };
                if self.account.borrow().as_ref().is_some_and(|account| {
                    self.sender_actions
                        .borrow()
                        .is_removed(&account.email, &folder, message)
                }) {
                    return false;
                }
                !self
                    .draft_removed
                    .borrow()
                    .contains(&self.draft_key(&folder, &message.message_id))
            });
            sources
                .sort_by_key(|(_, _, message)| std::cmp::Reverse((message.timestamp, message.uid)));
            let mut seen = HashSet::new();
            sources.retain(|(_, _, message)| {
                let id = message.message_id.trim().trim_matches(['<', '>']);
                id.is_empty() || seen.insert(id.to_owned())
            });
            let total = sources.len();
            sources.truncate(self.conversation_limit.get());
            let messages: Vec<_> = sources
                .into_iter()
                .map(|(sent, folder, message)| (sent, folder.to_owned(), message.clone()))
                .collect();
            (messages, total)
        };
        for (_, _, message) in &mut messages {
            message.prepare_display();
        }
        let group: Vec<_> = messages
            .iter()
            .filter(|(sent, _, _)| !sent)
            .map(|(_, _, message)| message.clone())
            .collect();
        let previous = self.cards.borrow().clone();
        let previous_sent = self.sent_cards.borrow().clone();
        let mut widgets: Vec<gtk::Widget> = Vec::new();
        self.cards.borrow_mut().clear();
        self.sent_cards.borrow_mut().clear();
        self.viewer.set_vexpand(false);
        let mut parcels = self.parcel_widgets(&messages, &group);
        let mut tickets = self.ticket_widgets(&messages);
        let mut events = self.calendar_widgets(&messages);
        let threaded = messages
            .iter()
            .enumerate()
            .filter(|(index, (_, _, message))| !message.is_draft && !events.compact.contains(index))
            .count()
            > 1;
        let first_message = messages
            .iter()
            .enumerate()
            .position(|(index, (_, _, message))| {
                !message.is_draft && !events.compact.contains(&index)
            });
        let unread = messages
            .iter()
            .enumerate()
            .any(|(index, (sent, _, message))| {
                !sent && !message.is_draft && !message.is_read && !events.compact.contains(&index)
            });
        for (index, (sent, folder, message)) in messages.iter().enumerate() {
            if events.compact.contains(&index) {
                if !sent && !message.is_read {
                    self.open(message.uid);
                }
                continue;
            }
            if message.is_draft {
                let folder = if *sent {
                    folder.clone()
                } else {
                    self.folder.borrow().clone()
                };
                if self.draft_in_composer(&folder, &message.message_id) {
                    continue;
                }
                let editor = self.draft_editor(&folder, message);
                if editor.compose.is_pinned() {
                    continue;
                }
                widgets.push(editor.widget.clone().upcast());
                if !message.body_loaded && !editor.loaded.get() {
                    if *sent {
                        self.open_sent(&folder, message.uid, &message.message_id, false);
                    } else {
                        self.open(message.uid);
                    }
                }
                continue;
            }
            widgets.append(&mut parcels[index]);
            widgets.append(&mut tickets[index]);
            widgets.append(&mut events.before[index]);
            let hide_quotes = models::conversation::quoted_message_visible(
                message,
                messages[index + 1..]
                    .iter()
                    .enumerate()
                    .filter(|(offset, _)| !events.compact.contains(&(index + 1 + offset)))
                    .map(|(_, (_, _, older))| older)
                    .filter(|older| !older.is_draft),
            );
            let expanded = if *sent {
                previous_sent
                    .get(&message.message_id)
                    .map(|card| card.is_expanded())
            } else {
                previous.get(&message.uid).map(|card| card.is_expanded())
            }
            .unwrap_or((!*sent && !message.is_read) || (!unread && Some(index) == first_message));
            let previous = if *sent {
                previous_sent.get(&message.message_id)
            } else {
                previous.get(&message.uid)
            };
            let card = if let Some(card) =
                previous.filter(|card| card.layout_matches(message, threaded, hide_quotes))
            {
                card.update(message);
                card.clone()
            } else {
                self.card(
                    message,
                    expanded,
                    if *sent { Some(folder) } else { None },
                    threaded,
                    hide_quotes,
                )
            };
            widgets.push(card.widget.clone().upcast());
            if *sent {
                self.sent_cards
                    .borrow_mut()
                    .insert(message.message_id.clone(), card);
                if expanded && !message.body_loaded {
                    self.open_sent(folder, message.uid, &message.message_id, false);
                }
            } else {
                self.cards.borrow_mut().insert(message.uid, card);
            }
            if expanded && !sent {
                self.open(message.uid);
            }
        }
        if total > messages.len() {
            self.conversation_more
                .set_label(&format!("Load older messages ({})", total - messages.len()));
            widgets.push(self.conversation_more.clone().upcast());
        }
        let mut child = self.viewer.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if !widgets.contains(&widget) {
                self.viewer.remove(&widget);
            }
        }
        let mut previous: Option<gtk::Widget> = None;
        for widget in widgets {
            if widget.parent().is_some() {
                self.viewer.reorder_child_after(&widget, previous.as_ref());
            } else {
                self.viewer.insert_child_after(&widget, previous.as_ref());
            }
            previous = Some(widget);
        }
        self.load_expanded();
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::{calendar, parcel};

    fn message(uid: u32, sender: &str) -> Message {
        Message {
            uid,
            timestamp: uid as i64,
            message_id: format!("message-{uid}"),
            subject: format!("Message {uid}"),
            sender: sender.into(),
            is_read: true,
            body_loaded: true,
            ..Default::default()
        }
    }

    fn parcel(order: &str, status: &str) -> parcel::Parcel {
        parcel::parse(&models::html::Html::new(&format!("<script type='application/ld+json'>{}</script>", serde_json::json!({
            "@type": "ParcelDelivery", "partOfOrder": {"orderNumber": order}, "deliveryStatus": status
        })))).remove(0)
    }

    fn contains_label(widget: &gtk::Widget, text: &str) -> bool {
        if widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.text() == text)
        {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if contains_label(&widget, text) {
                return true;
            }
            child = widget.next_sibling();
        }
        false
    }

    fn branch_status(responses: &gtk::Widget) -> String {
        responses
            .first_child()
            .unwrap()
            .downcast::<gtk::Grid>()
            .unwrap()
            .child_at(2, 0)
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .text()
            .to_string()
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn sent_conversation_shows_me_once_and_received_replies_without_duplicate_copies() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSentRepliesDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let state = State::new(
            ui::shell::Shell::new(&app),
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "me@example.com".into(),
            path: String::new(),
            name: "Me".into(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.folder.borrow_mut() = "Sent".into();
        let mut sent = message(1, "\"me@example.com\" <me@example.com>");
        sent.subject = "Plans".into();
        sent.recipients = "Alice <alice@example.com>".into();
        sent.body_text = "Let's meet tomorrow.".into();
        let mut reply = message(2, "Alice <alice@example.com>");
        reply.subject = "Re: Plans".into();
        reply.recipients = "me@example.com".into();
        reply.references = vec![sent.message_id.clone()];
        reply.body_text = "Tomorrow works for me.".into();
        *state.messages.borrow_mut() = vec![sent.clone()];
        *state.open_group.borrow_mut() = vec![sent.clone()];
        *state.related_sent.borrow_mut() = vec![
            models::SentMessage {
                folder: "INBOX".into(),
                message: reply,
            },
            models::SentMessage {
                folder: "INBOX".into(),
                message: sent,
            },
        ];
        state.render_conversation();
        assert_eq!(state.cards.borrow().len(), 1);
        assert_eq!(state.sent_cards.borrow().len(), 1);
        assert!(contains_label(state.viewer.upcast_ref(), "Me"));
        assert!(contains_label(state.viewer.upcast_ref(), "Alice"));
        assert!(!contains_label(state.viewer.upcast_ref(), "me@example.com"));
        assert!(!contains_label(
            state.viewer.upcast_ref(),
            "To: Alice <alice@example.com>"
        ));
        let window = state
            .viewer
            .root()
            .unwrap()
            .downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.present();
        if std::env::var_os("BREVLADA_DIAGNOSTIC_HOLD").is_some() {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_secs(15)));
        }
        window.destroy();
        ui::clear(&state.viewer);
        state.cards.borrow_mut().clear();
        state.sent_cards.borrow_mut().clear();
        ui::body_renderer::clear();
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(300)));
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn places_context_cards_above_sources_and_keeps_updates_on_the_original() {
        gtk::init().unwrap();
        adw::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaContextDiagnostic")
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
        let event = calendar::Event {
            uid: Some("meeting".into()),
            name: "Meeting".into(),
            start: "2026-10-02T12:00:00Z".into(),
            method: Some("REQUEST".into()),
            stamp: 20,
            invitation: true,
            organizer_email: Some("host@example.com".into()),
            attendees: vec![calendar::Attendee {
                email: "me@example.com".into(),
                name: None,
                status: "NEEDS-ACTION".into(),
            }],
            ..Default::default()
        };
        let mut original_parcel = message(10, "shop@example.com");
        original_parcel.parcels = vec![parcel("123", "Sent"), parcel("456", "Processing")];
        let mut original_event = message(20, "host@example.com");
        original_event.body_loaded = false;
        original_event.calendar_events = vec![
            event.clone(),
            calendar::Event {
                uid: Some("another-meeting".into()),
                name: "Another meeting".into(),
                ..event.clone()
            },
        ];
        let mut update = message(30, "shop@example.com");
        update.parcels = vec![parcel("123", "Delivered")];
        update.parcels[0].tracking_url = Some("https://example.com/track".into());
        let mut reply = message(40, "me@example.com");
        reply.body_loaded = false;
        reply.calendar_events = vec![calendar::Event {
            method: Some("REPLY".into()),
            stamp: 21,
            invitation: false,
            attendees: vec![calendar::Attendee {
                email: "me@example.com".into(),
                name: None,
                status: "ACCEPTED".into(),
            }],
            ..event.clone()
        }];
        let mut sent_invitation = message(15, "me@example.com");
        sent_invitation.calendar_events = vec![calendar::Event {
            uid: Some("sent-meeting".into()),
            name: "Sent meeting".into(),
            ..event
        }];
        let group = vec![
            message(50, "host@example.com"),
            update,
            original_event,
            original_parcel,
        ];
        *state.messages.borrow_mut() = group.clone();
        *state.open_group.borrow_mut() = group;
        *state.related_sent.borrow_mut() = vec![
            models::SentMessage {
                folder: "Sent".into(),
                message: reply,
            },
            models::SentMessage {
                folder: "Sent".into(),
                message: sent_invitation,
            },
        ];
        state.render_conversation();
        let cards = state.cards.borrow();
        let sent = state.sent_cards.borrow();
        assert_eq!(
            state.viewer.first_child().as_ref(),
            Some(cards[&50].widget.upcast_ref())
        );
        assert!(!sent.contains_key("message-40"));
        assert_eq!(
            cards[&30].widget.prev_sibling().as_ref(),
            Some(cards[&50].widget.upcast_ref())
        );
        let second_event = cards[&20].widget.prev_sibling().unwrap();
        let first_event = second_event.prev_sibling().unwrap();
        assert!(first_event.has_css_class("calendar-event"));
        assert!(second_event.has_css_class("calendar-event"));
        assert!(
            first_event
                .first_child()
                .unwrap()
                .has_css_class("calendar-card")
        );
        assert!(
            second_event
                .first_child()
                .unwrap()
                .has_css_class("calendar-card")
        );
        assert_eq!(
            first_event.prev_sibling().as_ref(),
            Some(cards[&30].widget.upcast_ref())
        );
        let responses = first_event.last_child().unwrap();
        assert!(responses.has_css_class("calendar-participants"));
        assert!(branch_status(&responses).starts_with("Accepted · "));
        let second_responses = second_event.last_child().unwrap();
        assert!(second_responses.has_css_class("calendar-participants"));
        let sent_event = sent["message-15"].widget.prev_sibling().unwrap();
        assert_eq!(
            sent_event.prev_sibling().as_ref(),
            Some(cards[&20].widget.upcast_ref())
        );
        assert!(sent_event.has_css_class("calendar-event"));
        let second_parcel = cards[&10].widget.prev_sibling().unwrap();
        let first_parcel = second_parcel.prev_sibling().unwrap();
        assert!(first_parcel.has_css_class("parcel-card"));
        assert!(second_parcel.has_css_class("parcel-card"));
        assert!(contains_label(&first_parcel, "Track Package"));
        assert!(contains_label(&first_parcel, "Delivered"));
        assert_eq!(
            first_parcel.prev_sibling().as_ref(),
            Some(sent["message-15"].widget.upcast_ref())
        );
        let messages: Vec<_> = state
            .messages
            .borrow()
            .iter()
            .cloned()
            .map(|message| (false, String::new(), message))
            .collect();
        let widgets = state.calendar_widgets(&messages);
        assert_eq!(
            widgets.before.iter().map(Vec::len).collect::<Vec<_>>(),
            [0, 0, 2, 0]
        );
        drop(cards);
        drop(sent);
        let mut updated_reply = state.related_sent.borrow()[0].message.clone();
        updated_reply.uid = 60;
        updated_reply.timestamp = 60;
        updated_reply.message_id = "reply-update".into();
        updated_reply.calendar_events[0].stamp = 22;
        updated_reply.calendar_events[0].attendees[0].status = "TENTATIVE".into();
        state.messages.borrow_mut().push(updated_reply.clone());
        state.open_group.borrow_mut().push(updated_reply);
        state.render_conversation();
        assert!(!state.cards.borrow().contains_key(&60));
        let responses = state.cards.borrow()[&20]
            .widget
            .prev_sibling()
            .unwrap()
            .prev_sibling()
            .unwrap()
            .last_child()
            .unwrap();
        let status = branch_status(&responses);
        assert!(status.starts_with("Maybe · "));
        let branch = responses
            .first_child()
            .unwrap()
            .downcast::<gtk::Grid>()
            .unwrap();
        assert!(branch.child_at(2, 0).unwrap().is::<gtk::Box>());

        let mut source = state
            .messages
            .borrow()
            .iter()
            .find(|message| message.uid == 20)
            .unwrap()
            .clone();
        source.calendar_events.truncate(1);
        let mut reply = state
            .messages
            .borrow()
            .iter()
            .find(|message| message.uid == 60)
            .unwrap()
            .clone();
        *state.messages.borrow_mut() = vec![reply.clone(), source.clone()];
        state.show_thread(vec![reply.clone(), source.clone()]);
        assert_eq!(state.cards.borrow().len(), 1);
        let card = state.cards.borrow()[&20].clone();
        assert!(card.is_expanded());
        reply.body_loaded = true;
        reply.body_text = "Maybe: Meeting".into();
        state.update_body(&reply);
        assert_eq!(state.cards.borrow()[&20].widget, card.widget);
        assert!(state.cards.borrow()[&20].is_expanded());
        assert!(!state.cards.borrow().contains_key(&60));
        state.event(Event::CalendarHistory(
            state.generation.get(),
            "meeting".into(),
            Ok(vec![source.clone(), reply]),
        ));
        assert_eq!(state.cards.borrow()[&20].widget, card.widget);
        assert!(state.cards.borrow()[&20].is_expanded());
        while commands.try_recv().is_ok() {}
        state.show_thread(vec![source.clone()]);
        let event = state.viewer.first_child().unwrap();
        assert!(branch_status(&event.last_child().unwrap()).starts_with("Maybe · "));
        assert!(
            !commands
                .try_iter()
                .any(|command| matches!(command, Command::CalendarHistory { .. }))
        );

        // Background body updates must retain classification before opening
        // a thread, even when the row was previously just a lightweight header.
        let mut cached_reply = message(70, "me@example.com");
        cached_reply.body_text = "Accepted: Meeting".into();
        cached_reply.calendar_events = vec![calendar::Event {
            method: Some("REPLY".into()),
            ..source.calendar_events[0].clone()
        }];
        let mut header = cached_reply.clone();
        header.body_loaded = false;
        header.body_text.clear();
        header.calendar_events.clear();
        *state.messages.borrow_mut() = vec![header];
        state.apply_cached_body(cached_reply);
        let header = state.messages.borrow()[0].clone();
        assert!(!header.body_loaded);
        assert!(calendar::is_reply(&header));
        state.show_thread(vec![header, source]);
        assert!(!state.cards.borrow().contains_key(&70));
        assert_eq!(state.cards.borrow().len(), 1);
    }
}
