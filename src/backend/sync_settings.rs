use super::Storage;
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub fn sync_folders(&self, account: &str) -> Result<Vec<String>> {
        self.0.execute(
            "INSERT OR IGNORE INTO rust_sync_settings (account_id,folders) VALUES (?1,?2)",
            params![account, serde_json::to_string(&["INBOX"])?],
        )?;
        let folders: String = self.0.query_row(
            "SELECT folders FROM rust_sync_settings WHERE account_id=?1",
            [account],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&folders)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_existing_and_new_accounts_to_inbox_and_preserves_saved_selections() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-settings-{}",
            gtk::glib::uuid_string_random()
        ));
        let path = directory.join("cache.db");
        let mut storage = Storage::open(&path).unwrap();
        storage
            .store_folders("existing", &["INBOX".into(), "Archive".into()])
            .unwrap();
        assert_eq!(storage.sync_folders("existing").unwrap(), ["INBOX"]);
        assert_eq!(storage.sync_folders("new").unwrap(), ["INBOX"]);
        storage
            .0
            .execute(
                "UPDATE rust_sync_settings SET folders=?2 WHERE account_id=?1",
                params!["existing", r#"["INBOX","Archive"]"#],
            )
            .unwrap();
        storage
            .0
            .execute(
                "UPDATE rust_sync_settings SET folders='[]' WHERE account_id='new'",
                [],
            )
            .unwrap();
        drop(storage);
        let storage = Storage::open(&path).unwrap();
        assert_eq!(
            storage.sync_folders("existing").unwrap(),
            ["INBOX", "Archive"]
        );
        assert!(storage.sync_folders("new").unwrap().is_empty());
        assert_eq!(storage.sync_folders("another").unwrap(), ["INBOX"]);
        assert_eq!(storage.folders("existing").unwrap(), ["Archive", "INBOX"]);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
