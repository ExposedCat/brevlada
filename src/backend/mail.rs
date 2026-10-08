use super::{accounts, parser};
use crate::models::{Account, Message};
use anyhow::{Context, Result, bail};
use imap::{Authenticator, Session};
use native_tls::{TlsConnector, TlsStream};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

pub struct Mail {
    pub(super) session: Session<TlsStream<TcpStream>>,
    socket: TcpStream,
    pub(super) drafts_folder: Option<String>,
}

struct OAuth(String);
impl Authenticator for OAuth {
    type Response = String;
    fn process(&self, challenge: &[u8]) -> String {
        if challenge.is_empty() {
            self.0.clone()
        } else {
            String::new()
        }
    }
}

impl Mail {
    pub fn connect(account: &Account) -> Result<Self> {
        Self::connect_with(account, |_| Ok(()))
    }

    pub fn cancellation_socket(&self) -> Result<TcpStream> {
        Ok(self.socket.try_clone()?)
    }

    pub fn connect_with(
        account: &Account,
        mut watch: impl FnMut(TcpStream) -> Result<()>,
    ) -> Result<Self> {
        if !account.ssl && !account.tls {
            bail!("This account needs SSL or STARTTLS enabled in Online Accounts");
        }
        let addresses = (account.host.as_str(), account.port).to_socket_addrs()?;
        let mut stream = None;
        for address in addresses {
            if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_secs(15)) {
                stream = Some(socket);
                break;
            }
        }
        let socket = stream.context("Could not connect to the mail server")?;
        socket.set_read_timeout(Some(Duration::from_secs(30)))?;
        socket.set_write_timeout(Some(Duration::from_secs(30)))?;
        let cancellation = socket.try_clone()?;
        watch(socket.try_clone()?)?;
        let tls = TlsConnector::new()?;
        let client = if account.ssl {
            let mut client = imap::Client::new(tls.connect(&account.host, socket)?);
            client.read_greeting()?;
            client
        } else {
            let mut client = imap::Client::new(socket);
            client.read_greeting()?;
            client.secure(&account.host, &tls)?
        };
        let session = if account.oauth2 {
            let token = accounts::token(account)?;
            client
                .authenticate(
                    "XOAUTH2",
                    &OAuth(format!(
                        "user={}\x01auth=Bearer {}\x01\x01",
                        account.username, token
                    )),
                )
                .map_err(|(error, _)| error)?
        } else {
            client
                .login(&account.username, accounts::password(account)?)
                .map_err(|(error, _)| error)?
        };
        Ok(Self {
            session,
            socket: cancellation,
            drafts_folder: None,
        })
    }

    pub fn folders(&mut self) -> Result<Vec<String>> {
        let folders = self.session.list(None, Some("*"))?;
        let mut names: Vec<String> = folders
            .iter()
            .filter(|f| {
                !f.attributes()
                    .contains(&imap::types::NameAttribute::NoSelect)
            })
            .map(|f| f.name().to_string())
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn has_unread(&mut self, folder: &str) -> Result<bool> {
        has_unread(&mut self.session, folder)
    }

    pub fn delivery_uids(
        &mut self,
        folder: &str,
        validity: Option<u32>,
        sender: &str,
        order: &str,
    ) -> Result<Vec<u32>> {
        let query = delivery_query(sender, order)?;
        let mailbox = self.session.examine(folder)?;
        anyhow::ensure!(
            validity.is_none() || mailbox.uid_validity == validity,
            "Mailbox changed while searching delivery updates"
        );
        let mut uids: Vec<_> = self.session.uid_search(query)?.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    pub fn headers(&mut self, folder: &str) -> Result<(u32, Vec<Message>, Vec<u32>)> {
        let mailbox = self.session.select(folder)?;
        let validity = mailbox
            .uid_validity
            .context("Mail server did not provide UIDVALIDITY")?;
        let mut uids: Vec<_> = self.session.uid_search("ALL")?.into_iter().collect();
        uids.sort_unstable();
        let recent = &uids[uids.len().saturating_sub(crate::theme::MESSAGE_LIMIT)..];
        if recent.is_empty() {
            return Ok((validity, Vec::new(), uids));
        }
        let messages = super::mail_headers::fetch(
            &mut self.session,
            folder,
            self.drafts_folder.as_deref(),
            validity,
            recent,
        )?;
        Ok((validity, messages, uids))
    }

    pub(super) fn body_with_validity(
        &mut self,
        folder: &str,
        uid: u32,
        validity: Option<u32>,
        inline: bool,
    ) -> Result<Message> {
        let mailbox = self.session.select(folder)?;
        anyhow::ensure!(
            validity.is_none() || mailbox.uid_validity == validity,
            "Mailbox changed while loading message"
        );
        let fetched = self
            .session
            .uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[])")?;
        let item = fetched
            .iter()
            .find(|item| item.uid == Some(uid) && item.body().is_some())
            .context("Message is no longer on the server")?;
        let bytes = item.body().context("Missing message body")?;
        let mut message = if inline {
            parser::parse_with_inline(uid, bytes, item.flags().contains(&imap::types::Flag::Seen))?
        } else {
            parser::parse(
                uid,
                bytes,
                item.flags().contains(&imap::types::Flag::Seen),
                true,
            )?
        };
        message.uid_validity = mailbox.uid_validity;
        super::mail_sync::apply_flags(
            &mut message,
            folder,
            item.flags(),
            self.drafts_folder.as_deref(),
        );
        Ok(message)
    }

    pub fn header_with_validity(
        &mut self,
        folder: &str,
        uid: u32,
        validity: Option<u32>,
    ) -> Result<Message> {
        let mailbox = self.session.examine(folder)?;
        anyhow::ensure!(
            validity.is_none() || mailbox.uid_validity == validity,
            "Mailbox changed while loading unsubscribe headers"
        );
        let fetched = self
            .session
            .uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[HEADER])")?;
        let item = fetched
            .iter()
            .find(|item| item.uid == Some(uid) && item.header().is_some())
            .context("Message is no longer on the server")?;
        parser::parse(
            uid,
            item.header().context("Missing message headers")?,
            item.flags().contains(&imap::types::Flag::Seen),
            false,
        )
    }

    pub fn mark_read(&mut self, folder: &str, uid: u32, validity: Option<u32>) -> Result<()> {
        let mailbox = self.session.select(folder)?;
        anyhow::ensure!(
            validity.is_none() || mailbox.uid_validity == validity,
            "Mailbox changed while marking message read"
        );
        self.session
            .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)")?;
        Ok(())
    }
}

