use super::Storage;
use crate::backend::avatars::{Source, now};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

pub struct Cached {
    pub image: Option<Vec<u8>>,
    pub fetched_at: i64,
}

impl Storage {
    pub fn avatar(&self, email: &str) -> Result<Option<Cached>> {
        Ok(self
            .0
            .query_row(
                "SELECT image, fetched_at FROM rust_avatars WHERE email=?1",
                params![email],
                |row| {
                    Ok(Cached {
                        image: row.get(0)?,
                        fetched_at: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn store_avatar(&self, email: &str, source: Source, image: Option<&[u8]>) -> Result<()> {
        self.0.execute(
            "INSERT OR REPLACE INTO rust_avatars VALUES (?1,?2,?3,?4)",
            params![email, source.as_str(), image, now()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::avatars;

    #[test]
    fn remembers_downloaded_avatars_and_misses_separately() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        assert!(storage.avatar("ada@example.com").unwrap().is_none());
        storage
            .store_avatar("ada@example.com", Source::Google, Some(b"image"))
            .unwrap();
        let cached = storage.avatar("ada@example.com").unwrap().unwrap();
        assert_eq!(cached.image.as_deref(), Some(b"image".as_slice()));
        assert!(avatars::is_fresh(true, cached.fetched_at, now()));
        // A later lookup replaces the entry rather than adding a duplicate.
        storage
            .store_avatar("ada@example.com", Source::None, None)
            .unwrap();
        let cached = storage.avatar("ada@example.com").unwrap().unwrap();
        assert!(cached.image.is_none());
        let source: String = storage
            .0
            .query_row("SELECT source FROM rust_avatars", [], |row| row.get(0))
            .unwrap();
        assert_eq!(source, "none");
    }
}
