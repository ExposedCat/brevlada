use super::{mail_sync, parser};
use crate::models::Message;
use anyhow::{Context, Result};
use imap::{Session, types::Flag};
use std::io::{Read, Write};

pub(super) fn fetch<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    drafts: Option<&str>,
    validity: u32,
    uids: &[u32],
) -> Result<Vec<Message>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let sequence = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetched = session.uid_fetch(sequence, "(UID FLAGS BODY.PEEK[HEADER])")?;
    let mut messages = Vec::new();
    for item in fetched.iter() {
        let Some(bytes) = item.header() else {
            continue;
        };
        let uid = item.uid.context("Missing message UID")?;
        if !uids.contains(&uid) {
            continue;
        }
        let mut message = parser::parse(uid, bytes, item.flags().contains(&Flag::Seen), false)?;
        message.uid_validity = Some(validity);
        mail_sync::apply_flags(&mut message, folder, item.flags(), drafts);
        messages.push(message);
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Debug)]
    struct MockStream(Cursor<Vec<u8>>);

    impl Read for MockStream {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(bytes)
        }
    }

    impl Write for MockStream {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ignores_unsolicited_flags_and_unrequested_headers() {
        let header = "From: Alice <alice@example.com>\r\nMessage-ID: <one>\r\n\r\n";
        let responses = format!(
            "a1 OK login\r\n* 2 FETCH (FLAGS (\\Seen))\r\n\
             * 3 FETCH (UID 3 FLAGS (\\Seen))\r\n\
             * 1 FETCH (UID 1 FLAGS () BODY[HEADER] {{{}}}\r\n{})\r\n\
             * 9 FETCH (UID 9 FLAGS () BODY[HEADER] {{{}}}\r\n{})\r\n\
             a2 OK fetch\r\n",
            header.len(),
            header,
            header.len(),
            header
        );
        let mut session = imap::Client::new(MockStream(Cursor::new(responses.into_bytes())))
            .login("fixture", "fixture")
            .unwrap();
        let messages = fetch(&mut session, "INBOX", None, 7, &[1, 4]).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].uid, 1);
        assert_eq!(messages[0].message_id, "one");
    }
}
