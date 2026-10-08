use super::{drafts, mail::Mail, parser, smtp, storage::Storage};
use crate::models::{
    Account, Draft, SentMessage,
    draft::{Saved, Target},
};
use anyhow::{Context, Result, ensure};
use imap::{Session, types::Flag};
use std::io::{Read, Write};

pub fn save(
    account: &Account,
    draft: &Draft,
    target: Option<&Target>,
    storage: &mut Storage,
) -> Result<Saved> {
    let original_parts = if draft.attachments_loaded {
        Vec::new()
    } else {
        let mut original = draft.clone();
        original.attachments.clear();
        original.removed_attachments.clear();
        drafts::prepare_send(account, &original, target)?.attachments
    };
    let mut hydrated = draft.clone();
    let mut index = 0;
    let mut retained = Vec::new();
    for part in &original_parts {
        let named =
            crate::models::attachment::name(&mailparse::parse_mail(part.as_bytes())?).is_some();
        let removed = named && draft.removed_attachments.contains(&index);
        if named {
            index += 1;
        }
        if !removed {
            retained.push(part.clone());
        }
    }
    hydrated.attachments.splice(0..0, retained);
    let mut mail = Mail::connect(account)?;
    let folder = mail
        .sync_folders()?
        .drafts
        .context("This account has no Drafts folder")?;
    let previous = target
        .map(|target| {
            ensure!(
                target.folder == folder,
                "The Drafts folder changed; reload before saving"
            );
            let location = drafts::resolve(&mut mail.session, target)?
                .context("Draft is no longer on the server")?;
            ensure!(
                mail.session.capabilities()?.has_str("UIDPLUS"),
                "Mail server needs UIDPLUS support to replace a draft safely"
            );
            Ok(location)
        })
        .transpose()?;
    let raw = smtp::draft_message(account, &hydrated)?;
    let mut message = parser::parse(0, raw.as_bytes(), true, true)?;
    message.is_draft = true;
    append(&mut mail.session, &folder, raw.as_bytes())?;
    let cleanup_error = if let Some((validity, uid)) = previous {
        let removed = drafts::remove_uid(&mut mail.session, &folder, validity, uid);
        if removed.is_ok()
            && let Some(target) = target
        {
            let mut target = target.clone();
            target.uid = uid;
            target.validity = Some(validity);
            if let Err(error) = storage.delete_draft(&account.email, &target) {
                eprintln!("Could not remove replaced draft from cache: {error}");
            }
        }
        removed.err().map(|error| error.to_string())
    } else {
        None
    };
    let cached = (|| -> Result<()> {
        let (validity, uid) = saved_location(&mut mail.session, &folder, &message.message_id)?;
        message.uid_validity = Some(validity);
        message.uid = uid;
        storage.store_saved_draft(&account.email, &folder, &message)
    })();
    if let Err(error) = cached {
        eprintln!("Could not cache saved draft: {error}");
    }
    Ok(Saved {
        item: SentMessage { folder, message },
        original_parts,
        cleanup_error,
    })
}

fn saved_location<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    id: &str,
) -> Result<(u32, u32)> {
    let validity = session
        .select(folder)?
        .uid_validity
        .context("Missing UIDVALIDITY")?;
    let quoted = id.replace('\\', "\\\\").replace('"', "\\\"");
    let found = session.uid_search(format!("HEADER Message-ID \"{quoted}\""))?;
    ensure!(!found.is_empty(), "Saved draft UID is not yet available");
    let sequence = found
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetched = session.uid_fetch(sequence, "(UID BODY.PEEK[HEADER])")?;
    let mut matching = Vec::new();
    for item in fetched.iter() {
        if let (Some(uid), Some(header)) = (item.uid, item.header())
            && found.contains(&uid)
            && parser::parse(uid, header, true, false)?.message_id == id
        {
            matching.push(uid);
        }
    }
    ensure!(
        matching.len() == 1,
        "Saved draft identity is ambiguous or unavailable"
    );
    Ok((validity, matching[0]))
}

fn append<T: Read + Write>(session: &mut Session<T>, folder: &str, raw: &[u8]) -> Result<()> {
    ensure!(!folder.contains(['\r', '\n']), "Invalid Drafts folder name");
    let quoted = folder.replace('\\', "\\\\").replace('"', "\\\"");
    session.append_with_flags(&quoted, raw, &[Flag::Draft, Flag::Seen])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
    };

    #[derive(Debug)]
    struct Stream {
        responses: Cursor<Vec<u8>>,
        commands: Arc<Mutex<Vec<u8>>>,
    }
    impl Read for Stream {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.responses.read(buffer)
        }
    }
    impl Write for Stream {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.commands.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn appends_with_draft_flags_and_reports_rejection_without_retrying() {
        for (response, success) in [
            ("a2 OK saved\r\n", true),
            ("a2 NO quota exceeded\r\n", false),
        ] {
            let commands = Arc::new(Mutex::new(Vec::new()));
            let mut session = imap::Client::new(Stream {
                responses: Cursor::new(
                    format!("a1 OK login\r\n+ ready\r\n{response}").into_bytes(),
                ),
                commands: commands.clone(),
            })
            .login("test", "test")
            .unwrap();
            let raw = b"Subject: Unfinished\r\n\r\nHello";
            assert_eq!(
                append(&mut session, "Work/\"Drafts\"", raw).is_ok(),
                success
            );
            let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert_eq!(written.matches("APPEND").count(), 1);
            assert!(written.contains("APPEND \"Work/\\\"Drafts\\\"\" (\\Draft \\Seen)"));
            assert!(written.ends_with("Subject: Unfinished\r\n\r\nHello\r\n"));
        }
    }

    #[test]
    fn locates_only_the_saved_draft_instead_of_fetching_the_mailbox() {
        let header = "Message-ID: <new-draft@example.com>\r\n\r\n";
        let commands = Arc::new(Mutex::new(Vec::new()));
        let mut session = imap::Client::new(Stream {
            responses: Cursor::new(format!("a1 OK login\r\n* OK [UIDVALIDITY 7] valid\r\na2 OK select\r\n* SEARCH 42\r\na3 OK search\r\n* 1 FETCH (UID 42 BODY[HEADER] {{{}}}\r\n{header})\r\na4 OK fetch\r\n", header.len()).into_bytes()),
            commands: commands.clone(),
        }).login("test", "test").unwrap();
        assert_eq!(
            saved_location(&mut session, "Drafts", "new-draft@example.com").unwrap(),
            (7, 42)
        );
        let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(written.contains("UID SEARCH HEADER Message-ID \"new-draft@example.com\""));
        assert!(written.contains("UID FETCH 42 (UID BODY.PEEK[HEADER])"));
        assert!(!written.contains("1:*"));
    }
}
