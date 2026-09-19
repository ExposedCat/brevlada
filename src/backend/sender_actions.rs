use super::{mail::Mail, storage::Storage, worker::Event};
use crate::models::{Account, Message, sender_action::SenderAction, senders};
use anyhow::{Context, Result, ensure};
use imap::{Session, types::NameAttribute};
use std::io::{Read, Write};

pub fn execute(
    account: &Account,
    folder: &str,
    generation: u64,
    sender: &str,
    action: SenderAction,
    storage: &mut Storage,
    events: &async_channel::Sender<Event>,
) -> Result<()> {
    let mut mail = Mail::connect(account)?;
    let result = apply(&mut mail.session, folder, sender, action);
    if result.is_ok() {
        events.send_blocking(Event::SenderActionDone(generation, sender.into(), action))?;
    }
    let refresh = (|| -> Result<()> {
        let (validity, flags) = mail.inventory(folder)?;
        storage.inventory(&account.email, folder, validity, &flags)?;
        let (validity, headers, uids) = mail.headers(folder)?;
        storage.reconcile(&account.email, folder, validity, headers, &uids)?;
        events.send_blocking(Event::Messages(
            generation,
            storage.messages(&account.email, folder)?,
            false,
        ))?;
        events.send_blocking(Event::Unread(
            account.email.clone(),
            vec![(folder.into(), flags.iter().any(|flag| !flag.read))],
        ))?;
        Ok(())
    })();
    result?;
    refresh
}

fn apply<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    sender: &str,
    action: SenderAction,
) -> Result<()> {
    ensure!(!sender.is_empty(), "Sender has no email address");
    ensure!(
        !sender.chars().any(char::is_control),
        "Sender address contains invalid characters"
    );
    let validity = session
        .select(folder)?
        .uid_validity
        .context("Missing UIDVALIDITY")?;
    let escaped = sender.replace('\\', "\\\\").replace('"', "\\\"");
    let candidates: Vec<_> = session
        .uid_search(format!("FROM \"{escaped}\""))?
        .into_iter()
        .collect();
    let mut uids = Vec::new();
    for chunk in candidates.chunks(200) {
        let headers =
            session.uid_fetch(sequence(chunk), "(UID BODY.PEEK[HEADER.FIELDS (FROM)])")?;
        for header in headers.iter() {
            let parsed = mailparse::parse_mail(header.header().context("Missing sender header")?)?;
            use mailparse::MailHeaderMap;
            let message = Message {
                sender: parsed.headers.get_first_value("From").unwrap_or_default(),
                ..Default::default()
            };
            if senders::key(&message) == sender {
                uids.push(header.uid.context("Missing message UID")?);
            }
        }
    }
    if uids.is_empty() {
        return Ok(());
    }
    let destination = if action == SenderAction::MarkRead {
        None
    } else {
        Some(destination(session, action)?)
    };
    let (can_move, can_expunge) = if destination.is_some() {
        let capabilities = session.capabilities()?;
        (
            capabilities.has_str("MOVE"),
            capabilities.has_str("UIDPLUS"),
        )
    } else {
        (false, false)
    };
    if let Some(destination) = &destination {
        if destination == folder && action == SenderAction::Archive {
            return Ok(());
        }
        ensure!(
            (can_move && destination != folder) || can_expunge,
            "Mail server needs MOVE or UIDPLUS support to safely remove these messages"
        );
    }
    ensure!(
        session.select(folder)?.uid_validity == Some(validity),
        "Mailbox changed during sender action; please retry"
    );
    for chunk in uids.chunks(200) {
        let sequence = sequence(chunk);
        if action == SenderAction::MarkRead {
            session.uid_store(&sequence, "+FLAGS.SILENT (\\Seen)")?;
            continue;
        }
        if action == SenderAction::Spam {
            session.uid_store(&sequence, "+FLAGS.SILENT ($Junk)")?;
        }
        let destination = destination.as_ref().unwrap();
        if destination != folder && can_move {
            session.uid_mv(&sequence, destination)?;
        } else {
            if destination != folder {
                session.uid_copy(&sequence, destination)?;
            }
            session.uid_store(&sequence, "+FLAGS.SILENT (\\Deleted)")?;
            session.uid_expunge(&sequence)?;
        }
    }
    Ok(())
}

