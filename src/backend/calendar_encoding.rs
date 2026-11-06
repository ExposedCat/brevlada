use crate::models::calendar::{Event, encoding};
use anyhow::Result;
use rusqlite::{Transaction, params};

pub(super) fn migrate(transaction: &Transaction<'_>) -> Result<()> {
    if transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM rust_metadata WHERE key='calendar_utf8')",
        [],
        |row| row.get::<_, bool>(0),
    )? {
        return Ok(());
    }
    let mut statement = transaction
        .prepare("SELECT id,calendar_events FROM rust_headers WHERE calendar_events<>'[]'")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let mut events: Vec<Event> = serde_json::from_str(row.get_ref(1)?.as_str()?)?;
        let mut changed = false;
        for event in &mut events {
            changed |= encoding::repair(event);
        }
        if changed {
            transaction.execute(
                "UPDATE rust_headers SET calendar_events=?2 WHERE id=?1",
                params![id, serde_json::to_string(&events)?],
            )?;
        }
    }
    transaction.execute("INSERT INTO rust_metadata VALUES ('calendar_utf8','1')", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend::storage::Storage, models::Message};

    #[test]
    fn repairs_cached_events_once_without_changing_mail_or_identifiers() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 7,
            message_id: "cached".into(),
            sender: "Sender <sender@example.com>".into(),
            body_loaded: true,
            body_html: "<p>Český text</p>".into(),
            calendar_events: vec![Event {
                name: "DoruÄ\u{8d}ovÃ¡nÃ\u{ad}".into(),
                uid: Some("opaque-Ã©".into()),
                start: "2026-11-05".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        storage.store("one", "INBOX", &message).unwrap();
        storage
            .0
            .execute("DELETE FROM rust_metadata WHERE key='calendar_utf8'", [])
            .unwrap();
        let storage = Storage::initialize(storage.0).unwrap();
        let mut expected = message;
        expected.calendar_events[0].name = "Doručování".into();
        assert_eq!(
            storage.message("one", "INBOX", 7).unwrap(),
            Some(expected.clone())
        );
        let storage = Storage::initialize(storage.0).unwrap();
        assert_eq!(storage.message("one", "INBOX", 7).unwrap(), Some(expected));
    }
}
