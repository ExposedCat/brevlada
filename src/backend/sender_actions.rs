use super::{mail::Mail, storage::Storage, worker::Event};
use crate::models::{Account, Message, sender_action::SenderAction, senders};
use anyhow::{Context, Result, ensure};
use imap::{Session, types::NameAttribute};
use std::io::{Read, Write};

pub struct Outcome {
    pub messages: Option<Vec<Message>>,
    pub error: Option<String>,
    pub removed: Vec<Message>,
}

pub fn execute(
    account: &Account,
    folder: &str,
    target: &crate::models::action_target::ActionTarget,
    action: SenderAction,
    storage: &mut Storage,
    events: &async_channel::Sender<Event>,
) -> Result<Outcome> {
    let mut mail = Mail::connect(account)?;
    let result = match target {
        crate::models::action_target::ActionTarget::Sender(sender) => {
            apply(&mut mail.session, folder, sender, action.clone())
        }
        crate::models::action_target::ActionTarget::Messages(messages) => apply_messages(
            &mut mail,
            folder,
            storage.validity(&account.email, folder)?,
            messages,
            action.clone(),
        ),
    };
    let mut removed = Vec::new();
    let mut error = None;
    let applied = result.is_ok();
    match result {
        Ok(messages) => {
            if action.removes_messages() {
                removed = messages.clone();
            }
            if let Err(failure) = storage.confirm_action(&account.email, folder, &messages, action)
            {
                error = Some(format!(
                    "Action completed, but the mail cache could not update: {failure}"
                ));
            }
        }
        Err(failure) => error = Some(failure.to_string()),
    }
    let refresh = (|| -> Result<Vec<Message>> {
        let revision = storage.read_revision()?;
        let (validity, flags) = mail.inventory(folder)?;
        storage.inventory_since(&account.email, folder, validity, &flags, revision)?;
        events.send_blocking(Event::Unread(
            account.email.clone(),
            vec![(folder.into(), flags.iter().any(|flag| !flag.read))],
        ))?;
        storage.sender_headers(&account.email, folder)
    })();
    let messages = match refresh {
        Ok(messages) => Some(messages),
        Err(failure) => {
            if error.is_none() {
                error = Some(if applied {
                    format!("Action completed, but the message list could not refresh: {failure}")
                } else {
                    failure.to_string()
                });
            }
            storage.sender_headers(&account.email, folder).ok()
        }
    };
    Ok(Outcome {
        messages,
        error,
        removed,
    })
}

pub struct UndoOutcome {
    pub messages: Option<Vec<Message>>,
    pub restored: Vec<Message>,
    pub error: Option<String>,
}

pub fn undo_spam(
    account: &Account,
    folder: &str,
    originals: &[Message],
    storage: &mut Storage,
    events: &async_channel::Sender<Event>,
) -> Result<UndoOutcome> {
    let mut mail = Mail::connect(account)?;
    let spam = destination(&mut mail.session, &SenderAction::Spam)?;
    let mut restored = Vec::new();
    let mut error = None;
    for original in originals {
        let result = undo_message(&mut mail.session, &spam, folder, original);
        match result {
            Ok(moved) => {
                restored.push(original.clone());
                if let Err(failure) = storage.confirm_action(
                    &account.email,
                    &spam,
                    &[moved],
                    SenderAction::MoveTo(folder.into()),
                ) {
                    error = Some(format!(
                        "Message restored, but the mail cache could not update: {failure}"
                    ));
                }
            }
            Err(failure) => {
                error = Some(failure.to_string());
                break;
            }
        }
    }
    let refresh = (|| -> Result<Vec<Message>> {
        let revision = storage.read_revision()?;
        let (validity, flags) = mail.inventory(folder)?;
        let missing =
            storage.inventory_since(&account.email, folder, validity, &flags, revision)?;
        for chunk in missing.chunks(200) {
            let headers = mail.header_batch(folder, validity, chunk)?;
            storage.store_headers(&account.email, folder, validity, &headers)?;
        }
        events.send_blocking(Event::Unread(
            account.email.clone(),
            storage.unread(&account.email)?,
        ))?;
        storage.sender_headers(&account.email, folder)
    })();
    let messages = match refresh {
        Ok(messages) => Some(messages),
        Err(failure) => {
            error.get_or_insert_with(|| format!("Could not refresh restored messages: {failure}"));
            storage.sender_headers(&account.email, folder).ok()
        }
    };
    Ok(UndoOutcome {
        messages,
        restored,
        error,
    })
}

