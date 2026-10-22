use super::{mail::Mail, sender_actions, special_use};
use crate::models::{
    Account,
    sender_action::SenderAction,
    settings::{AccountFolders, Settings},
};
use anyhow::{Result, ensure};
use imap::{Session, types::NameAttribute};
use std::{
    io::{Read, Write},
    path::Path,
};

pub(super) fn load(account: &Account, path: &Path) -> Result<AccountFolders> {
    let mut mail = Mail::connect(account)?;
    inspect(&mut mail.session, account, path)
}

pub(super) fn save(
    account: &Account,
    path: &Path,
    folder: Option<String>,
    share: bool,
) -> Result<AccountFolders> {
    let mut mail = Mail::connect(account)?;
    configure(&mut mail.session, account, path, folder, share)
}

pub(super) fn archive_action(
    settings: &Settings,
    account: &str,
    action: SenderAction,
) -> SenderAction {
    if action == SenderAction::Archive {
        settings
            .accounts
            .get(account)
            .and_then(|settings| settings.archive_folder.clone())
            .map(SenderAction::MoveTo)
            .unwrap_or(action)
    } else {
        action
    }
}

fn inspect<T: Read + Write>(
    session: &mut Session<T>,
    account: &Account,
    path: &Path,
) -> Result<AccountFolders> {
    let can_share = special_use::can_share(session)?;
    let listed = session.list(None, Some("*"))?;
    let mut folders: Vec<_> = listed
        .iter()
        .filter(|name| {
            !name.attributes().contains(&NameAttribute::NoSelect)
                && !name.name().eq_ignore_ascii_case("INBOX")
        })
        .map(|name| name.name().to_owned())
        .collect();
    folders.sort();
    let server_archive = sender_actions::listed_destination(&listed, &SenderAction::Archive);
    let settings = Settings::load(path)?;
    Ok(AccountFolders {
        folders,
        archive_folder: settings
            .accounts
            .get(&account.email)
            .and_then(|settings| settings.archive_folder.clone()),
        server_archive,
        can_share,
    })
}

