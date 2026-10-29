use super::write;
use anyhow::Result;
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) const FILENAME: &str = ".attachment-paths.json";

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Index {
    files: BTreeMap<String, Vec<File>>,
    messages: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct File {
    path: PathBuf,
    size: u64,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    glib::compute_checksum_for_data(glib::ChecksumType::Sha1, bytes)
        .unwrap()
        .to_string()
}

impl File {
    fn matches(&self, expected: &str) -> bool {
        let check = || -> Result<bool> {
            let mut file = std::fs::File::open(&self.path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() != self.size {
                return Ok(false);
            }
            let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha1).unwrap();
            let mut buffer = [0; 64 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                checksum.update(&buffer[..count]);
            }
            Ok(checksum.string().as_deref() == Some(expected))
        };
        check().unwrap_or(false)
    }
}

impl Index {
    pub(super) fn read(directory: &Path) -> Result<Self> {
        match std::fs::read(directory.join(FILENAME)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn save(&self, directory: &Path) -> Result<()> {
        std::fs::create_dir_all(directory)?;
        write(&directory.join(FILENAME), &serde_json::to_vec(self)?)
    }

    pub(super) fn remember(&mut self, path: &Path, bytes: &[u8]) -> String {
        let digest = digest(bytes);
        let files = self.files.entry(digest.clone()).or_default();
        if !files.iter().any(|file| file.path == path) {
            files.push(File {
                path: path.into(),
                size: bytes.len() as u64,
            });
        }
        digest
    }

    pub(super) fn bind(&mut self, identity: &str, digest: &str) {
        self.messages.insert(identity.into(), digest.into());
    }

    pub(super) fn remember_copy(&mut self, identity: &str, path: &Path) -> Result<()> {
        let digest = self
            .messages
            .get(identity)
            .ok_or_else(|| anyhow::anyhow!("Attachment fingerprint is unavailable"))?;
        let source = self
            .files
            .get(digest)
            .and_then(|files| files.first())
            .ok_or_else(|| anyhow::anyhow!("Attachment path is unavailable"))?;
        let file = File {
            path: path.into(),
            size: source.size,
        };
        anyhow::ensure!(
            file.matches(digest),
            "Saved attachment changed before its path could be remembered"
        );
        let files = self.files.get_mut(digest).unwrap();
        files.retain(|file| file.path != path);
        files.insert(0, file);
        Ok(())
    }

    pub(super) fn known(&self, identity: &str) -> bool {
        self.messages.contains_key(identity)
    }

    pub(super) fn has_source(&self, digest: &str) -> bool {
        self.files.contains_key(digest)
    }

    pub(super) fn source(&self, digest: &str) -> Option<PathBuf> {
        self.files
            .get(digest)
            .and_then(|files| files.iter().find(|file| file.matches(digest)))
            .map(|file| file.path.clone())
    }

    pub(super) fn path(&self, identity: &str) -> Option<PathBuf> {
        self.source(self.messages.get(identity)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{
            attachment_cache::{load_at, location, remember_parts_at},
            attachments,
        },
        models::Message,
    };

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brevlada-local-attachments-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn original_paths_survive_draft_saves_sending_and_folder_changes_without_downloading() {
        let directory = directory();
        let original = directory.join("résumé.bin");
        let bytes = b"original";
        std::fs::write(&original, bytes).unwrap();
        let cache = directory.join("Brevlada");
        let mut index = Index::default();
        index.remember(&original, bytes);
        index.save(&cache).unwrap();
        let parts = vec![
            "Content-Type: image/png\r\nContent-ID: <inline>\r\n\r\nPNG".into(),
            attachments::encode("résumé.bin", "application/octet-stream", bytes).unwrap(),
            attachments::encode("résumé.bin", "application/octet-stream", b"different").unwrap(),
        ];
        let mut message = Message {
            message_id: "draft-1".into(),
            attachments: vec!["résumé.bin".into(), "résumé.bin".into()],
            ..Default::default()
        };
        for id in ["draft-1", "draft-2", "sent"] {
            message.message_id = id.into();
            remember_parts_at(&directory, "me@example.com", "Drafts", &message, &parts).unwrap();
            let local = location(&directory, "me@example.com", "Drafts", &message, 0).unwrap();
            assert_eq!(
                load_at(&local, || anyhow::bail!(
                    "Must not download a local attachment"
                ))
                .unwrap(),
                original
            );
            let other = location(&directory, "me@example.com", "Drafts", &message, 1).unwrap();
            assert!(
                load_at(&other, || anyhow::bail!(
                    "This duplicate name has different contents"
                ))
                .is_err()
            );
        }
        message.uid = 42;
        message.uid_validity = Some(7);
        let synced = location(&directory, "me@example.com", "Sent", &message, 0).unwrap();
        assert_eq!(
            load_at(&synced, || anyhow::bail!(
                "Must reuse the persisted original path"
            ))
            .unwrap(),
            original
        );
        let other_account = location(&directory, "other@example.com", "Sent", &message, 0).unwrap();
        assert!(
            load_at(&other_account, || anyhow::bail!(
                "No binding for this account"
            ))
            .is_err()
        );
        assert!(!cache.join("résumé.bin").exists());

        let modified = std::fs::metadata(&original).unwrap().modified().unwrap();
        std::fs::write(&original, b"modified").unwrap();
        std::fs::File::open(&original)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let downloaded = load_at(&synced, || Ok(bytes.to_vec())).unwrap();
        assert_ne!(downloaded, original);
        assert_eq!(std::fs::read(&original).unwrap(), b"modified");
        assert_eq!(std::fs::read(&downloaded).unwrap(), bytes);
        assert_eq!(
            load_at(&synced, || anyhow::bail!(
                "Must reuse the replacement download"
            ))
            .unwrap(),
            downloaded
        );
        std::fs::remove_file(&downloaded).unwrap();
        assert_eq!(load_at(&synced, || Ok(bytes.to_vec())).unwrap(), downloaded);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saved_destinations_are_verified_and_edited_downloads_are_preserved() {
        let directory = directory();
        let message = Message {
            message_id: "received".into(),
            attachments: vec!["notes.txt".into()],
            ..Default::default()
        };
        let location = location(&directory, "me", "INBOX", &message, 0).unwrap();
        let cached = load_at(&location, || Ok(b"original".to_vec())).unwrap();
        let saved = directory.join("saved.txt");
        std::fs::copy(&cached, &saved).unwrap();
        let mut index = Index::read(&location.directory).unwrap();
        index.remember_copy(&location.identity, &saved).unwrap();
        index.save(&location.directory).unwrap();
        std::fs::remove_file(&cached).unwrap();
        assert_eq!(
            load_at(&location, || anyhow::bail!(
                "Must reuse the Save As destination"
            ))
            .unwrap(),
            saved
        );
        std::fs::write(&saved, b"modified").unwrap();
        assert!(index.remember_copy(&location.identity, &saved).is_err());
        let replacement = load_at(&location, || Ok(b"original".to_vec())).unwrap();
        assert_eq!(std::fs::read(&saved).unwrap(), b"modified");
        std::fs::write(&replacement, b"modified").unwrap();
        assert!(load_at(&location, || anyhow::bail!("Offline")).is_err());
        let fresh = load_at(&location, || Ok(b"original".to_vec())).unwrap();
        assert_ne!(fresh, replacement);
        assert_eq!(std::fs::read(&replacement).unwrap(), b"modified");
        assert_eq!(std::fs::read(&fresh).unwrap(), b"original");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
