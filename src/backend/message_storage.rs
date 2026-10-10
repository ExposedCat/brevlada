//! Messages are stored as header rows, which list queries read, and body rows,
//! which also keep the HTML text and ticket data derived when a body is stored.
use crate::models::{Message, dom::Html, search::html_text, senders, ticket};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};

/// Header columns in the order `decode` reads them, followed in each query by
/// the body text, body HTML and inline media.
macro_rules! columns {
    () => {
        "uid, uid_validity, message_id, subject, sender, reply_to, recipients, cc, in_reply_to, \
         date, timestamp, \"references\", is_read, read_revision, is_draft, is_flagged, is_spam, \
         flag_revision, attachments, attachment_details, inline_media_loaded, unsubscribe, \
         parcels, calendar_events, tickets, body_loaded"
    };
}
pub(super) use columns;

/// Selects header columns and the given body columns of each message.
macro_rules! select {
    ($body:literal) => {
        concat!(
            "SELECT ",
            $crate::backend::storage::messages::columns!(),
            ", ",
            $body,
            " FROM rust_headers h LEFT JOIN rust_bodies b ON b.id=h.id"
        )
    };
    () => {
        $crate::backend::storage::messages::select!(
            "coalesce(b.body_text, ''), coalesce(b.body_html, ''), coalesce(b.inline_media, '[]')"
        )
    };
}
pub(super) use select;

macro_rules! insert {
    ($conflict:literal) => {
        concat!(
            "INSERT OR ",
            $conflict,
            " INTO rust_headers (id, account_id, folder, sender_key, ",
            columns!(),
            ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
             ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)"
        )
    };
}

/// Message ids are the mailbox id shifted above the 32-bit UID, so a message is
/// found by its id alone and a mailbox's messages form one contiguous range.
#[derive(Clone, Copy)]
pub(super) struct Mailbox(i64);

impl Mailbox {
    pub fn find(connection: &Connection, account: &str, folder: &str) -> Result<Option<Self>> {
        Ok(connection
            .prepare_cached("SELECT id FROM rust_mailboxes WHERE account_id=?1 AND folder=?2")?
            .query_row(params![account, folder], |row| row.get(0))
            .optional()?
            .map(Self))
    }

    pub fn ensure(connection: &Connection, account: &str, folder: &str) -> Result<Self> {
        if let Some(mailbox) = Self::find(connection, account, folder)? {
            return Ok(mailbox);
        }
        Ok(Self(
            connection
                .prepare_cached(
                    "INSERT INTO rust_mailboxes (account_id, folder) VALUES (?1, ?2) RETURNING id",
                )?
                .query_row(params![account, folder], |row| row.get(0))?,
        ))
    }

    pub fn id(self, uid: u32) -> i64 {
        self.0 << 32 | i64::from(uid)
    }

    pub fn first(self) -> i64 {
        self.id(0)
    }

    pub fn last(self) -> i64 {
        self.id(u32::MAX)
    }
}

/// Locally sent messages have no mailbox. Mailbox ids start at one, which
/// leaves them the ids below the first mailbox.
pub(super) fn sent_id(connection: &Connection, account: &str, message_id: &str) -> Result<i64> {
    Ok(connection
        .prepare_cached(
            "SELECT coalesce(
             (SELECT id FROM rust_headers WHERE account_id=?1 AND folder IS NULL AND message_id=?2),
             (SELECT coalesce(max(id), 0) + 1 FROM rust_headers WHERE id<?3))",
        )?
        .query_row(params![account, message_id, Mailbox(1).first()], |row| {
            row.get(0)
        })?)
}

pub(super) fn json<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(row.get_ref(index)?.as_str()?)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, error.into()))
}

/// List rows and search read the visible text of an HTML body after the plain
/// text, unless the plain text already includes it.
pub(super) fn append_html_text(text: &mut String, html_text: &str) {
    if !html_text.is_empty() && !text.contains(html_text) {
        text.push('\n');
        text.push_str(html_text);
    }
}

pub(super) fn decode(row: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        uid: row.get(0)?,
        uid_validity: row.get(1)?,
        message_id: row.get(2)?,
        subject: row.get(3)?,
        sender: row.get(4)?,
        reply_to: row.get(5)?,
        recipients: row.get(6)?,
        cc: row.get(7)?,
        in_reply_to: row.get(8)?,
        date: row.get(9)?,
        timestamp: row.get(10)?,
        references: json(row, 11)?,
        is_read: row.get(12)?,
        read_revision: row.get(13)?,
        is_draft: row.get(14)?,
        is_flagged: row.get(15)?,
        is_spam: row.get(16)?,
        flag_revision: row.get(17)?,
        attachments: json(row, 18)?,
        attachment_details: json(row, 19)?,
        inline_media_loaded: row.get(20)?,
        unsubscribe: json(row, 21)?,
        parcels: json(row, 22)?,
        calendar_events: json(row, 23)?,
        tickets: json(row, 24)?,
        body_loaded: row.get(25)?,
        body_text: row.get(26)?,
        body_html: row.get(27)?,
        inline_media: json(row, 28)?,
        ..Default::default()
    })
}