fn undo_message<T: Read + Write>(
    session: &mut Session<T>,
    spam: &str,
    destination: &str,
    original: &Message,
) -> Result<Message> {
    let query = if !original.message_id.is_empty() {
        format!("HEADER Message-ID {}", quoted(&original.message_id)?)
    } else {
        let sender = senders::key(original);
        ensure!(
            !sender.is_empty(),
            "Could not identify this message in Spam"
        );
        format!("FROM {}", quoted(&sender)?)
    };
    let validity = session
        .select(spam)?
        .uid_validity
        .context("Missing Spam UIDVALIDITY")?;
    let uids: Vec<_> = session.uid_search(query)?.into_iter().collect();
    let mut matches = Vec::new();
    for chunk in uids.chunks(200) {
        matches.extend(
            super::mail_headers::fetch(session, spam, None, validity, chunk)?
                .into_iter()
                .filter(|message| {
                    message.message_id == original.message_id
                        && senders::key(message) == senders::key(original)
                        && message.subject == original.subject
                        && message.timestamp == original.timestamp
                }),
        );
    }
    ensure!(
        matches.len() == 1,
        "Could not uniquely identify this message in Spam; open the Spam folder to restore it"
    );
    let message = matches.pop().unwrap();
    let sequence = message.uid.to_string();
    ensure!(
        session.select(spam)?.uid_validity == Some(validity),
        "Spam mailbox changed; refresh and retry"
    );
    session.uid_store(&sequence, "-FLAGS.SILENT ($Junk)")?;
    session.uid_store(&sequence, "+FLAGS.SILENT ($NotJunk)")?;
    ensure!(
        apply_uids(
            session,
            spam,
            validity,
            &[message.uid],
            &SenderAction::MoveTo(destination.into())
        )?,
        "Message is already in the destination folder"
    );
    Ok(message)
}

fn apply<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    sender: &str,
    action: SenderAction,
) -> Result<Vec<Message>> {
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
    let mut messages = Vec::new();
    for chunk in candidates.chunks(200) {
        messages.extend(
            super::mail_headers::fetch(session, folder, None, validity, chunk)?
                .into_iter()
                .filter(|message| senders::key(message) == sender),
        );
    }
    let uids: Vec<_> = messages.iter().map(|message| message.uid).collect();
    if !apply_uids(session, folder, validity, &uids, &action)? {
        messages.clear();
    }
    Ok(messages)
}

fn apply_messages(
    mail: &mut Mail,
    folder: &str,
    validity: Option<u32>,
    messages: &[(u32, String)],
    action: SenderAction,
) -> Result<Vec<Message>> {
    let validity = validity.context("Mailbox is not ready; refresh and retry")?;
    let uids: Vec<_> = messages.iter().map(|(uid, _)| *uid).collect();
    let mut verified = Vec::new();
    for chunk in uids.chunks(200) {
        for header in mail.header_batch(folder, validity, chunk)? {
            ensure!(
                messages
                    .iter()
                    .any(|(uid, id)| *uid == header.uid && *id == header.message_id),
                "Message changed on the server; refresh and retry"
            );
            verified.push(header);
        }
    }
    let uids: Vec<_> = verified.iter().map(|message| message.uid).collect();
    if !apply_uids(&mut mail.session, folder, validity, &uids, &action)? {
        verified.clear();
    }
    Ok(verified)
}

