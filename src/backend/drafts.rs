use super::{mail::Mail, parser, storage::Storage};
use crate::models::{Account, Draft, draft::Target};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use imap::Session;
use std::io::{Read, Write};

pub fn prepare_send(account: &Account, draft: &Draft, target: Option<&Target>) -> Result<Draft> {
    let mut draft = draft.clone();
    if draft.attachments_loaded {
        return Ok(draft);
    }
    if let Some(target) = target {
        let mut mail = Mail::connect(account)?;
        let (validity, uid) =
            resolve(&mut mail.session, target)?.context("Draft is no longer on the server")?;
        let retained = attachments(
            &mut mail.session,
            &target.folder,
            validity,
            uid,
            &draft.removed_attachments,
        )?;
        draft.attachments.splice(0..0, retained);
    } else if let Some(source) = &draft.attachment_source {
        let mut mail = Mail::connect(account)?;
        let mut source = source.clone();
        if source.folder.is_empty() {
            source.folder = mail
                .sync_folders()?
                .sent
                .context("This account has no Sent folder")?;
        }
        let retained = forward_attachments(&mut mail.session, &source, &draft.removed_attachments)?;
        draft.attachments.splice(0..0, retained);
    }
    Ok(draft)
}

fn forward_attachments<T: Read + Write>(
    session: &mut Session<T>,
    source: &Target,
    removed: &[usize],
) -> Result<Vec<String>> {
    let validity = session
        .examine(&source.folder)?
        .uid_validity
        .context("Missing UIDVALIDITY for forwarded message")?;
    ensure!(
        source.validity.is_none_or(|expected| expected == validity),
        "Source mailbox changed; reopen the message before forwarding"
    );
    let uid = if source.uid != 0 {
        source.uid
    } else {
        ensure!(
            !source.message_id.is_empty(),
            "Forwarded message identity is unavailable"
        );
        let id = source.message_id.replace('\\', "\\\\").replace('"', "\\\"");
        let found = session.uid_search(format!("HEADER Message-ID \"{id}\""))?;
        ensure!(
            found.len() == 1,
            "Forwarded message is missing or ambiguous"
        );
        *found.iter().next().unwrap()
    };
    let fetched = session.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")?;
    let bytes = fetched
        .iter()
        .find(|item| item.uid == Some(uid) && item.body().is_some())
        .and_then(|item| item.body())
        .context("Forwarded message is no longer on the server")?;
    let header = parser::parse(uid, bytes, true, false)?;
    ensure!(
        header.message_id == parser::normalize_message_id(&source.message_id),
        "Source message changed; reopen it before forwarding"
    );
    let mut retained = Vec::new();
    retain_selected(
        &mailparse::parse_mail(bytes)?,
        &mut retained,
        removed,
        &mut 0,
    )?;
    Ok(retained)
}

fn attachments<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    validity: u32,
    uid: u32,
    removed: &[usize],
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
    retain_selected(&mail, &mut retained, removed, &mut 0)?;
    Ok(retained)
}

#[cfg(test)]
fn retain_parts(mail: &mailparse::ParsedMail<'_>, retained: &mut Vec<String>) -> Result<()> {
    retain_selected(mail, retained, &[], &mut 0)
}

fn retain_selected(
    mail: &mailparse::ParsedMail<'_>,
    retained: &mut Vec<String>,
    removed: &[usize],
    index: &mut usize,
) -> Result<()> {
    if crate::models::attachment::name(mail).is_some() {
        let current = *index;
        *index += 1;
        if removed.contains(&current) {
            return Ok(());
        }
    }
    let disposition = mail.get_content_disposition();
    let attachment = disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
        || mail.ctype.params.contains_key("name");
    if !attachment && !mail.subparts.is_empty() {
        for child in &mail.subparts {
            retain_selected(child, retained, removed, index)?;
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

pub(super) fn resolve<T: Read + Write>(
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

pub(super) fn remove_uid<T: Read + Write>(
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
    fn sends_materialized_attachments_without_fetching_or_duplicating_them() {
        let account = Account {
            email: "me@example.com".into(),
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
        let source = crate::models::Message {
            uid: 1,
            message_id: "draft".into(),
            attachments: vec!["file.bin".into()],
            ..Default::default()
        };
        let mut draft = Draft::from(&source);
        draft.attachments_loaded = true;
        draft.attachments = vec![
            super::super::attachments::encode("file.bin", "application/octet-stream", &[0, 255])
                .unwrap(),
        ];
        let target = Target::new("Drafts", &source);
        let prepared = prepare_send(&account, &draft, Some(&target)).unwrap();
        assert_eq!(prepared.attachments, draft.attachments);
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn forwarding_retains_attachments_and_checks_source_identity() {
        let raw = "Message-ID: <source@example.com>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nOriginal body\r\n--x\r\nContent-Type: application/octet-stream; name=report.bin\r\nContent-Disposition: attachment; filename=report.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAP8K\r\n--x--\r\n";
        for (validity, id, removed, success, count) in [
            (7, "source@example.com", vec![], true, 1),
            (7, "source@example.com", vec![0], true, 0),
            (8, "source@example.com", vec![], false, 0),
            (7, "other@example.com", vec![], false, 0),
        ] {
            let commands = Arc::new(Mutex::new(Vec::new()));
            let mut session = imap::Client::new(Stream {
                responses: Cursor::new(format!(
                    "a1 OK login\r\n* OK [UIDVALIDITY {validity}] valid\r\na2 OK examine\r\n* 1 FETCH (UID 42 BODY[] {{{}}}\r\n{raw})\r\na3 OK fetch\r\n", raw.len()
                ).into_bytes()),
                commands: commands.clone(),
            }).login("test", "test").unwrap();
            let source = Target {
                folder: "INBOX".into(),
                uid: 42,
                validity: Some(7),
                message_id: id.into(),
            };
            let result = forward_attachments(&mut session, &source, &removed);
            assert_eq!(result.is_ok(), success);
            if success {
                let parts = result.unwrap();
                assert_eq!(parts.len(), count);
                if count > 0 {
                    let part = mailparse::parse_mail(parts[0].as_bytes()).unwrap();
                    assert_eq!(part.get_body_raw().unwrap(), [0, 255, 10]);
                    assert_eq!(
                        part.get_content_disposition().params["filename"],
                        "report.bin"
                    );
                }
            }
            let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(written.contains("EXAMINE \"INBOX\""));
            assert!(!written.contains("STORE"));
        }
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