fn insert(
    connection: &Connection,
    sql: &str,
    id: i64,
    account: &str,
    folder: Option<&str>,
    message: &Message,
) -> Result<bool> {
    Ok(connection.prepare_cached(sql)?.execute(params![
        id,
        account,
        folder,
        senders::key(message),
        message.uid,
        message.uid_validity,
        message.message_id,
        message.subject,
        message.sender,
        message.reply_to,
        message.recipients,
        message.cc,
        message.in_reply_to,
        message.date,
        message.timestamp,
        serde_json::to_string(&message.references)?,
        message.is_read,
        message.read_revision,
        message.is_draft,
        message.is_flagged,
        message.is_spam,
        message.flag_revision,
        serde_json::to_string(&message.attachments)?,
        serde_json::to_string(&message.attachment_details)?,
        message.inline_media_loaded,
        serde_json::to_string(&message.unsubscribe)?,
        serde_json::to_string(&message.parcels)?,
        serde_json::to_string(&message.calendar_events)?,
        serde_json::to_string(&message.tickets)?,
        message.body_loaded,
    ])? > 0)
}

fn store_body(connection: &Connection, id: i64, message: &Message) -> Result<()> {
    if message.body_text.is_empty()
        && message.body_html.is_empty()
        && message.inline_media.is_empty()
    {
        connection
            .prepare_cached("DELETE FROM rust_bodies WHERE id=?1")?
            .execute([id])?;
        return Ok(());
    }
    let html_text = if message.body_html.is_empty() {
        String::new()
    } else {
        html_text(&message.body_html)
    };
    connection
        .prepare_cached("INSERT OR REPLACE INTO rust_bodies VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?
        .execute(params![
            id,
            message.body_text,
            message.body_html,
            html_text,
            serde_json::to_string(&ticket::parse(&Html::new(&message.body_html)))?,
            serde_json::to_string(&message.inline_media)?,
        ])?;
    Ok(())
}

/// Stores a message, replacing any message with the same id.
pub(super) fn put(
    connection: &Connection,
    id: i64,
    account: &str,
    folder: Option<&str>,
    message: &Message,
) -> Result<()> {
    insert(connection, insert!("REPLACE"), id, account, folder, message)?;
    store_body(connection, id, message)
}

/// Stores a message unless a message with the same id is already cached.
pub(super) fn add(
    connection: &Connection,
    id: i64,
    account: &str,
    folder: Option<&str>,
    message: &Message,
) -> Result<()> {
    if insert(connection, insert!("IGNORE"), id, account, folder, message)? {
        store_body(connection, id, message)?;
    }
    Ok(())
}

/// Stores a fetched body and the header fields fetched with it. Unlike `put`,
/// this leaves the indexed columns, and so the index pages, untouched.
pub(super) fn store_fetched(connection: &Connection, id: i64, message: &Message) -> Result<()> {
    connection
        .prepare_cached(
            "UPDATE rust_headers SET uid_validity=?2, message_id=?3, reply_to=?4, cc=?5,
             in_reply_to=?6, attachments=?7, attachment_details=?8, inline_media_loaded=?9,
             unsubscribe=?10, parcels=?11, calendar_events=?12, tickets=?13, body_loaded=?14
             WHERE id=?1",
        )?
        .execute(params![
            id,
            message.uid_validity,
            message.message_id,
            message.reply_to,
            message.cc,
            message.in_reply_to,
            serde_json::to_string(&message.attachments)?,
            serde_json::to_string(&message.attachment_details)?,
            message.inline_media_loaded,
            serde_json::to_string(&message.unsubscribe)?,
            serde_json::to_string(&message.parcels)?,
            serde_json::to_string(&message.calendar_events)?,
            serde_json::to_string(&message.tickets)?,
            message.body_loaded,
        ])?;
    store_body(connection, id, message)
}

/// Replaces the header fields a folder refresh reports, keeping the cached body,
/// attachments and the cards derived from them.
pub(super) fn refresh(connection: &Connection, id: i64, message: &Message) -> Result<()> {
    connection
        .prepare_cached(
            "UPDATE rust_headers SET sender_key=?2, uid_validity=?3, message_id=?4, subject=?5,
             sender=?6, reply_to=?7, recipients=?8, cc=?9, in_reply_to=?10, date=?11,
             timestamp=?12, \"references\"=?13, is_read=?14, read_revision=?15, is_draft=?16,
             is_flagged=?17, is_spam=?18, flag_revision=?19, unsubscribe=?20 WHERE id=?1",
        )?
        .execute(params![
            id,
            senders::key(message),
            message.uid_validity,
            message.message_id,
            message.subject,
            message.sender,
            message.reply_to,
            message.recipients,
            message.cc,
            message.in_reply_to,
            message.date,
            message.timestamp,
            serde_json::to_string(&message.references)?,
            message.is_read,
            message.read_revision,
            message.is_draft,
            message.is_flagged,
            message.is_spam,
            message.flag_revision,
            serde_json::to_string(&message.unsubscribe)?,
        ])?;
    Ok(())
}
