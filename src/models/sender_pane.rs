use super::Message;

#[derive(Default)]
pub struct SenderPane {
    revision: u64,
    session: Option<Session>,
}

struct Session {
    sender: String,
    messages: Vec<Message>,
    cursor: Option<(i64, u32)>,
    paging: Paging,
}

enum Paging {
    Ready,
    Loading(u64),
    Exhausted,
    Failed,
}

pub struct PageRequest {
    pub sender: String,
    pub before: Option<(i64, u32)>,
    pub ticket: u64,
}

impl SenderPane {
    pub fn sender(&self) -> Option<&str> {
        self.session.as_ref().map(|session| session.sender.as_str())
    }

    pub fn messages(&self) -> &[Message] {
        self.session
            .as_ref()
            .map_or(&[], |session| &session.messages)
    }

    pub fn messages_mut(&mut self) -> &mut [Message] {
        self.session
            .as_mut()
            .map_or(&mut [], |session| &mut session.messages)
    }

    pub fn include<'a>(&mut self, messages: impl IntoIterator<Item = &'a Message>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let mut known: std::collections::HashSet<_> = session
            .messages
            .iter()
            .map(|message| (message.uid, message.message_id.clone()))
            .collect();
        session.messages.extend(
            messages
                .into_iter()
                .filter(|message| super::senders::key(message) == session.sender)
                .filter(|message| known.insert((message.uid, message.message_id.clone())))
                .cloned(),
        );
    }

    pub fn include_newer(&mut self, messages: &[Message]) {
        let Some(cursor) = self.session.as_ref().and_then(|session| session.cursor) else {
            return;
        };
        self.include(
            messages
                .iter()
                .filter(|message| (message.timestamp, message.uid) >= cursor),
        );
    }

    pub fn replace(&mut self, original: &Message, replacement: &Message) {
        for message in self.messages_mut() {
            if message.uid == original.uid && message.message_id == original.message_id {
                *message = replacement.clone();
            }
        }
    }

    pub fn remove(&mut self, removed: &[Message]) {
        if let Some(session) = self.session.as_mut() {
            session.messages.retain(|message| {
                !removed.iter().any(|old| {
                    old.uid == message.uid
                        && (message.uid_validity.is_none()
                            || old.uid_validity == message.uid_validity)
                        && old.message_id == message.message_id
                })
            });
        }
    }

    pub fn select(&mut self, sender: Option<String>) -> bool {
        if self.sender() == sender.as_deref() {
            return false;
        }
        self.revision += 1;
        self.session = sender.map(|sender| Session {
            sender,
            messages: Vec::new(),
            cursor: None,
            paging: Paging::Ready,
        });
        true
    }

    pub fn loading(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| matches!(session.paging, Paging::Loading(_)))
    }

    pub fn can_load(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| matches!(session.paging, Paging::Ready | Paging::Failed))
    }

    pub fn request(&mut self) -> Option<PageRequest> {
        if !self.can_load() {
            return None;
        }
        self.revision += 1;
        let session = self.session.as_mut().unwrap();
        session.paging = Paging::Loading(self.revision);
        Some(PageRequest {
            sender: session.sender.clone(),
            before: session.cursor,
            ticket: self.revision,
        })
    }

    pub fn accepts(&self, ticket: u64) -> bool {
        self.session.as_ref().is_some_and(
            |session| matches!(session.paging, Paging::Loading(current) if current == ticket),
        )
    }

    pub fn finish(&mut self, ticket: u64, messages: Vec<Message>, has_more: bool) -> bool {
        if !self.accepts(ticket) {
            return false;
        }
        let session = self.session.as_mut().unwrap();
        let cursor = messages
            .last()
            .map(|message| (message.timestamp, message.uid));
        let advances =
            cursor.is_some_and(|cursor| session.cursor.is_none_or(|before| cursor < before));
        session.paging = if has_more && advances {
            Paging::Ready
        } else {
            Paging::Exhausted
        };
        if advances {
            session.cursor = cursor;
        }
        let mut known: std::collections::HashMap<_, _> = session
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| ((message.uid, message.message_id.clone()), index))
            .collect();
        for message in messages {
            let key = (message.uid, message.message_id.clone());
            if let Some(&index) = known.get(&key) {
                if !session.messages[index].body_loaded {
                    session.messages[index] = message;
                }
            } else {
                known.insert(key, session.messages.len());
                session.messages.push(message);
            }
        }
        true
    }

    pub fn fail(&mut self, ticket: u64) -> bool {
        if !self.accepts(ticket) {
            return false;
        }
        self.session.as_mut().unwrap().paging = Paging::Failed;
        true
    }

    #[cfg(test)]
    pub fn ticket(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub fn seed(&mut self, messages: Vec<Message>) {
        self.session.as_mut().unwrap().messages = messages;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_and_closed_sessions_reject_late_pages_and_errors() {
        let mut pane = SenderPane::default();
        pane.select(Some("a@example.com".into()));
        let a = pane.request().unwrap();
        pane.select(Some("b@example.com".into()));
        let b = pane.request().unwrap();
        assert!(!pane.finish(a.ticket, vec![Message::default()], true));
        assert!(!pane.fail(a.ticket));
        assert_eq!(pane.sender(), Some("b@example.com"));
        assert!(pane.loading());
        pane.select(None);
        assert!(!pane.finish(b.ticket, vec![Message::default()], true));
        assert!(!pane.loading());
        assert!(!pane.can_load());
        assert!(pane.messages().is_empty());
    }

    #[test]
    fn reopening_active_sender_keeps_the_session_and_each_page_is_consumed_once() {
        let mut pane = SenderPane::default();
        pane.select(Some("a@example.com".into()));
        let first = pane.request().unwrap();
        assert!(!pane.select(Some(first.sender.clone())));
        assert!(pane.request().is_none());
        let message = Message {
            uid: 10,
            timestamp: 10,
            ..Default::default()
        };
        assert!(pane.finish(first.ticket, vec![message.clone()], true));
        let second = pane.request().unwrap();
        assert_eq!(second.before, Some((10, 10)));
        assert!(!pane.finish(first.ticket, vec![message], false));
        assert!(pane.loading());
        assert!(pane.fail(second.ticket));
        let retry = pane.request().unwrap();
        assert_eq!(retry.before, second.before);
        assert!(pane.finish(retry.ticket, Vec::new(), true));
        assert!(!pane.can_load());
    }

    #[test]
    fn arrivals_join_loaded_history_without_changing_its_cursor_or_reopening_paging() {
        let mut pane = SenderPane::default();
        pane.select(Some("a@example.com".into()));
        let first = pane.request().unwrap();
        let older = Message {
            uid: 1,
            timestamp: 1,
            message_id: "older".into(),
            sender: first.sender.clone(),
            ..Message::default()
        };
        pane.finish(first.ticket, vec![older.clone()], true);
        let newest = Message {
            uid: 2,
            timestamp: 2,
            message_id: "newest".into(),
            body_loaded: true,
            body_html: "<p>Full body</p>".into(),
            ..older.clone()
        };
        pane.include(&[
            newest.clone(),
            newest.clone(),
            Message {
                sender: "b@example.com".into(),
                ..older.clone()
            },
        ]);
        pane.include_newer(&[Message {
            uid: 0,
            timestamp: 0,
            message_id: "outside-page".into(),
            ..older.clone()
        }]);
        assert_eq!(pane.messages().len(), 2);
        let next = pane.request().unwrap();
        assert_eq!(next.before, Some((1, 1)));
        pane.include(std::slice::from_ref(&newest));
        assert!(pane.accepts(next.ticket));
        pane.finish(
            next.ticket,
            vec![Message {
                body_loaded: true,
                body_html: "<p>Older body</p>".into(),
                ..older
            }],
            false,
        );
        assert!(pane.messages().iter().all(|message| message.body_loaded));
        pane.include(&[newest]);
        assert_eq!(pane.messages().len(), 2);
        assert!(!pane.can_load());
    }
}