fn apply_uids<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    validity: u32,
    uids: &[u32],
    action: &SenderAction,
) -> Result<bool> {
    if uids.is_empty() {
        return Ok(false);
    }
    let destination = if !action.removes_messages() {
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
        if destination == folder && !matches!(action, SenderAction::Delete) {
            return Ok(false);
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
        if let Some(read) = action.read_state() {
            session.uid_store(
                &sequence,
                if read {
                    "+FLAGS.SILENT (\\Seen)"
                } else {
                    "-FLAGS.SILENT (\\Seen)"
                },
            )?;
            continue;
        }
        if let Some(flagged) = action.flagged_state() {
            session.uid_store(
                &sequence,
                if flagged {
                    "+FLAGS.SILENT (\\Flagged)"
                } else {
                    "-FLAGS.SILENT (\\Flagged)"
                },
            )?;
            continue;
        }
        if *action == SenderAction::Unspam {
            session.uid_store(&sequence, "-FLAGS.SILENT ($Junk)")?;
            session.uid_store(&sequence, "+FLAGS.SILENT ($NotJunk)")?;
        }
        if *action == SenderAction::Spam {
            session.uid_store(&sequence, "-FLAGS.SILENT ($NotJunk)")?;
            session.uid_store(&sequence, "+FLAGS.SILENT ($Junk)")?;
        }
        let destination = destination.as_ref().unwrap();
        if destination != folder && can_move {
            session.uid_mv(&sequence, destination)?;
        } else {
            if destination != folder {
                session.uid_copy(&sequence, quoted(destination)?)?;
            }
            session.uid_store(&sequence, "+FLAGS.SILENT (\\Deleted)")?;
            session.uid_expunge(&sequence)?;
        }
    }
    Ok(true)
}

fn quoted(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "Message headers contain invalid characters"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn sequence(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn destination<T: Read + Write>(session: &mut Session<T>, action: &SenderAction) -> Result<String> {
    if matches!(action, SenderAction::Restore | SenderAction::Unspam) {
        return Ok("INBOX".into());
    }
    let folders = session.list(None, Some("*"))?;
    if let SenderAction::MoveTo(destination) = action {
        ensure!(
            folders.iter().any(|folder| folder.name() == destination
                && !folder.attributes().contains(&NameAttribute::NoSelect)),
            "Destination folder is not selectable; refresh and retry"
        );
        return Ok(destination.clone());
    }
    let special = match action {
        SenderAction::Archive => "\\Archive",
        SenderAction::Spam => "\\Junk",
        _ => "\\Trash",
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
    if *action == SenderAction::Archive
        && let Some(name) = marked("\\All")
    {
        return Ok(name);
    }
    for folder in selectable {
        let leaf = folder
            .delimiter()
            .and_then(|delimiter| folder.name().rsplit(delimiter).next())
            .unwrap_or(folder.name());
        if fallback_folder(leaf, action.clone()) {
            return Ok(folder.name().to_owned());
        }
    }
    anyhow::bail!(
        "Could not find the account's {} folder",
        match action {
            SenderAction::Archive => "Archive",
            SenderAction::Spam => "Spam",
            _ => "Trash",
        }
    )
}

fn fallback_folder(name: &str, action: SenderAction) -> bool {
    let name = name.to_lowercase();
    if action == SenderAction::Archive {
        matches!(name.as_str(), "archive" | "archives" | "all mail")
    } else if action == SenderAction::Spam {
        matches!(
            name.as_str(),
            "spam" | "junk" | "junk mail" | "junk e-mail" | "bulk" | "bulk mail"
        )
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
             * 1 FETCH (UID 1 BODY[HEADER] {{{}}}\r\n{})\r\n\
             * 2 FETCH (UID 90 BODY[HEADER] {{{}}}\r\n{})\r\n\
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
    fn moves_to_special_use_folders_and_marks_spam_before_moving() {
        for action in [
            SenderAction::Archive,
            SenderAction::Delete,
            SenderAction::Spam,
        ] {
            let from = "From: Alice <alice@example.com>\r\n\r\n";
            let special = match action {
                SenderAction::Archive => "\\Archive",
                SenderAction::Spam => "\\Junk",
                _ => "\\Trash",
            };
            let responses = format!(
                "a1 OK login\r\n* OK [UIDVALIDITY 7] valid\r\na2 OK select\r\n\
                 * SEARCH 1\r\na3 OK search\r\n\
                 * 1 FETCH (UID 1 BODY[HEADER] {{{}}}\r\n{})\r\na4 OK fetch\r\n\
                 * LIST ({special}) \"/\" \"Localized\"\r\na5 OK list\r\n\
                 * CAPABILITY IMAP4rev1 MOVE\r\na6 OK capability\r\n\
                 * OK [UIDVALIDITY 7] valid\r\na7 OK select\r\na8 OK mutation\r\na9 OK mutation\r\na10 OK mutation\r\n",
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
            apply(&mut session, "INBOX", "alice@example.com", action.clone()).unwrap();
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

    fn session(
        responses: String,
    ) -> (
        Session<MockStream>,
        std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    ) {
        let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let session = imap::Client::new(MockStream {
            responses: Cursor::new(responses.into_bytes()),
            commands: commands.clone(),
        })
        .login("test", "test")
        .unwrap();
        (session, commands)
    }

    #[test]
    fn unread_and_stars_only_change_the_requested_flag() {
        for (action, flag) in [
            (SenderAction::MarkUnread, "-FLAGS.SILENT (\\Seen)"),
            (SenderAction::Star, "+FLAGS.SILENT (\\Flagged)"),
            (SenderAction::Unstar, "-FLAGS.SILENT (\\Flagged)"),
        ] {
            let (mut session, commands) = session(
                "a1 OK login\r\n* OK [UIDVALIDITY 7] valid\r\na2 OK select\r\na3 OK store\r\n"
                    .into(),
            );
            assert!(apply_uids(&mut session, "INBOX", 7, &[1, 2], &action).unwrap());
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(sent.contains(&format!("UID STORE 1,2 {flag}")), "{sent}");
            assert!(
                !sent.contains("MOVE") && !sent.contains("COPY") && !sent.contains("EXPUNGE"),
                "{sent}"
            );
        }
    }

    #[test]
    fn restore_and_unspam_move_to_inbox_and_unspam_clears_junk() {
        for action in [SenderAction::Restore, SenderAction::Unspam] {
            let (mut session, commands) = session(
                "a1 OK login\r\n* CAPABILITY IMAP4rev1 MOVE\r\na2 OK capabilities\r\n\
                 * OK [UIDVALIDITY 7] valid\r\na3 OK select\r\na4 OK mutation\r\na5 OK mutation\r\na6 OK mutation\r\n".into()
            );
            assert!(apply_uids(&mut session, "Trash", 7, &[8], &action).unwrap());
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(sent.contains("UID MOVE 8 \"INBOX\""), "{sent}");
            if action == SenderAction::Unspam {
                assert!(
                    sent.find("-FLAGS.SILENT ($Junk)").unwrap()
                        < sent.find("+FLAGS.SILENT ($NotJunk)").unwrap()
                );
                assert!(
                    sent.find("+FLAGS.SILENT ($NotJunk)").unwrap() < sent.find("UID MOVE").unwrap()
                );
            } else {
                assert!(!sent.contains("$Junk") && !sent.contains("$NotJunk"));
            }
        }
    }

    #[test]
    fn move_to_validates_the_folder_and_uses_a_scoped_uidplus_fallback() {
        let (mut session, commands) = session(
            "a1 OK login\r\n* LIST () \"/\" \"Work\"\r\na2 OK list\r\n\
             * CAPABILITY IMAP4rev1 UIDPLUS\r\na3 OK capabilities\r\n\
             * OK [UIDVALIDITY 7] valid\r\na4 OK select\r\na5 OK copy\r\na6 OK store\r\na7 OK expunge\r\n".into()
        );
        assert!(
            apply_uids(
                &mut session,
                "INBOX",
                7,
                &[2],
                &SenderAction::MoveTo("Work".into())
            )
            .unwrap()
        );
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(sent.contains("UID COPY 2 \"Work\""), "{sent}");
        assert!(
            sent.contains("UID STORE 2 +FLAGS.SILENT (\\Deleted)"),
            "{sent}"
        );
        assert!(sent.contains("UID EXPUNGE 2"), "{sent}");

        let (mut session, commands) = self::session(
            "a1 OK login\r\n* LIST (\\Noselect) \"/\" \"Work\"\r\na2 OK list\r\n".into(),
        );
        assert!(
            apply_uids(
                &mut session,
                "INBOX",
                7,
                &[2],
                &SenderAction::MoveTo("Work".into())
            )
            .is_err()
        );
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(!sent.contains("UID COPY") && !sent.contains("UID STORE"));
    }

    #[test]
    fn undo_spam_uses_the_new_spam_uid_and_restores_the_original_folder() {
        let header =
            "From: Alice <alice@example.com>\r\nSubject: Plans\r\nMessage-ID: <one>\r\n\r\n";
        let original = super::super::parser::parse(1, header.as_bytes(), true, false).unwrap();
        let (mut session, commands) = session(format!(
            "a1 OK login\r\n* OK [UIDVALIDITY 12] valid\r\na2 OK select\r\n\
             * SEARCH 91\r\na3 OK search\r\n* 1 FETCH (UID 91 BODY[HEADER] {{{}}}\r\n{})\r\na4 OK fetch\r\n\
             * OK [UIDVALIDITY 12] valid\r\na5 OK select\r\na6 OK store\r\na7 OK store\r\n\
             * LIST () \"/\" \"Work\"\r\na8 OK list\r\n\
             * CAPABILITY IMAP4rev1 MOVE\r\na9 OK capability\r\n\
             * OK [UIDVALIDITY 12] valid\r\na10 OK select\r\na11 OK move\r\n",
            header.len(),
            header
        ));
        let moved = undo_message(&mut session, "Junk", "Work", &original).unwrap();
        assert_eq!(moved.uid, 91);
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(
            sent.contains("UID SEARCH HEADER Message-ID \"one\""),
            "{sent}"
        );
        assert!(
            sent.contains("UID STORE 91 -FLAGS.SILENT ($Junk)"),
            "{sent}"
        );
        assert!(
            sent.contains("UID STORE 91 +FLAGS.SILENT ($NotJunk)"),
            "{sent}"
        );
        assert!(sent.contains("UID MOVE 91 \"Work\""), "{sent}");
        assert!(!sent.contains("UID MOVE 1 ") && !sent.contains("EXPUNGE"));
    }

    #[test]
    fn undo_spam_refuses_ambiguous_matches_and_changed_mailboxes() {
        let header =
            "From: Alice <alice@example.com>\r\nSubject: Plans\r\nMessage-ID: <one>\r\n\r\n";
        let original = super::super::parser::parse(1, header.as_bytes(), true, false).unwrap();
        let (mut session, commands) = session(format!(
            "a1 OK login\r\n* OK [UIDVALIDITY 12] valid\r\na2 OK select\r\n\
             * SEARCH 91 92\r\na3 OK search\r\n\
             * 1 FETCH (UID 91 BODY[HEADER] {{{}}}\r\n{})\r\n\
             * 2 FETCH (UID 92 BODY[HEADER] {{{}}}\r\n{})\r\na4 OK fetch\r\n",
            header.len(),
            header,
            header.len(),
            header
        ));
        assert!(undo_message(&mut session, "Junk", "Work", &original).is_err());
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(!sent.contains("UID STORE") && !sent.contains("UID MOVE"));
        let (mut session, commands) = self::session(format!(
            "a1 OK login\r\n* OK [UIDVALIDITY 12] valid\r\na2 OK select\r\n\
             * SEARCH 91\r\na3 OK search\r\n* 1 FETCH (UID 91 BODY[HEADER] {{{}}}\r\n{})\r\na4 OK fetch\r\n\
             * OK [UIDVALIDITY 13] changed\r\na5 OK select\r\n",
            header.len(),
            header
        ));
        assert!(undo_message(&mut session, "Junk", "Work", &original).is_err());
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(!sent.contains("UID STORE") && !sent.contains("UID MOVE"));
    }

    #[test]
    fn spam_does_not_fall_back_to_trash_or_expunge_existing_spam() {
        let (mut session, commands) =
            session("a1 OK login\r\n* LIST (\\Trash) \"/\" \"Trash\"\r\na2 OK list\r\n".into());
        assert!(apply_uids(&mut session, "INBOX", 7, &[1], &SenderAction::Spam).is_err());
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(!sent.contains("UID STORE") && !sent.contains("UID MOVE"));
        let (mut session, commands) = self::session(
            "a1 OK login\r\n* LIST (\\Junk) \"/\" \"Spam\"\r\na2 OK list\r\n\
             * CAPABILITY IMAP4rev1 MOVE UIDPLUS\r\na3 OK capabilities\r\n"
                .into(),
        );
        assert!(!apply_uids(&mut session, "Spam", 7, &[1], &SenderAction::Spam).unwrap());
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(!sent.contains("UID STORE") && !sent.contains("EXPUNGE"));
    }

    #[test]
    fn distinguishes_archive_and_trash_names() {
        assert!(fallback_folder("All Mail", SenderAction::Archive));
        assert!(fallback_folder("Junk", SenderAction::Spam));
        assert!(!fallback_folder("Deleted Items", SenderAction::Spam));
        assert!(!fallback_folder("Junk", SenderAction::Delete));
        assert!(!fallback_folder("Trash", SenderAction::Archive));
    }
}
