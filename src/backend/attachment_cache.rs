use crate::models::{Account, Message, attachment};
use anyhow::{Context, Result};
use gio::prelude::*;
use gtk::{gio, glib};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

mod filenames;

static CACHE: Mutex<()> = Mutex::new(());

struct Location {
    directory: PathBuf,
    identity: String,
    name: String,
}

type Index = BTreeMap<String, String>;

#[cfg(test)]
pub fn path(account: &Account, folder: &str, message: &Message, index: usize) -> Result<PathBuf> {
    let downloads = glib::user_special_dir(glib::UserDirectory::Downloads)
        .unwrap_or_else(|| glib::home_dir().join("Downloads"));
    let location = location(&downloads, &account.email, folder, message, index)?;
    let _lock = CACHE.lock().unwrap();
    filename(&location, &read_index(&location.directory)?)
}

fn location(
    downloads: &Path,
    account: &str,
    folder: &str,
    message: &Message,
    index: usize,
) -> Result<Location> {
    let name = message
        .attachments
        .get(index)
        .context("Attachment is no longer available")?;
    let source = if message.message_id.is_empty() {
        serde_json::to_string(&(folder, message.uid_validity, message.uid))?
    } else {
        message.message_id.clone()
    };
    let identity = serde_json::to_string(&(account.to_lowercase(), source, index, name))?;
    let digest = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, identity).unwrap();
    Ok(Location {
        directory: downloads.join("Brevlada"),
        identity: digest.to_string(),
        name: attachment::safe_name(name),
    })
}

pub fn load(
    account: &Account,
    folder: &str,
    message: &Message,
    index: usize,
    download: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<PathBuf> {
    let downloads = glib::user_special_dir(glib::UserDirectory::Downloads)
        .unwrap_or_else(|| glib::home_dir().join("Downloads"));
    load_at(
        &location(&downloads, &account.email, folder, message, index)?,
        download,
    )
}

fn load_at(location: &Location, download: impl FnOnce() -> Result<Vec<u8>>) -> Result<PathBuf> {
    {
        let _lock = CACHE.lock().unwrap();
        let mut index = read_index(&location.directory)?;
        filenames::migrate(&location.directory, &mut index)?;
        if index.contains_key(&location.identity) {
            let path = filename(location, &index)?;
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    let bytes = download()?;
    let _lock = CACHE.lock().unwrap();
    let mut index = read_index(&location.directory)?;
    let path = filename(location, &index)?;
    if index.contains_key(&location.identity) && path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(&location.directory)?;
    index.insert(
        location.identity.clone(),
        path.file_name().unwrap().to_string_lossy().into_owned(),
    );
    write_index(&location.directory, &index)?;
    write(&path, &bytes)?;
    Ok(path)
}

fn filename(location: &Location, index: &Index) -> Result<PathBuf> {
    let name = if let Some(name) = index.get(&location.identity) {
        anyhow::ensure!(
            attachment::safe_name(name) == *name && name != ".attachments.json",
            "Invalid saved attachment filename"
        );
        name.clone()
    } else {
        filenames::available(&location.directory, &location.name, index)?
    };
    Ok(location.directory.join(name))
}

fn read_index(directory: &Path) -> Result<Index> {
    match std::fs::read(directory.join(".attachments.json")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Index::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_index(directory: &Path, index: &Index) -> Result<()> {
    write(
        &directory.join(".attachments.json"),
        &serde_json::to_vec(index)?,
    )
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    gio::File::for_path(path).replace_contents(
        bytes,
        None,
        false,
        gio::FileCreateFlags::PRIVATE,
        gio::Cancellable::NONE,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn reuses_saved_files_and_downloads_again_after_deletion() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-attachment-cache-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let message = Message {
            message_id: "one@example.com".into(),
            attachments: vec!["../../résumé.txt".into(), "../../résumé.txt".into()],
            ..Default::default()
        };
        let first = location(&directory, "me@example.com", "INBOX", &message, 0).unwrap();
        let calls = Cell::new(0);
        let download = || {
            calls.set(calls.get() + 1);
            Ok(vec![0, 255, 10])
        };
        let path = load_at(&first, download).unwrap();
        assert_eq!(path, directory.join("Brevlada/résumé.txt"));
        assert_eq!(load_at(&first, download).unwrap(), path);
        assert_eq!(calls.get(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), [0, 255, 10]);
        let second = location(&directory, "me@example.com", "INBOX", &message, 1).unwrap();
        let duplicate = load_at(&second, || Ok(vec![1, 2])).unwrap();
        assert_eq!(duplicate, directory.join("Brevlada/résumé (2).txt"));
        assert_eq!(std::fs::read(&path).unwrap(), [0, 255, 10]);
        assert_eq!(std::fs::read(&duplicate).unwrap(), [1, 2]);
        let existing = directory.join("Brevlada/résumé (3).txt");
        std::fs::write(&existing, "User's file").unwrap();
        let other = location(&directory, "other@example.com", "INBOX", &message, 0).unwrap();
        assert_eq!(
            load_at(&other, || Ok(vec![3])).unwrap(),
            directory.join("Brevlada/résumé (4).txt")
        );
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "User's file");
        let mut synced = message.clone();
        synced.uid = 42;
        synced.uid_validity = Some(7);
        let synced = location(&directory, "me@example.com", "Sent", &synced, 0).unwrap();
        assert_eq!(load_at(&synced, download).unwrap(), path);
        assert_eq!(calls.get(), 1);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(load_at(&first, download).unwrap(), path);
        assert_eq!(calls.get(), 2);
        std::fs::remove_file(&path).unwrap();
        assert!(load_at(&first, || anyhow::bail!("Network failed")).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read(&duplicate).unwrap(), [1, 2]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migrates_hash_folders_without_downloading_or_overwriting_files() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-attachment-migration-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let message = Message {
            message_id: "one@example.com".into(),
            attachments: vec![
                "notes.txt".into(),
                "notes.txt".into(),
                ".attachments.json".into(),
            ],
            ..Default::default()
        };
        let first = location(&directory, "me@example.com", "INBOX", &message, 0).unwrap();
        let second = location(&directory, "me@example.com", "INBOX", &message, 1).unwrap();
        for (location, bytes) in [(&first, "First"), (&second, "Second")] {
            let folder = location.directory.join(&location.identity);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(&location.name), bytes).unwrap();
        }
        std::fs::write(first.directory.join("notes.txt"), "User's file").unwrap();
        let path = load_at(&first, || anyhow::bail!("Must reuse existing file")).unwrap();
        let other = load_at(&second, || anyhow::bail!("Must reuse existing file")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "First");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "Second");
        assert_ne!(path, other);
        assert_eq!(path.parent().unwrap(), first.directory);
        assert_eq!(other.parent().unwrap(), first.directory);
        assert!(!first.directory.join(&first.identity).exists());
        assert!(!second.directory.join(&second.identity).exists());
        assert_eq!(
            std::fs::read_to_string(first.directory.join("notes.txt")).unwrap(),
            "User's file"
        );
        let hidden = location(&directory, "me@example.com", "INBOX", &message, 2).unwrap();
        let hidden_path = load_at(&hidden, || Ok(b"Attachment contents".to_vec())).unwrap();
        assert_eq!(hidden_path.file_name().unwrap(), ".attachments (2).json");
        assert_eq!(
            load_at(&first, || anyhow::bail!("Must reuse existing file")).unwrap(),
            path
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
