use super::{mail::Mail, parser, smtp, storage::Storage};
use crate::models::{
    Account, Draft,
    draft::{Saved, Target},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use imap::{Session, types::Flag};
use std::io::{Read, Write};

pub fn save(
    account: &Account,
    draft: &Draft,
    target: Option<&Target>,
    storage: &mut Storage,
) -> Result<Saved> {
    let mut draft = draft.clone();
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
            let location =
                resolve(&mut mail.session, target)?.context("Draft is no longer on the server")?;
            ensure!(
                mail.session.capabilities()?.has_str("UIDPLUS"),
                "Mail server needs UIDPLUS support to replace a draft safely"
            );
            Ok(location)
        })
        .transpose()?;
    if let Some((validity, uid)) = previous {
        draft.attachments = attachments(&mut mail.session, &folder, validity, uid)?;
    }
    let raw = smtp::draft_message(account, &draft)?;
    let mut message = parser::parse(0, raw.as_bytes(), true, true)?;
    message.is_draft = true;
    // Do not retry APPEND: a lost response can still mean the draft was saved.
    append(&mut mail.session, &folder, raw.as_bytes())?;
    let cleanup_error = if let Some((validity, uid)) = previous {
        let removed = remove_uid(&mut mail.session, &folder, validity, uid);
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

    // Once APPEND succeeds, cache failures must not invite saving a duplicate.
    let cached = (|| -> Result<()> {
        let (validity, uid) = saved_location(&mut mail.session, &folder, &message.message_id)?;
        message.uid_validity = Some(validity);
        message.uid = uid;
        storage.store_saved_draft(&account.email, &folder, &message)?;
        Ok(())
    })();
    if let Err(error) = cached {
        eprintln!("Could not cache saved draft: {error}");
    }
    Ok(Saved {
        folder,
        message,
        cleanup_error,
    })
}

// Locate only the newly appended message; never scan/reconcile the whole mailbox here.
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

pub fn prepare_send(account: &Account, draft: &Draft, target: Option<&Target>) -> Result<Draft> {
    let mut draft = draft.clone();
    if let Some(target) = target {
        let mut mail = Mail::connect(account)?;
        let (validity, uid) =
            resolve(&mut mail.session, target)?.context("Draft is no longer on the server")?;
        draft.attachments = attachments(&mut mail.session, &target.folder, validity, uid)?;
    }
    Ok(draft)
}

fn attachments<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    validity: u32,
    uid: u32,
) -> Result<Vec<String>> {
    ensure!(
        session.select(folder)?.uid_validity == Some(validity),
        "Draft mailbox changed while reading attachments"
    );
    let fetched = session.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")?;
    let bytes = fetched
        .iter()
        .find(|item| item.uid == Some(uid) && item.body().is_some())
        .and_then(|item| item.body())
        .context("Draft is no longer on the server")?;
    let mail = mailparse::parse_mail(bytes)?;
    let mut retained = Vec::new();
    retain_parts(&mail, &mut retained)?;
    Ok(retained)
}

fn retain_parts(mail: &mailparse::ParsedMail<'_>, retained: &mut Vec<String>) -> Result<()> {
    let disposition = mail.get_content_disposition();
    let attachment = disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
        || mail.ctype.params.contains_key("name");
    if !attachment && !mail.subparts.is_empty() {
        for child in &mail.subparts {
            retain_parts(child, retained)?;
        }
    } else if attachment || !matches!(mail.ctype.mimetype.as_str(), "text/plain" | "text/html") {
        let mut part = String::new();
        for header in &mail.headers {
            if !header
                .get_key_ref()
                .eq_ignore_ascii_case("Content-Transfer-Encoding")
            {
                part.push_str(&format!(
                    "{}: {}\r\n",
                    header.get_key_ref(),
                    String::from_utf8_lossy(header.get_value_raw())
                ));
            }
        }
        part.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        let encoded = STANDARD.encode(mail.get_body_raw()?);
        for line in encoded.as_bytes().chunks(76) {
            part.push_str(std::str::from_utf8(line)?);
            part.push_str("\r\n");
        }
        retained.push(part);
    }
    Ok(())
}

pub fn delete(account: &Account, target: &Target, storage: &mut Storage) -> Result<()> {
    let mut mail = Mail::connect(account)?;
    let mut target = target.clone();
    if let Some((validity, uid)) = resolve(&mut mail.session, &target)? {
        remove_uid(&mut mail.session, &target.folder, validity, uid)?;
        target.uid = uid;
        target.validity = Some(validity);
    }
    if let Err(error) = storage.delete_draft(&account.email, &target) {
        eprintln!("Could not remove deleted draft from cache: {error}");
    }
    Ok(())
}