fn configure<T: Read + Write>(
    session: &mut Session<T>,
    account: &Account,
    path: &Path,
    folder: Option<String>,
    share: bool,
) -> Result<AccountFolders> {
    let mut current = inspect(session, account, path)?;
    if let Some(folder) = &folder {
        ensure!(
            current.folders.contains(folder),
            "The selected archive folder is no longer available. Reload account settings and choose again"
        );
    }
    if share {
        let folder = folder.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Choose an archive folder before sharing it with other mail apps")
        })?;
        special_use::share_archive(session, folder)?;
        current.server_archive = Some(folder.to_owned());
    }
    current.archive_folder = folder.clone();
    Settings::update(path, |settings| {
        if let Some(folder) = folder {
            settings
                .accounts
                .entry(account.email.clone())
                .or_default()
                .archive_folder = Some(folder);
        } else {
            settings.accounts.remove(&account.email);
        }
    })
    .map_err(|error| {
        if share {
            anyhow::anyhow!(
                "The server was updated, but Brevlada could not save its preference: {error}"
            )
        } else {
            error
        }
    })?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::special_use::tests::session;

    fn account() -> Account {
        Account {
            path: String::new(),
            email: "me@example.com".into(),
            name: "Me".into(),
            host: "fixture".into(),
            username: "me".into(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        }
    }

    fn directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brevlada-account-settings-{}",
            gtk::glib::uuid_string_random()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    const FOLDERS: &str = "* LIST () \"/\" INBOX\r\n* LIST () \"/\" Archive\r\n* LIST (\\All) \"/\" \"[Gmail]/All Mail\"\r\n* LIST (\\Noselect) \"/\" Parent\r\n";

    #[test]
    fn local_archive_choices_persist_per_account_and_automatic_restores_server_selection() {
        let directory = directory();
        let path = directory.join("settings.json");
        Settings::update(&path, |settings| {
            settings.trust_all_senders = true;
            settings.start_at_login = false;
            settings
                .accounts
                .entry("other@example.com".into())
                .or_default()
                .archive_folder = Some("Saved".into());
        })
        .unwrap();
        for folder in [Some("Archive".to_owned()), None] {
            let (mut session, commands) = session(&format!(
                "* CAPABILITY IMAP4rev1 SPECIAL-USE\r\na2 OK caps\r\n{FOLDERS}a3 OK list\r\n"
            ));
            let config = configure(&mut session, &account(), &path, folder.clone(), false).unwrap();
            assert_eq!(config.archive_folder, folder);
            assert_eq!(config.server_archive.as_deref(), Some("[Gmail]/All Mail"));
            assert!(!config.can_share);
            assert!(
                !config
                    .folders
                    .iter()
                    .any(|folder| folder == "INBOX" || folder == "Parent")
            );
            let settings = Settings::load(&path).unwrap();
            assert!(settings.trust_all_senders && !settings.start_at_login);
            assert_eq!(
                settings.accounts["other@example.com"]
                    .archive_folder
                    .as_deref(),
                Some("Saved")
            );
            assert_eq!(
                archive_action(&settings, "other@example.com", SenderAction::Archive),
                SenderAction::MoveTo("Saved".into())
            );
            assert_eq!(
                archive_action(&settings, "me@example.com", SenderAction::Archive),
                folder
                    .map(SenderAction::MoveTo)
                    .unwrap_or(SenderAction::Archive)
            );
            assert_eq!(
                archive_action(&settings, "other@example.com", SenderAction::Delete),
                SenderAction::Delete
            );
            assert!(
                !String::from_utf8(commands.lock().unwrap().clone())
                    .unwrap()
                    .contains("SETMETADATA")
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_or_unshareable_choices_leave_saved_settings_untouched() {
        let directory = directory();
        let path = directory.join("settings.json");
        Settings::update(&path, |settings| {
            settings
                .accounts
                .entry(account().email)
                .or_default()
                .archive_folder = Some("Saved".into());
        })
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        for (folder, share, tail) in [
            (Some("Missing".into()), false, ""),
            (Some("INBOX".into()), false, ""),
            (None, true, ""),
            (
                Some("Archive".into()),
                true,
                "* CAPABILITY IMAP4rev1 SPECIAL-USE\r\na4 OK caps\r\n",
            ),
        ] {
            let (mut session, commands) = session(&format!(
                "* CAPABILITY IMAP4rev1 SPECIAL-USE\r\na2 OK caps\r\n{FOLDERS}a3 OK list\r\n{tail}"
            ));
            assert!(configure(&mut session, &account(), &path, folder, share).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(
                !String::from_utf8(commands.lock().unwrap().clone())
                    .unwrap()
                    .contains("SETMETADATA")
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn shared_choice_is_only_saved_after_the_server_accepts_and_confirms_it() {
        let directory = directory();
        let path = directory.join("settings.json");
        for accepted in [false, true] {
            let tail = if accepted {
                "a6 OK saved\r\n* LIST (\\Archive) \"/\" Archive\r\na7 OK list\r\n* LIST (\\Archive) \"/\" Archive\r\na8 OK list\r\n"
            } else {
                "a6 NO not permitted\r\n"
            };
            let (mut session, _) = session(&format!(
                "* CAPABILITY IMAP4rev1 SPECIAL-USE METADATA\r\na2 OK caps\r\n{FOLDERS}a3 OK list\r\n* CAPABILITY IMAP4rev1 SPECIAL-USE METADATA\r\na4 OK caps\r\n* LIST () \"/\" Archive\r\na5 OK list\r\n{tail}"
            ));
            let result = configure(
                &mut session,
                &account(),
                &path,
                Some("Archive".into()),
                true,
            );
            if accepted {
                let config = result.unwrap();
                assert_eq!(config.server_archive.as_deref(), Some("Archive"));
                assert_eq!(config.archive_folder.as_deref(), Some("Archive"));
                assert_eq!(
                    Settings::load(&path).unwrap().accounts["me@example.com"]
                        .archive_folder
                        .as_deref(),
                    Some("Archive")
                );
            } else {
                assert!(result.is_err());
                assert!(!path.exists());
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
