use super::Storage;
use anyhow::Result;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

type Counts = HashMap<(String, String), usize>;

impl Storage {
    pub(super) fn subscription_counts(&self, accounts: &[String], now: i64) -> Result<Counts> {
        let mut statement = self.0.prepare(
            "SELECT account_id, sender_key, message_id, folder, uid, uid_validity
             FROM rust_headers WHERE account_id IN (SELECT value FROM json_each(?1))
             AND timestamp>=?2 AND timestamp<=?3 AND folder IS NOT NULL
             AND NOT is_draft AND NOT is_spam",
        )?;
        let mut rows = statement.query(params![
            serde_json::to_string(accounts)?,
            now - crate::models::dashboard::SUBSCRIPTION_RECENT_DAYS * 86400,
            now
        ])?;
        let mut seen: HashMap<(String, String), HashSet<String>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let folder: String = row.get(3)?;
            if crate::backend::mail_sync::excluded_folder(&folder, &[]) {
                continue;
            }
            let account: String = row.get(0)?;
            let sender: String = row.get(1)?;
            let message_id: String = row.get(2)?;
            let identity = if message_id.is_empty() {
                serde_json::to_string(&(
                    folder,
                    row.get::<_, u32>(4)?,
                    row.get::<_, Option<u32>>(5)?,
                ))?
            } else {
                message_id
            };
            seen.entry((account, sender)).or_default().insert(identity);
        }
        Ok(seen
            .into_iter()
            .map(|(key, messages)| (key, messages.len()))
            .collect())
    }
}
