use crate::models::Message;
use anyhow::Result;
use mailparse::{MailHeaderMap, ParsedMail};

pub fn parse(uid: u32, bytes: &[u8], is_read: bool, body_loaded: bool) -> Result<Message> {
    let mail = mailparse::parse_mail(bytes)?;
    let header = |name| mail.headers.get_first_value(name).unwrap_or_default();
    let date = header("Date");
    let references = format!("{} {}", header("References"), header("In-Reply-To"));
    let mut message = Message {
        uid,
        message_id: normalize_message_id(&header("Message-ID")).to_string(),
        subject: header("Subject"),
        sender: header("From"),
        recipients: header("To"),
        timestamp: mailparse::dateparse(&date).unwrap_or_default(),
        date,
        references: references
            .split_whitespace()
            .map(|s| s.trim_matches(['<', '>']).to_string())
            .collect(),
        is_read,
        body_loaded,
        ..Default::default()
    };
    if body_loaded {
        bodies(&mail, &mut message)?;
    }
    Ok(message)
}

pub fn normalize_message_id(value: &str) -> &str {
    value.trim().trim_matches(['<', '>']).trim()
}

fn bodies(mail: &ParsedMail<'_>, message: &mut Message) -> Result<()> {
    let disposition = mail.get_content_disposition();
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| mail.ctype.params.get("name"));
    if let Some(name) = filename {
        message.attachments.push(name.clone());
        return Ok(());
    }
    if !mail.subparts.is_empty() {
        for part in &mail.subparts {
            bodies(part, message)?;
        }
    } else if mail.ctype.mimetype == "text/html" {
        message.body_html.push_str(&mail.get_body()?);
    } else if mail.ctype.mimetype == "text/plain" {
        if !message.body_text.is_empty() {
            message.body_text.push('\n');
        }
        message.body_text.push_str(&mail.get_body()?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_multipart_bodies_and_attachment_names() {
        let raw = b"From: =?UTF-8?Q?J=C3=B6rg?= <j@example.com>\r\nSubject: =?UTF-8?Q?Gr=C3=BC=C3=9Fe?=\r\nMessage-ID: <one>\r\nReferences: <parent>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8=\r\n--x\r\nContent-Type: text/html\r\n\r\n<b>Hello</b>\r\n--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=report.pdf\r\n\r\nPDF\r\n--x--\r\n";
        let message = parse(7, raw, false, true).unwrap();
        assert_eq!(message.subject, "Grüße");
        assert!(message.sender.contains("Jörg"));
        assert_eq!(message.body_text.trim(), "Hello");
        assert!(message.body_html.contains("<b>Hello</b>"));
        assert_eq!(message.attachments, ["report.pdf"]);
        assert_eq!(message.references, ["parent"]);
        assert!(message.body_loaded);
    }

    #[test]
    fn headers_remain_lazy() {
        let message = parse(9, b"Subject: Test\r\n\r\n", true, false).unwrap();
        assert!(!message.body_loaded);
        assert!(message.is_read);
    }

    #[test]
    fn normalizes_folded_message_ids_consistently() {
        let header = parse(66, b"Message-ID: <same@example.com>\r\n\r\n", false, false).unwrap();
        let body = parse(
            66,
            b"Message-ID: \r\n <same@example.com>\r\n\r\nHello",
            false,
            true,
        )
        .unwrap();
        assert_eq!(header.message_id, "same@example.com");
        assert_eq!(header.message_id, body.message_id);
        assert_eq!(
            normalize_message_id(" <same@example.com> "),
            header.message_id
        );
        assert_ne!(
            normalize_message_id("<other@example.com>"),
            header.message_id
        );
    }
}
