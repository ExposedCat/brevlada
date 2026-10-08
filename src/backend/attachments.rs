use super::{mail::Mail, parser};
use crate::models::{Account, Message, attachment};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};

pub const MAX_BYTES: usize = 20 * 1024 * 1024;

pub fn validate(parts: &[String]) -> Result<()> {
    let mut total = 0;
    for part in parts {
        total += mailparse::parse_mail(part.as_bytes())?
            .get_body_raw()?
            .len();
        ensure!(
            total <= MAX_BYTES,
            "Attachments exceed the 20 MB total limit"
        );
    }
    Ok(())
}

pub fn encode(name: &str, mime: &str, bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "Attachments must be smaller than 20 MB"
    );
    ensure!(
        mime.contains('/')
            && mime
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/!#$&^_.+-".contains(&c)),
        "Invalid attachment content type"
    );
    let name = attachment::safe_name(name);
    let mut part = format!("Content-Type: {mime}\r\nContent-Disposition: attachment;\r\n");
    let encoded: Vec<String> = name.bytes().map(|byte| format!("%{byte:02X}")).collect();
    for (index, chunk) in encoded.chunks(18).enumerate() {
        if index > 0 {
            part.push_str(";\r\n");
        }
        part.push_str(&format!(
            " filename*{index}*={}{value}",
            if index == 0 { "UTF-8''" } else { "" },
            value = chunk.concat()
        ));
    }
    part.push_str("\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let encoded = STANDARD.encode(bytes);
    for line in encoded.as_bytes().chunks(76) {
        part.push_str(std::str::from_utf8(line)?);
        part.push_str("\r\n");
    }
    Ok(part)
}

pub fn download(
    account: &Account,
    folder: &str,
    message: &Message,
    index: usize,
) -> Result<Vec<u8>> {
    let mut mail = Mail::connect(account)?;
    let folder = if folder.is_empty() {
        mail.sync_folders()?
            .sent
            .context("This account has no Sent folder")?
    } else {
        folder.to_owned()
    };
    let mailbox = mail.session.examine(&folder)?;
    ensure!(
        message.uid_validity.is_none() || mailbox.uid_validity == message.uid_validity,
        "Mailbox changed; reload the message"
    );
    let uid = if message.uid == 0 {
        ensure!(
            !message.message_id.is_empty() && !message.message_id.chars().any(char::is_control),
            "Message identity is unavailable"
        );
        let id = message
            .message_id
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let found = mail
            .session
            .uid_search(format!("HEADER Message-ID \"{id}\""))?;
        ensure!(
            found.len() == 1,
            "Sent message is missing or ambiguous; refresh the Sent folder"
        );
        *found.iter().next().unwrap()
    } else {
        message.uid
    };
    let fetched = mail
        .session
        .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")?;
    let raw = fetched
        .iter()
        .find(|item| item.uid == Some(uid))
        .and_then(|item| item.body())
        .context("Message is no longer on the server")?;
    decode(message, raw, index)
}

fn decode(message: &Message, raw: &[u8], index: usize) -> Result<Vec<u8>> {
    let header = parser::parse(message.uid, raw, true, false)?;
    ensure!(
        header.message_id == message.message_id,
        "Message changed; reload it before saving attachments"
    );
    let parsed = mailparse::parse_mail(raw)?;
    let mut parts = Vec::new();
    collect(&parsed, &mut parts);
    let part = parts
        .get(index)
        .context("Attachment is no longer available")?;
    ensure!(
        attachment::name(part).as_ref() == message.attachments.get(index),
        "Attachment changed; reload the message"
    );
    Ok(part.get_body_raw()?)
}

fn collect<'a, 'b>(
    mail: &'b mailparse::ParsedMail<'a>,
    parts: &mut Vec<&'b mailparse::ParsedMail<'a>>,
) {
    if attachment::name(mail).is_some() {
        parts.push(mail);
    } else {
        for child in &mail.subparts {
            collect(child, parts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_unicode_names_and_binary_data_without_injecting_headers() {
        let name = "résumé \"東京\".bin";
        let raw = encode(name, "application/octet-stream", &[0, 255, 13, 10]).unwrap();
        let parsed = mailparse::parse_mail(raw.as_bytes()).unwrap();
        assert_eq!(attachment::name(&parsed).as_deref(), Some(name));
        assert_eq!(parsed.get_body_raw().unwrap(), [0, 255, 13, 10]);
        let raw = encode("../../evil\r\nBcc: test.txt", "text/plain", b"hello").unwrap();
        let parsed = mailparse::parse_mail(raw.as_bytes()).unwrap();
        assert_eq!(attachment::name(&parsed).unwrap(), "evilBcc: test.txt");
        assert!(encode("test", "text/plain\r\nBcc: bad", b"").is_err());
    }

    #[test]
    fn downloads_duplicate_names_by_position_and_refuses_changed_messages() {
        let first = encode("report.bin", "application/octet-stream", &[0, 255]).unwrap();
        let second = encode("report.bin", "application/octet-stream", &[13, 10]).unwrap();
        let raw = format!(
            "Message-ID: <test@example.com>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nHello\r\n--x\r\n{first}--x\r\n{second}--x--\r\n"
        );
        let mut message = parser::parse(42, raw.as_bytes(), false, true).unwrap();
        assert_eq!(decode(&message, raw.as_bytes(), 0).unwrap(), [0, 255]);
        assert_eq!(decode(&message, raw.as_bytes(), 1).unwrap(), [13, 10]);
        assert!(decode(&message, raw.as_bytes(), 2).is_err());
        message.attachments[0] = "changed.bin".into();
        assert!(decode(&message, raw.as_bytes(), 0).is_err());
        message.message_id = "other@example.com".into();
        assert!(decode(&message, raw.as_bytes(), 1).is_err());
    }
}