fn sequence(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn destination<T: Read + Write>(session: &mut Session<T>, action: SenderAction) -> Result<String> {
    let folders = session.list(None, Some("*"))?;
    let special = if action == SenderAction::Archive {
        "\\Archive"
    } else {
        "\\Trash"
    };
    let selectable: Vec<_> = folders
        .iter()
        .filter(|folder| !folder.attributes().contains(&NameAttribute::NoSelect))
        .collect();
    let marked = |attribute: &str| {
        selectable
            .iter()
            .find(|folder| {
                folder.attributes().iter().any(|value| {
            matches!(value, NameAttribute::Custom(value) if value.eq_ignore_ascii_case(attribute))
        })
            })
            .map(|folder| folder.name().to_owned())
    };
    if let Some(name) = marked(special) {
        return Ok(name);
    }
    if action == SenderAction::Archive
        && let Some(name) = marked("\\All")
    {
        return Ok(name);
    }
    for folder in selectable {
        let leaf = folder
            .delimiter()
            .and_then(|delimiter| folder.name().rsplit(delimiter).next())
            .unwrap_or(folder.name());
        if fallback_folder(leaf, action) {
            return Ok(folder.name().to_owned());
        }
    }
    anyhow::bail!(
        "Could not find the account's {} folder",
        if action == SenderAction::Archive {
            "Archive"
        } else {
            "Trash"
        }
    )
}

fn fallback_folder(name: &str, action: SenderAction) -> bool {
    let name = name.to_lowercase();
    if action == SenderAction::Archive {
        matches!(name.as_str(), "archive" | "archives" | "all mail")
    } else {
        matches!(
            name.as_str(),
            "trash" | "bin" | "deleted items" | "deleted messages"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Debug)]
    struct MockStream {
        responses: Cursor<Vec<u8>>,
        commands: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }
    impl Read for MockStream {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.responses.read(buffer)
        }
    }
    impl Write for MockStream {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.commands.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn marks_only_exact_sender_matches_including_older_messages() {
        let from = "From: Alice <alice@example.com>\r\n\r\n";
        let other = "From: Other <not-alice@example.com>\r\n\r\n";
        let responses = format!(
            "a1 OK login\r\n* OK [UIDVALIDITY 7] valid\r\na2 OK select\r\n\
             * SEARCH 1 90\r\na3 OK search\r\n\
             * 1 FETCH (UID 1 BODY[HEADER.FIELDS (FROM)] {{{}}}\r\n{})\r\n\
             * 2 FETCH (UID 90 BODY[HEADER.FIELDS (FROM)] {{{}}}\r\n{})\r\n\
             a4 OK fetch\r\n* OK [UIDVALIDITY 7] valid\r\na5 OK select\r\na6 OK store\r\n",
            from.len(),
            from,
            other.len(),
            other
        );
        let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = imap::Client::new(MockStream {
            responses: Cursor::new(responses.into_bytes()),
            commands: commands.clone(),
        })
        .login("test", "test")
        .unwrap();
        apply(
            &mut session,
            "INBOX",
            "alice@example.com",
            SenderAction::MarkRead,
        )
        .unwrap();
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(
            sent.contains("UID STORE 1 +FLAGS.SILENT (\\Seen)"),
            "{sent}"
        );
        assert!(!sent.contains("UID STORE 90"));
    }

    #[test]
    fn moves_to_special_use_folders_and_marks_spam_before_deleting() {
        for action in [
            SenderAction::Archive,
            SenderAction::Delete,
            SenderAction::Spam,
        ] {
            let from = "From: Alice <alice@example.com>\r\n\r\n";
            let special = if action == SenderAction::Archive {
                "\\Archive"
            } else {
                "\\Trash"
            };
            let responses = format!(
                "a1 OK login\r\n* OK [UIDVALIDITY 7] valid\r\na2 OK select\r\n\
                 * SEARCH 1\r\na3 OK search\r\n\
                 * 1 FETCH (UID 1 BODY[HEADER.FIELDS (FROM)] {{{}}}\r\n{})\r\na4 OK fetch\r\n\
                 * LIST ({special}) \"/\" \"Localized\"\r\na5 OK list\r\n\
                 * CAPABILITY IMAP4rev1 MOVE\r\na6 OK capability\r\n\
                 * OK [UIDVALIDITY 7] valid\r\na7 OK select\r\na8 OK mutation\r\na9 OK mutation\r\n",
                from.len(),
                from
            );
            let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut session = imap::Client::new(MockStream {
                responses: Cursor::new(responses.into_bytes()),
                commands: commands.clone(),
            })
            .login("test", "test")
            .unwrap();
            apply(&mut session, "INBOX", "alice@example.com", action).unwrap();
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(sent.contains("UID MOVE 1 \"Localized\""), "{sent}");
            assert!(!sent.contains("EXPUNGE"));
            if action == SenderAction::Spam {
                assert!(
                    sent.find("+FLAGS.SILENT ($Junk)").unwrap() < sent.find("UID MOVE").unwrap()
                );
            } else {
                assert!(!sent.contains("$Junk"));
            }
        }
    }

    #[test]
    fn distinguishes_archive_and_trash_names() {
        assert!(fallback_folder("All Mail", SenderAction::Archive));
        assert!(fallback_folder("Deleted Items", SenderAction::Spam));
        assert!(!fallback_folder("Junk", SenderAction::Delete));
        assert!(!fallback_folder("Trash", SenderAction::Archive));
    }
}
