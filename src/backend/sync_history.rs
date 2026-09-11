use super::Storage;
use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use std::time::{Duration, SystemTime};

impl Storage {
    pub fn record_sync_started(&self, at: SystemTime) -> Result<()> {
        let seconds = at.duration_since(SystemTime::UNIX_EPOCH)?.as_secs();
        self.0.execute(
            "INSERT OR REPLACE INTO rust_metadata (key, value) VALUES ('last_sync_started', ?1)",
            [seconds.to_string()],
        )?;
        Ok(())
    }

    pub fn last_sync_started(&self) -> Result<Option<SystemTime>> {
        let value: Option<String> = self
            .0
            .query_row(
                "SELECT value FROM rust_metadata WHERE key='last_sync_started'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| {
                SystemTime::UNIX_EPOCH
                    .checked_add(Duration::from_secs(value.parse()?))
                    .context("Invalid saved sync timestamp")
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_start_time_even_without_a_completed_sync() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-sync-history-{}",
            gtk::glib::uuid_string_random()
        ));
        let path = directory.join("cache.db");
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        {
            let storage = Storage::open(&path).unwrap();
            assert_eq!(storage.last_sync_started().unwrap(), None);
            storage.record_sync_started(at).unwrap();
        }
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.last_sync_started().unwrap(), Some(at));
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
