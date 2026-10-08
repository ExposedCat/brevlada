use super::Storage;
use crate::models::Message;
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub fn calendar_messages(&self, account: &str, event_uid: &str) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT data FROM rust_messages WHERE account_id=?1
             AND EXISTS(SELECT 1 FROM json_each(data, '$.calendar_events') AS event WHERE json_extract(event.value, '$.uid')=?2)
             UNION ALL
             SELECT data FROM rust_sent_local WHERE account_id=?1
             AND EXISTS(SELECT 1 FROM json_each(data, '$.calendar_events') AS event WHERE json_extract(event.value, '$.uid')=?2)",
        )?;
        let rows =
            statement.query_map(params![account, event_uid], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::calendar::{self, Event};

    #[test]
    fn history_spans_folders_and_local_replies_without_crossing_accounts() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let invitation = Message {
            uid: 1,
            message_id: "invitation".into(),
            sender: "host@example.com".into(),
            calendar_events: vec![Event {
                uid: Some("one".into()),
                organizer_email: Some("host@example.com".into()),
                attendees: vec![calendar::Attendee {
                    email: "me@example.com".into(),
                    name: None,
                    status: "NEEDS-ACTION".into(),
                }],
                sequence: 1,
                method: Some("REQUEST".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut response = invitation.clone();
        response.uid = 2;
        response.message_id = "reply".into();
        response.sender = "me@example.com".into();
        response.calendar_events[0].method = Some("REPLY".into());
        response.calendar_events[0].attendees[0].status = "ACCEPTED".into();
        storage.store("me", "INBOX", &invitation).unwrap();
        storage.store_sent("me", &response).unwrap();
        storage.store("other", "INBOX", &response).unwrap();
        let history = storage.calendar_messages("me", "one").unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            calendar::snapshot(&invitation.calendar_events[0], &history).attendees[0].status,
            "ACCEPTED"
        );
        assert!(storage.calendar_messages("me", "two").unwrap().is_empty());
    }

    #[test]
    fn lightweight_headers_keep_reply_classification_without_loading_bodies() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let invitation = Message {
            uid: 1,
            message_id: "invitation".into(),
            sender: "host@example.com".into(),
            body_loaded: true,
            body_html: "<p>Invitation body</p>".into(),
            calendar_events: vec![crate::models::calendar::Event {
                uid: Some("meeting".into()),
                stamp: 10,
                method: Some("REQUEST".into()),
                organizer_email: Some("host@example.com".into()),
                attendees: vec![crate::models::calendar::Attendee {
                    email: "me@example.com".into(),
                    name: None,
                    status: "NEEDS-ACTION".into(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut reply = invitation.clone();
        reply.uid = 2;
        reply.message_id = "response".into();
        reply.sender = "me@example.com".into();
        reply.calendar_events[0].method = Some("REPLY".into());
        reply.calendar_events[0].stamp = 11;
        reply.calendar_events[0].attendees[0].status = "ACCEPTED".into();
        reply.attachments.push("response.ics".into());
        reply
            .attachment_details
            .push(crate::models::attachment::Details {
                mime: "text/calendar".into(),
                size: 100,
                calendar_rendered: Some(true),
            });
        storage.store("me", "INBOX", &invitation).unwrap();
        storage.store("me", "INBOX", &reply).unwrap();
        let mut mixed = reply.clone();
        mixed.uid = 3;
        mixed.message_id = "response-with-file".into();
        mixed.attachments.push("notes.pdf".into());
        storage.store("me", "INBOX", &mixed).unwrap();
        let headers = storage.sender_headers("me", "INBOX").unwrap();
        assert!(
            headers
                .iter()
                .all(|message| !message.body_loaded && message.body_html.is_empty())
        );
        let source = headers.iter().find(|message| message.uid == 1).unwrap();
        let reply = headers.iter().find(|message| message.uid == 2).unwrap();
        let mixed = headers.iter().find(|message| message.uid == 3).unwrap();
        assert!(crate::models::calendar::compact_reply(
            reply,
            &source.calendar_events
        ));
        assert!(!crate::models::calendar::compact_reply(
            mixed,
            &source.calendar_events
        ));
        let snapshot =
            crate::models::calendar::snapshot(&source.calendar_events[0], [source, reply]);
        assert_eq!(snapshot.attendees[0].status, "ACCEPTED");
        assert_eq!(
            crate::models::calendar::participants(&snapshot, [reply])[0].responded_at,
            Some(11)
        );
    }
}