impl Drop for Mail {
    fn drop(&mut self) {
        let _ = self.session.logout();
    }
}

fn has_unread<T: Read + Write>(session: &mut Session<T>, folder: &str) -> Result<bool> {
    use imap::types::{StatusAttribute, UnsolicitedResponse};

    // imap 2.4 routes STATUS attributes through unsolicited_responses; the
    // returned Mailbox.unseen is only populated by SELECT/EXAMINE response codes.
    // Discard earlier notifications so only this request can supply the count.
    for _ in session.unsolicited_responses.try_iter() {}
    session.status(folder, "(UNSEEN)")?;
    let mut count = None;
    for response in session.unsolicited_responses.try_iter() {
        if let UnsolicitedResponse::Status {
            mailbox,
            attributes,
        } = response
            && (mailbox == folder
                || (mailbox.eq_ignore_ascii_case("INBOX") && folder.eq_ignore_ascii_case("INBOX")))
        {
            for attribute in attributes {
                if let StatusAttribute::Unseen(value) = attribute {
                    count = Some(value);
                }
            }
        }
    }
    Ok(count.context("Mail server did not return an unread count for the requested folder")? > 0)
}

fn delivery_query(sender: &str, order: &str) -> Result<String> {
    let quote = |value: &str| -> Result<String> {
        anyhow::ensure!(
            !value.trim().is_empty() && !value.chars().any(char::is_control),
            "Invalid delivery search value"
        );
        Ok(format!(
            "\"{}\"",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    };
    Ok(format!("FROM {} TEXT {}", quote(sender)?, quote(order)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    #[test]
    fn delivery_search_quotes_values_and_rejects_missing_ids_and_controls() {
        assert_eq!(
            delivery_query("shop@example.com", "123").unwrap(),
            "FROM \"shop@example.com\" TEXT \"123\""
        );
        assert_eq!(
            delivery_query("shop@example.com", "a\"b\\c").unwrap(),
            "FROM \"shop@example.com\" TEXT \"a\\\"b\\\\c\""
        );
        for order in ["", "  ", "123\r\nALL"] {
            assert!(delivery_query("shop@example.com", order).is_err());
        }
    }

    #[derive(Debug)]
    struct MockStream(Cursor<Vec<u8>>);

    impl Read for MockStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl Write for MockStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn session(responses: &str) -> Session<MockStream> {
        imap::Client::new(MockStream(Cursor::new(
            format!("a1 OK Logged in\r\n{responses}").into_bytes(),
        )))
        .login("fixture", "fixture")
        .unwrap()
    }

    #[test]
    fn reads_status_counts_from_the_imap_response_channel() {
        let mut session = session(
            "* STATUS INBOX (UNSEEN 3)\r\na2 OK STATUS completed\r\n\
             * STATUS INBOX (UNSEEN 0)\r\na3 OK STATUS completed\r\n",
        );
        assert!(has_unread(&mut session, "inbox").unwrap());
        assert!(!has_unread(&mut session, "INBOX").unwrap());
    }

    #[test]
    fn ignores_stale_counts_and_other_mailboxes() {
        let mut session = session(
            "* STATUS INBOX (UNSEEN 5)\r\na2 OK STATUS completed\r\n\
             * STATUS Other (UNSEEN 9)\r\n\
             * STATUS INBOX (MESSAGES 7)\r\na3 OK STATUS completed\r\n",
        );
        session.status("INBOX", "(UNSEEN)").unwrap();
        assert!(has_unread(&mut session, "INBOX").is_err());
    }
}
