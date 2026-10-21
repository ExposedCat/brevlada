use super::{
    Storage,
    messages::{self, Mailbox},
};
use crate::{
    backend::{mail_sync::Flags, parser::normalize_message_id},
    models::Message,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;

impl Storage {
    pub fn uncached_uids(&self, account: &str, folder: &str) -> Result<Vec<u32>> {
        let mut statement = self.0.prepare(
            "SELECT uid FROM rust_headers WHERE account_id=?1 AND folder=?2 AND NOT body_loaded
             ORDER BY timestamp DESC, uid DESC",
        )?;
        Ok(statement
            .query_map(params![account, folder], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[cfg(test)]
    pub fn messages(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        self.messages_with_limit(account, folder, crate::theme::MESSAGE_LIMIT)
    }

    #[cfg(test)]
    pub fn messages_with_limit(
        &self,
        account: &str,
        folder: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(concat!(
            messages::select!(),
            " WHERE h.account_id=?1 AND h.folder=?2
             ORDER BY h.timestamp DESC, h.uid DESC LIMIT ?3"
        ))?;
        Ok(statement
            .query_map(params![account, folder, limit as i64], messages::decode)?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[cfg(test)]
    pub fn sender_page(
        &self,
        account: &str,
        folder: &str,
        sender: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(concat!(
            messages::select!("coalesce(b.body_text, ''), coalesce(b.body_html, ''), '[]'"),
            " WHERE h.account_id=?1 AND h.folder=?2 AND h.sender_key=?3
             AND instr(lower(h.sender), ?3) > 0
             AND (?4 IS NULL OR (h.timestamp, h.uid) < (?4, ?5))
             ORDER BY h.timestamp DESC, h.uid DESC LIMIT ?6"
        ))?;
        let rows = statement.query_map(
            params![
                account,
                folder,
                sender,
                before.map(|b| b.0),
                before.map(|b| b.1),
                limit as i64
            ],
            |row| {
                let mut message = messages::decode(row)?;
                message.inline_media_loaded = false;
                message.prepare_display();
                Ok(message)
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    #[cfg(test)]
    pub fn recipient_page(
        &self,
        account: &str,
        folder: &str,
        recipient: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        self.correspondent_page(
            account,
            folder,
            recipient,
            before,
            limit,
            &crate::models::senders::Correspondence {
                outgoing: true,
                ..Default::default()
            },
        )
    }

    pub fn correspondent_page(
        &self,
        account: &str,
        folder: &str,
        recipient: &str,
        before: Option<(i64, u32)>,
        limit: usize,
        scope: &crate::models::senders::Correspondence,
    ) -> Result<Vec<Message>> {
        let mut candidates = self.0.prepare(
            "SELECT uid, sender, recipients FROM rust_headers
             WHERE account_id=?1 AND folder=?2
             AND (?3 IS NULL OR (timestamp, uid) < (?3, ?4))
             AND (?5 OR sender_key=?6 OR sender_key IN (SELECT value FROM json_each(?7)))
             ORDER BY timestamp DESC, uid DESC",
        )?;
        let rows = candidates.query_map(
            params![
                account,
                folder,
                before.map(|b| b.0),
                before.map(|b| b.1),
                scope.outgoing,
                recipient,
                serde_json::to_string(&scope.own)?
            ],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut selected = Vec::new();
        for row in rows {
            if selected.len() == limit {
                break;
            }
            let (uid, sender, recipients) = row?;
            if scope.matches(
                &Message {
                    sender,
                    recipients,
                    ..Default::default()
                },
                recipient,
            ) {
                selected.push(uid);
            }
        }
        let mut statement = self.0.prepare(concat!(
            messages::select!("coalesce(b.body_text, ''), coalesce(b.body_html, ''), '[]'"),
            " WHERE h.account_id=?1 AND h.folder=?2 AND h.uid=?3"
        ))?;
        let mut page = Vec::new();
        for uid in selected {
            let mut message =
                statement.query_row(params![account, folder, uid], messages::decode)?;
            message.inline_media_loaded = false;
            message.prepare_display();
            page.push(message);
        }
        Ok(page)
    }

    pub fn pending_bodies(
        &self,
        account: &str,
        folder: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(concat!(
            messages::select!("'', '', coalesce(b.inline_media, '[]')"),
            " WHERE h.account_id=?1 AND h.folder=?2 AND NOT h.body_loaded
             AND (?3 IS NULL OR (h.timestamp, h.uid) < (?3, ?4))
             ORDER BY h.timestamp DESC, h.uid DESC LIMIT ?5"
        ))?;
        let rows = statement.query_map(
            params![
                account,
                folder,
                before.map(|b| b.0),
                before.map(|b| b.1),
                limit as i64
            ],
            |row| {
                let mut message = messages::decode(row)?;
                message.attachments.clear();
                message.calendar_events.clear();
                message.tickets.clear();
                Ok(message)
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn validity(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Self::folder_validity(&self.0, account, folder)
    }

    pub(super) fn folder_validity(
        connection: &Connection,
        account: &str,
        folder: &str,
    ) -> Result<Option<u32>> {
        Ok(connection
            .prepare_cached(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
            )?
            .query_row(params![account, folder], |r| r.get(0))
            .optional()?
            .flatten())
    }

    #[cfg(test)]
    pub fn inventory(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        flags: &[Flags],
    ) -> Result<Vec<u32>> {
        self.inventory_since(account, folder, validity, flags, self.read_revision()?)
    }

    pub fn inventory_since(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        flags: &[Flags],
        snapshot_revision: i64,
    ) -> Result<Vec<u32>> {
        let (existing, live) = self.write(|transaction| {
            let revision = Self::next_read_revision(transaction)?;
            let mailbox = Mailbox::ensure(transaction, account, folder)?;
            let reads = Self::reads_since(
                transaction,
                mailbox,
                account,
                folder,
                validity,
                snapshot_revision,
            )?;
            let starred = Self::flags_since(
                transaction,
                mailbox,
                account,
                folder,
                validity,
                snapshot_revision,
            )?;
            let read = |item: &Flags| reads.get(&item.uid).copied().unwrap_or(item.read);
            let previous = Self::folder_validity(transaction, account, folder)?;
            Self::observe_notifications(
                transaction,
                mailbox,
                account,
                folder,
                validity,
                previous,
                &flags
                    .iter()
                    .map(|item| (item.uid, !read(item) && !item.draft))
                    .collect::<Vec<_>>(),
            )?;
            if previous != Some(validity) {
                transaction.execute(
                    "DELETE FROM rust_headers WHERE id BETWEEN ?1 AND ?2",
                    params![mailbox.first(), mailbox.last()],
                )?;
            }
            let existing = Self::uids(transaction, mailbox)?;
            let removed = Self::removed_in(transaction, account, folder, validity)?;
            let live: HashSet<_> = flags
                .iter()
                .map(|item| item.uid)
                .filter(|uid| !removed.contains(uid))
                .collect();
            for uid in existing.difference(&live) {
                transaction
                    .prepare_cached("DELETE FROM rust_headers WHERE id=?1")?
                    .execute([mailbox.id(*uid)])?;
            }
            let mut update = transaction.prepare_cached(
                "UPDATE rust_headers SET is_read=?2, is_flagged=?3, is_draft=?4, read_revision=?5,
                 flag_revision=?5 WHERE id=?1",
            )?;
            for item in flags.iter().filter(|item| existing.contains(&item.uid)) {
                update.execute(params![
                    mailbox.id(item.uid),
                    read(item),
                    starred.get(&item.uid).copied().unwrap_or(item.flagged),
                    item.draft,
                    revision
                ])?;
            }
            transaction.execute(
                "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
                params![account, folder, validity],
            )?;
            transaction.execute(
                "INSERT OR REPLACE INTO rust_unread VALUES (?1,?2,?3)",
                params![
                    account,
                    folder,
                    flags.iter().any(|f| !read(f) && !removed.contains(&f.uid))
                ],
            )?;
            Ok((existing, live))
        })?;
        let mut missing: Vec<_> = live.difference(&existing).copied().collect();
        missing.sort_unstable_by(|a, b| b.cmp(a));
        Ok(missing)
    }

    pub fn store_headers(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        messages: &[Message],
    ) -> Result<()> {
        self.write(|transaction| {
            let revision = Self::next_read_revision(transaction)?;
            let current = Self::folder_validity(transaction, account, folder)?;
            ensure!(current == Some(validity), "Mailbox changed during sync");
            let mailbox = Mailbox::ensure(transaction, account, folder)?;
            let removed = Self::removed_in(transaction, account, folder, validity)?;
            for message in messages
                .iter()
                .filter(|message| !removed.contains(&message.uid))
            {
                let mut message = message.clone();
                message.read_revision = revision;
                message.flag_revision = revision;
                messages::add(
                    transaction,
                    mailbox.id(message.uid),
                    account,
                    Some(folder),
                    &message,
                )?;
            }
            Ok(())
        })
    }

    pub fn store_body(
        &mut self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        fetched: &Message,
    ) -> Result<Option<Message>> {
        self.write(|transaction| {
            if Self::folder_validity(transaction, account, folder)? != validity {
                return Ok(None);
            }
            let Some(mut message) = Self::cached(transaction, account, folder, fetched.uid)? else {
                return Ok(None);
            };
            if !normalize_message_id(&message.message_id).is_empty()
                && !(message.is_draft && fetched.message_id.is_empty())
                && normalize_message_id(&message.message_id)
                    != normalize_message_id(&fetched.message_id)
            {
                return Ok(None);
            }
            if message.is_draft && message.message_id.is_empty() {
                message.message_id = fetched.message_id.clone();
            }
            message.uid_validity = validity;
            message.reply_to = fetched.reply_to.clone();
            message.cc = fetched.cc.clone();
            message.in_reply_to = fetched.in_reply_to.clone();
            message.body_text = fetched.body_text.clone();
            message.body_html = fetched.body_html.clone();
            message.attachments = fetched.attachments.clone();
            message.attachment_details = fetched.attachment_details.clone();
            if fetched.inline_media_loaded {
                message.inline_media = fetched.inline_media.clone();
                message.inline_media_loaded = true;
            }
            message.unsubscribe = fetched.unsubscribe.clone().or(message.unsubscribe);
            message.parcels = fetched.parcels.clone();
            message.calendar_events = fetched.calendar_events.clone();
            message.tickets = fetched.tickets.clone();
            message.body_loaded = fetched.body_loaded;
            let mailbox = Mailbox::ensure(transaction, account, folder)?;
            messages::store_fetched(transaction, mailbox.id(message.uid), &message)?;
            Ok(Some(message))
        })
    }

    pub fn store_unsubscribe(
        &self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        header: &Message,
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let current = self
            .message(account, folder, header.uid)?
            .ok_or_else(|| anyhow::anyhow!("Message is no longer cached"))?;
        ensure!(
            normalize_message_id(&current.message_id) == normalize_message_id(&header.message_id),
            "The mailbox changed. Reload the folder."
        );
        let updated = self.0.execute(
            "UPDATE rust_headers SET unsubscribe=?4 WHERE id=?3
             AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2) IS ?5
             AND message_id=?6",
            params![
                account,
                folder,
                Mailbox::ensure(&self.0, account, folder)?.id(header.uid),
                serde_json::to_string(&header.unsubscribe)?,
                validity,
                current.message_id
            ],
        )?;
        ensure!(
            updated == 1,
            "Mailbox changed while loading unsubscribe headers"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_legacy_unsubscribe_headers_without_overwriting_cached_body_or_flags() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let legacy = Message {
            uid: 2,
            message_id: "same".into(),
            body_loaded: true,
            body_text: "Cached body".into(),
            is_read: true,
            is_flagged: true,
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.org/u".into(),
                one_click: None,
            }),
            ..Default::default()
        };
        storage.store("a", "INBOX", &legacy).unwrap();
        let header = Message {
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.org/u".into(),
                one_click: Some(true),
            }),
            body_loaded: false,
            body_text: String::new(),
            is_read: false,
            is_flagged: false,
            ..legacy.clone()
        };
        storage
            .store_unsubscribe("a", "INBOX", None, &header)
            .unwrap();
        assert_eq!(
            storage.message("a", "INBOX", 2).unwrap().unwrap(),
            Message {
                unsubscribe: header.unsubscribe.clone(),
                ..legacy
            }
        );
        assert!(
            storage
                .store_unsubscribe("a", "INBOX", Some(99), &header)
                .is_err()
        );
        assert!(
            storage
                .store_unsubscribe(
                    "a",
                    "INBOX",
                    None,
                    &Message {
                        message_id: "replaced".into(),
                        ..header
                    }
                )
                .is_err()
        );
        assert_eq!(
            storage
                .message("a", "INBOX", 2)
                .unwrap()
                .unwrap()
                .unsubscribe
                .unwrap()
                .one_click,
            Some(true)
        );
    }

    #[test]
    fn resumes_missing_work_and_rejects_obsolete_bodies() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let flags = [
            Flags {
                uid: 1,
                read: false,
                flagged: false,
                draft: false,
            },
            Flags {
                uid: 2,
                read: false,
                flagged: false,
                draft: false,
            },
        ];
        assert_eq!(
            storage.inventory("a", "INBOX", 7, &flags).unwrap(),
            vec![2, 1]
        );
        let header = Message {
            uid: 2,
            message_id: "new".into(),
            ..Default::default()
        };
        storage
            .store_headers("a", "INBOX", 7, std::slice::from_ref(&header))
            .unwrap();
        assert_eq!(storage.inventory("a", "INBOX", 7, &flags).unwrap(), vec![1]);
        let fetched = Message {
            body_loaded: true,
            body_text: "Cached body".into(),
            tickets: vec![crate::models::ticket::Reservation {
                name: "Express".into(),
                kind: crate::models::ticket::Kind::Train,
                number: Some("booking-one".into()),
                ..Default::default()
            }],
            ..header.clone()
        };
        let read = Message {
            is_read: true,
            is_flagged: true,
            ..header.clone()
        };
        storage.store("a", "INBOX", &read).unwrap();
        let stored = storage
            .store_body("a", "INBOX", Some(7), &fetched)
            .unwrap()
            .unwrap();
        assert!(stored.is_read && stored.is_flagged && stored.body_loaded);
        assert_eq!(stored.tickets, fetched.tickets);
        let marked = storage
            .mark_read_cached("a", "INBOX", Some(7), 2)
            .unwrap()
            .unwrap();
        assert!(marked.is_read && marked.is_flagged && marked.body_loaded);
        assert!(
            storage
                .mark_read_cached("a", "INBOX", Some(8), 2)
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .pending_bodies("a", "INBOX", None, 128)
                .unwrap()
                .is_empty()
        );
        storage.inventory("a", "INBOX", 8, &flags).unwrap();
        assert!(
            storage
                .store_body("a", "INBOX", Some(7), &fetched)
                .unwrap()
                .is_none()
        );
        storage.store_headers("a", "INBOX", 8, &[header]).unwrap();
        storage.inventory("a", "INBOX", 8, &[]).unwrap();
        assert!(
            storage
                .store_body("a", "INBOX", Some(8), &fetched)
                .unwrap()
                .is_none()
        );
        assert!(storage.messages("a", "INBOX").unwrap().is_empty());
    }
}
