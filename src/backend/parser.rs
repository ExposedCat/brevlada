use crate::models::{InlineMedia, Message, Unsubscribe};
use anyhow::Result;
use base64::Engine;
use mailparse::{MailHeaderMap, ParsedMail};

pub fn parse(uid: u32, bytes: &[u8], is_read: bool, body_loaded: bool) -> Result<Message> {
    parse_inner(uid, bytes, is_read, body_loaded, false)
}

pub fn parse_with_inline(uid: u32, bytes: &[u8], is_read: bool) -> Result<Message> {
    parse_inner(uid, bytes, is_read, true, true)
}

fn parse_inner(
    uid: u32,
    bytes: &[u8],
    is_read: bool,
    body_loaded: bool,
    inline: bool,
) -> Result<Message> {
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
        cc: header("Cc"),
        in_reply_to: {
            let value = header("In-Reply-To");
            let id = normalize_message_id(&value);
            (!id.is_empty()).then(|| id.to_owned())
        },
        timestamp: mailparse::dateparse(&date).unwrap_or_default(),
        date,
        references: references
            .split_whitespace()
            .map(|s| s.trim_matches(['<', '>']).to_string())
            .collect(),
        is_read,
        body_loaded,
        unsubscribe: unsubscribe(
            &header("List-Unsubscribe"),
            &header("List-Unsubscribe-Post"),
        ),
        ..Default::default()
    };
    if body_loaded {
        message.inline_media_loaded = inline;
        bodies(&mail, &mut message, inline)?;
    }
    Ok(message)
}

fn unsubscribe(value: &str, post: &str) -> Option<Unsubscribe> {
    let links: Vec<_> = value
        .split(',')
        .filter_map(|part| part.trim().strip_prefix('<')?.strip_suffix('>'))
        .collect();
    let url = links
        .iter()
        .find(|url| url.starts_with("https://"))
        .or_else(|| links.iter().find(|url| url.starts_with("mailto:")))?;
    Some(Unsubscribe {
        url: (*url).to_owned(),
        one_click: Some(url.starts_with("https://") && post.trim() == "List-Unsubscribe=One-Click"),
    })
}

pub fn normalize_message_id(value: &str) -> &str {
    value.trim().trim_matches(['<', '>']).trim()
}

fn bodies(mail: &ParsedMail<'_>, message: &mut Message, inline: bool) -> Result<()> {
    if inline
        && matches!(
            mail.ctype.mimetype.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        )
    {
        let content_id = mail
            .headers
            .get_first_value("Content-ID")
            .unwrap_or_default();
        let content_id = content_id
            .trim()
            .trim_matches(['<', '>'])
            .to_ascii_lowercase();
        if !content_id.is_empty() {
            let bytes = mail.get_body_raw()?;
            let current: usize = message
                .inline_media
                .iter()
                .map(|part| part.data.len())
                .sum();
            if current + bytes.len() * 4 / 3 <= 8 * 1024 * 1024 {
                message.inline_media.push(InlineMedia {
                    content_id,
                    mime: mail.ctype.mimetype.clone(),
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
        }
    }
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
            bodies(part, message, inline)?;
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
    fn reads_unsubscribe_headers_without_loading_body() {
        let raw = b"List-Unsubscribe: <mailto:leave@example.org>, <https://example.org/u/1>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\n";
        let message = parse(4, raw, false, false).unwrap();
        assert_eq!(
            message.unsubscribe,
            Some(Unsubscribe {
                url: "https://example.org/u/1".into(),
                one_click: Some(true),
            })
        );
        assert!(!message.body_loaded);
    }

    #[test]
    fn keeps_browser_and_mailto_unsubscribe_links_without_one_click() {
        for (headers, url) in [
            (
                "List-Unsubscribe: <https://example.org/preferences>",
                "https://example.org/preferences",
            ),
            (
                "List-Unsubscribe: <https://example.org/preferences>\r\nList-Unsubscribe-Post: something-else",
                "https://example.org/preferences",
            ),
            (
                "List-Unsubscribe: <mailto:leave@example.org>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click",
                "mailto:leave@example.org",
            ),
        ] {
            let raw = format!("{headers}\r\n\r\n");
            let message = parse(4, raw.as_bytes(), false, false).unwrap();
            assert_eq!(
                message.unsubscribe,
                Some(Unsubscribe {
                    url: url.into(),
                    one_click: Some(false)
                })
            );
        }
    }

    #[test]
    fn distinguishes_legacy_cached_unsubscribe_links_from_get_links() {
        let unsubscribe: Unsubscribe =
            serde_json::from_str(r#"{"url":"https://example.org/u/1"}"#).unwrap();
        assert_eq!(unsubscribe.one_click, None);
        let raw = b"List-Unsubscribe: <https://example.org/u/1>\r\n\r\n";
        let message = parse(4, raw, false, false).unwrap();
        assert_eq!(message.unsubscribe.unwrap().one_click, Some(false));
    }

    #[test]
    fn keeps_inline_cid_images_from_multipart_mail() {
        let raw = b"MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=x\r\n\r\n--x\r\nContent-Type: text/html\r\n\r\n<img src='cid:logo@example.org'>\r\n--x\r\nContent-Type: image/png\r\nContent-ID: <logo@example.org>\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--x--\r\n";
        let message = parse_with_inline(3, raw, false).unwrap();
        assert!(message.inline_media_loaded);
        assert_eq!(message.inline_media.len(), 1);
        assert_eq!(message.inline_media[0].data, "aGVsbG8=");
        let background = parse(3, raw, false, true).unwrap();
        assert!(!background.inline_media_loaded);
        assert!(background.inline_media.is_empty());
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
