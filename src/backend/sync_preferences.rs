use super::{connections::Connections, storage::Storage, sync_queue::SyncQueue, worker::Event};
use crate::models::{Account, sync::Folders};
use anyhow::{Result, ensure};

pub(super) fn load(
    account: &Account,
    storage: &mut Storage,
    connections: &mut Connections,
    events: &async_channel::Sender<Event>,
) -> Result<Folders> {
    let discovered = connections.execute(account, |mail| mail.sync_folders())?;
    storage.store_folders(&account.email, &discovered.all)?;
    events.send_blocking(Event::Folders(account.email.clone(), discovered.all))?;
    let canonical = |folders: Vec<String>| {
        folders
            .into_iter()
            .map(|folder| {
                if folder.eq_ignore_ascii_case("INBOX") {
                    "INBOX".into()
                } else {
                    folder
                }
            })
            .collect()
    };
    Ok(Folders {
        folders: canonical(discovered.sync),
        selected: canonical(storage.sync_folders(&account.email)?),
    })
}

pub(super) fn save(
    account: &Account,
    mut folders: Vec<String>,
    storage: &Storage,
    queue: &SyncQueue,
) -> Result<Vec<String>> {
    let available = storage.folders(&account.email)?;
    let previous = storage.sync_folders(&account.email)?;
    for folder in &mut folders {
        if folder.eq_ignore_ascii_case("INBOX") {
            *folder = "INBOX".into();
        }
        ensure!(
            available.iter().chain(&previous).any(|known| {
                known == folder || (known.eq_ignore_ascii_case("INBOX") && folder == "INBOX")
            }),
            "A selected folder is no longer available. Reload the folder list and choose again."
        );
    }
    folders.sort();
    folders.dedup();
    storage.set_sync_folders(&account.email, &folders)?;
    queue.set_folders(&account.email, folders.clone());
    queue.refresh();
    Ok(folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_choices_update_live_sync_and_keep_accounts_and_cache_separate() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let account = Account {
            email: "alice@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        storage
            .store_folders(&account.email, &["INBOX".into(), "Archive".into()])
            .unwrap();
        storage
            .store(
                &account.email,
                "Archive",
                &crate::models::Message {
                    uid: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        let queue = SyncQueue::default();
        queue.register(account.clone(), false);
        let saved = save(
            &account,
            vec!["inbox".into(), "Archive".into(), "Archive".into()],
            &storage,
            &queue,
        )
        .unwrap();
        assert_eq!(saved, ["Archive", "INBOX"]);
        assert_eq!(storage.sync_folders(&account.email).unwrap(), saved);
        assert!(!queue.headers_only(&account.email, "Archive"));
        assert!(queue.queued_len() > 0);
        assert_eq!(storage.sync_folders("work@example.com").unwrap(), ["INBOX"]);
        assert!(save(&account, vec!["Missing".into()], &storage, &queue).is_err());
        assert_eq!(storage.sync_folders(&account.email).unwrap(), saved);
        assert!(!queue.headers_only(&account.email, "Archive"));
        save(&account, Vec::new(), &storage, &queue).unwrap();
        assert!(storage.sync_folders(&account.email).unwrap().is_empty());
        assert!(queue.headers_only(&account.email, "Archive"));
        assert!(
            storage
                .message(&account.email, "Archive", 1)
                .unwrap()
                .is_some()
        );
        queue.close();
    }
}
