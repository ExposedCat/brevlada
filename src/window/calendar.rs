use super::*;
use models::calendar::{self, Response};

pub(super) struct CalendarWidgets {
    pub before: Vec<Vec<gtk::Widget>>,
    pub compact: HashSet<usize>,
}

impl State {
    pub(super) fn calendar_widgets(
        self: &Rc<Self>,
        messages: &[(bool, String, Message)],
    ) -> CalendarWidgets {
        let mut widgets = CalendarWidgets {
            before: vec![Vec::new(); messages.len()],
            compact: HashSet::new(),
        };
        let Some(account) = self.account.borrow().clone() else {
            return widgets;
        };
        let mut events = Vec::new();
        let mut invitations = Vec::new();
        let mut sources: Vec<_> = messages
            .iter()
            .enumerate()
            .filter(|(_, (_, _, message))| !message.is_draft)
            .flat_map(|(index, (_, _, message))| {
                calendar::events(message)
                    .into_iter()
                    .map(move |source| (index, message, source))
            })
            .collect();
        sources.sort_by_key(|(index, _, source)| {
            (
                matches!(source.method.as_deref(), Some("REPLY" | "CANCEL")),
                std::cmp::Reverse(*index),
            )
        });
        for (index, message, source) in sources {
            if !source.invitation
                && message.tickets.iter().any(|reservation| {
                    reservation.kind == models::ticket::Kind::Event
                        && reservation
                            .calendar_event()
                            .is_some_and(|event| event.same_event(&source))
                })
            {
                continue;
            }
            if events
                .iter()
                .any(|old: &calendar::Event| old.same_event(&source))
            {
                continue;
            }
            if let Some(uid) = &source.uid
                && !self.calendar_history.borrow().contains_key(uid)
            {
                self.calendar_history
                    .borrow_mut()
                    .insert(uid.clone(), Vec::new());
                if let Err(error) = self.sender.send(Command::CalendarHistory {
                    account: account.clone(),
                    uid: uid.clone(),
                    generation: self.generation.get(),
                }) {
                    self.toast.add_toast(adw::Toast::new(&error.to_string()));
                }
            }
            let history = self.calendar_history.borrow();
            let cached = source.uid.as_ref().and_then(|uid| history.get(uid));
            let event = calendar::snapshot(
                &source,
                messages
                    .iter()
                    .map(|(_, _, message)| message)
                    .chain(cached.into_iter().flatten()),
            );
            let key = (
                account.email.clone(),
                event.uid.clone().unwrap_or_default(),
                event.recurrence_id.clone(),
            );
            let pending = self.calendar_pending.borrow().contains(&key);
            let state = Rc::downgrade(self);
            let original = message.clone();
            let responding = event.clone();
            let from = account.clone();
            let card = ui::calendar::card(&event, &account.email, pending, move |response| {
                if let Some(state) = state.upgrade() {
                    state.calendar_reply(
                        from.clone(),
                        original.clone(),
                        responding.clone(),
                        response,
                    );
                }
            });
            if !matches!(source.method.as_deref(), Some("REPLY" | "CANCEL"))
                && !event.attendees.is_empty()
            {
                let people = calendar::participants(
                    &event,
                    messages
                        .iter()
                        .map(|(_, _, message)| message)
                        .chain(cached.into_iter().flatten()),
                );
                let participants = ui::calendar::participants(&people, &self.avatars).upcast();
                widgets.before[index]
                    .push(ui::calendar::with_participants(&card, &participants).upcast());
                invitations.push(event.clone());
            } else {
                widgets.before[index].push(card.upcast());
            }
            events.push(event);
        }
        widgets.compact = messages
            .iter()
            .enumerate()
            .filter(|(_, (_, _, message))| calendar::compact_reply(message, &invitations))
            .map(|(index, _)| index)
            .collect();
        widgets
    }

    fn calendar_reply(
        self: &Rc<Self>,
        account: Account,
        message: Message,
        event: calendar::Event,
        response: Response,
    ) {
        let key = (
            account.email.clone(),
            event.uid.clone().unwrap_or_default(),
            event.recurrence_id.clone(),
        );
        if !self.calendar_pending.borrow_mut().insert(key.clone()) {
            return;
        }
        if let Err(error) = self.sender.send(Command::CalendarReply {
            account,
            message: Box::new(message),
            event: Box::new(event),
            response,
        }) {
            self.calendar_pending.borrow_mut().remove(&key);
            self.toast
                .add_toast(adw::Toast::new(&format!("Could not send RSVP: {error}")));
        }
        ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
            self.render_conversation()
        });
    }

    pub(super) fn calendar_history_loaded(
        self: &Rc<Self>,
        uid: String,
        result: Result<Vec<Message>, String>,
    ) {
        let mut messages = match result {
            Ok(messages) => messages,
            Err(error) => {
                self.calendar_history.borrow_mut().remove(&uid);
                self.toast.add_toast(adw::Toast::new(&format!(
                    "Could not load RSVP history: {error}"
                )));
                return;
            }
        };
        if let Some(previous) = self.calendar_history.borrow().get(&uid) {
            for message in previous {
                if !messages
                    .iter()
                    .any(|old| old.message_id == message.message_id)
                {
                    messages.push(message.clone());
                }
            }
        }
        messages.sort_by(|left, right| {
            (&left.message_id, left.uid).cmp(&(&right.message_id, right.uid))
        });
        if self.calendar_history.borrow().get(&uid) == Some(&messages) {
            return;
        }
        let visible = self.calendar_visible(&uid);
        self.calendar_history.borrow_mut().insert(uid, messages);
        if visible {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                self.render_conversation()
            });
        }
    }

    fn calendar_visible(&self, uid: &str) -> bool {
        self.open_group
            .borrow()
            .iter()
            .chain(self.related_sent.borrow().iter().map(|sent| &sent.message))
            .any(|message| {
                message
                    .calendar_events
                    .iter()
                    .any(|event| event.uid.as_deref() == Some(uid))
            })
    }

    pub(super) fn calendar_cache_changed(self: &Rc<Self>, message: &Message) {
        let mut changed = false;
        for event in calendar::events(message) {
            if let Some(uid) = event.uid
                && let Some(history) = self.calendar_history.borrow_mut().get_mut(&uid)
            {
                if history.iter().any(|old| old == message) {
                    continue;
                }
                history.retain(|old| old.message_id != message.message_id);
                history.push(message.clone());
                changed = true;
            }
        }
        if changed {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                self.render_conversation()
            });
        }
    }
}
