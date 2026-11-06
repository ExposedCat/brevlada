use crate::models::{Message, html::Html, parcel, receipt};
use anyhow::Result;
use rusqlite::{Connection, Transaction, params};

pub(super) fn store(connection: &Connection, id: i64, message: &Message) -> Result<()> {
    let html = Html::new(&message.body_html);
    let parcels = parcel::parse_message_in(message, &html);
    let receipts = receipt::parse_in(&html);
    if parcels.is_empty() && receipts.is_empty() {
        connection
            .prepare_cached("DELETE FROM rust_message_items WHERE header_id=?1")?
            .execute([id])?;
    } else {
        connection
            .prepare_cached("INSERT OR REPLACE INTO rust_message_items VALUES (?1,?2,?3)")?
            .execute(params![
                id,
                serde_json::to_string(&parcels)?,
                serde_json::to_string(&receipts)?
            ])?;
    }
    Ok(())
}

pub(super) fn schema(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS rust_message_items (
         header_id INTEGER PRIMARY KEY, parcel_data TEXT NOT NULL, receipt_data TEXT NOT NULL);
         CREATE TRIGGER IF NOT EXISTS rust_message_items_deleted AFTER DELETE ON rust_headers
         BEGIN DELETE FROM rust_message_items WHERE header_id=old.id; END;
         CREATE INDEX IF NOT EXISTS rust_dashboard_headers ON rust_headers(account_id, timestamp DESC, uid DESC)
         WHERE folder IS NOT NULL AND NOT is_draft AND NOT is_spam
         AND (parcels<>'[]' OR calendar_events<>'[]' OR tickets<>'[]');"
    )?;
    Ok(())
}

pub(super) fn migrate(transaction: &Transaction<'_>) -> Result<()> {
    let indexed: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM rust_metadata WHERE key='message_items')",
        [],
        |row| row.get(0),
    )?;
    if indexed {
        return Ok(());
    }
    let mut statement = transaction.prepare(
        "SELECT h.id,h.sender,h.subject,b.body_html FROM rust_headers h JOIN rust_bodies b ON b.id=h.id
         WHERE (h.sender_key='service.eu@ugreen.com'
         OR ((b.body_html LIKE '%application/ld+json%' OR b.body_html LIKE '%itemscope%')
         AND (b.body_html LIKE '%Order%' OR b.body_html LIKE '%Invoice%' OR b.body_html LIKE '%ParcelDelivery%')))"
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        store(
            transaction,
            row.get(0)?,
            &Message {
                sender: row.get(1)?,
                subject: row.get(2)?,
                body_html: row.get(3)?,
                body_loaded: true,
                ..Default::default()
            },
        )?;
    }
    transaction.execute("INSERT INTO rust_metadata VALUES ('message_items','1')", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::storage::Storage,
        models::dashboard::{Range, timestamp},
    };

    #[test]
    fn backfills_old_receipts_and_parcels_and_keeps_the_index_current() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 1, sender: "shop@example.com".into(), timestamp: timestamp("2026-11-04T12:00:00Z").unwrap(),
            body_loaded: true,
            body_html: r#"<script type="application/ld+json">{"@graph":[{"@type":"ParcelDelivery","orderNumber":"123","deliveryStatus":"In transit"},{"@type":"Order","orderNumber":"123","orderDate":"2026-11-03","price":42,"priceCurrency":"EUR"}]}</script>"#.into(),
            ..Default::default()
        };
        storage.store("one", "INBOX", &message).unwrap();
        storage
            .0
            .execute("DELETE FROM rust_message_items", [])
            .unwrap();
        storage
            .0
            .execute("DELETE FROM rust_metadata WHERE key='message_items'", [])
            .unwrap();
        let storage = Storage::initialize(storage.0).unwrap();
        let items = storage
            .dashboard(&["one".into()], message.timestamp, Range::Month)
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| item.source.message.body_html.is_empty())
        );
        storage
            .0
            .execute("UPDATE rust_bodies SET body_html=''", [])
            .unwrap();
        assert_eq!(
            storage
                .dashboard(&["one".into()], message.timestamp, Range::Month)
                .unwrap()
                .len(),
            2
        );
        storage
            .store(
                "one",
                "INBOX",
                &Message {
                    body_html: String::new(),
                    ..message.clone()
                },
            )
            .unwrap();
        assert!(
            storage
                .dashboard(&["one".into()], message.timestamp, Range::Month)
                .unwrap()
                .is_empty()
        );
        storage.store("one", "INBOX", &message).unwrap();
        storage.0.execute("DELETE FROM rust_headers", []).unwrap();
        assert_eq!(
            storage
                .0
                .query_row("SELECT count(*) FROM rust_message_items", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