fn resolve<T: Read + Write>(
    session: &mut Session<T>,
    target: &Target,
) -> Result<Option<(u32, u32)>> {
    let validity = session
        .select(&target.folder)?
        .uid_validity
        .context("Missing UIDVALIDITY")?;
    ensure!(
        target.validity.is_none_or(|expected| expected == validity),
        "Draft mailbox changed; reload before deleting or replacing this draft"
    );
    let uid = if target.uid != 0 && target.validity.is_some() {
        target.uid
    } else {
        ensure!(
            !target.message_id.is_empty() && !target.message_id.starts_with("brevlada-draft:"),
            "Draft mailbox identity is unavailable; reload the draft"
        );
        let id = target.message_id.replace('\\', "\\\\").replace('"', "\\\"");
        let found = session.uid_search(format!("HEADER Message-ID \"{id}\""))?;
        let Some(uid) = found.into_iter().min() else {
            return Ok(None);
        };
        uid
    };
    let fetched = session.uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[HEADER])")?;
    let Some(item) = fetched.iter().find(|item| item.uid == Some(uid)) else {
        return Ok(None);
    };
    let mut header = parser::parse(
        uid,
        item.header().context("Missing draft headers")?,
        true,
        false,
    )?;
    header.is_draft = true;
    super::mail_sync::ensure_draft_id(&mut header, &target.folder);
    ensure!(
        header.message_id == target.message_id,
        "Draft changed on the server; reload before deleting or replacing it"
    );
    Ok(Some((validity, uid)))
}

fn remove_uid<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    validity: u32,
    uid: u32,
) -> Result<()> {
    ensure!(
        session.capabilities()?.has_str("UIDPLUS"),
        "Mail server needs UIDPLUS support to delete a draft safely"
    );
    ensure!(
        session.select(folder)?.uid_validity == Some(validity),
        "Draft mailbox changed during deletion"
    );
    session.uid_store(uid.to_string(), "+FLAGS.SILENT (\\Deleted)")?;
    session.uid_expunge(uid.to_string())?;
    Ok(())
}

fn append<T: Read + Write>(session: &mut Session<T>, folder: &str, raw: &[u8]) -> Result<()> {
    ensure!(!folder.contains(['\r', '\n']), "Invalid Drafts folder name");
    // APPEND in imap 2.4 interpolates the mailbox inside quotes.
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

    #[test]
    fn deletes_only_the_verified_uid_and_refuses_changed_mailboxes_or_messages() {
        let header = "Message-ID: <draft@example.com>\r\n\r\n";
        for (validity, id, success) in [
            (7, "draft@example.com", true),
            (8, "draft@example.com", false),
            (7, "different", false),
        ] {
            let commands = Arc::new(Mutex::new(Vec::new()));
            let mut session = imap::Client::new(Stream {
                responses: Cursor::new(format!(
                    "a1 OK login\r\n* OK [UIDVALIDITY {validity}] valid\r\na2 OK select\r\n\
                     * 1 FETCH (UID 42 FLAGS (\\Draft) BODY[HEADER] {{{}}}\r\n{header})\r\na3 OK fetch\r\n\
                     * CAPABILITY IMAP4rev1 UIDPLUS\r\na4 OK capabilities\r\n\
                     * OK [UIDVALIDITY 7] valid\r\na5 OK select\r\na6 OK store\r\na7 OK expunge\r\n", header.len()
                ).into_bytes()),
                commands: commands.clone(),
            }).login("test", "test").unwrap();
            let target = Target {
                folder: "Drafts".into(),
                uid: 42,
                validity: Some(7),
                message_id: id.into(),
            };
            let result = resolve(&mut session, &target).and_then(|location| {
                let (validity, uid) = location.unwrap();
                remove_uid(&mut session, &target.folder, validity, uid)
            });
            assert_eq!(result.is_ok(), success);
            let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            if success {
                assert!(written.contains("UID STORE 42 +FLAGS.SILENT (\\Deleted)"));
                assert!(written.contains("UID EXPUNGE 42"));
                assert!(!written.contains("a7 EXPUNGE"));
            } else {
                assert!(!written.contains("STORE"));
                assert!(!written.contains("EXPUNGE"));
            }
        }
    }

    #[test]
    fn retains_attachment_bytes_and_inline_content_ids_when_editing_a_draft() {
        let original = mailparse::parse_mail(b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nOld body\r\n--x\r\nContent-Type: application/octet-stream; name=report.bin\r\nContent-Disposition: attachment; filename=report.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAP8K\r\n--x\r\nContent-Type: image/png\r\nContent-ID: <inline>\r\n\r\nPNG\r\n--x--\r\n").unwrap();
        let mut parts = Vec::new();
        retain_parts(&original, &mut parts).unwrap();
        assert_eq!(parts.len(), 2);
        let attachment = mailparse::parse_mail(parts[0].as_bytes()).unwrap();
        assert_eq!(attachment.get_body_raw().unwrap(), [0, 255, 10]);
        assert_eq!(
            attachment.get_content_disposition().params["filename"],
            "report.bin"
        );
        use mailparse::MailHeaderMap;
        assert_eq!(
            mailparse::parse_mail(parts[1].as_bytes())
                .unwrap()
                .headers
                .get_first_value("Content-ID")
                .as_deref(),
            Some("<inline>")
        );
    }
}
