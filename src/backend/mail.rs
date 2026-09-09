use super::{accounts, parser};
use crate::models::{Account, Message};
use anyhow::{Context, Result, bail};
use imap::{Authenticator, Session};
use native_tls::{TlsConnector, TlsStream};
use std::{
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

pub struct Mail {
    session: Session<TlsStream<TcpStream>>,
    socket: TcpStream,
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
        let sequence = recent
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetched = self
            .session
            .uid_fetch(sequence, "(UID FLAGS BODY.PEEK[HEADER])")?;
        let mut messages = Vec::new();
        for item in fetched.iter() {
            let uid = item.uid.context("Missing message UID")?;
            let bytes = item.header().context("Missing message headers")?;
            let mut message = parser::parse(
                uid,
                bytes,
                item.flags().contains(&imap::types::Flag::Seen),
                false,
            )?;
            message.is_flagged = item.flags().contains(&imap::types::Flag::Flagged);
            messages.push(message);
        }
        Ok((validity, messages, uids))
    }

    pub fn body(&mut self, folder: &str, uid: u32) -> Result<Message> {
        self.session.select(folder)?;
        let fetched = self
            .session
            .uid_fetch(uid.to_string(), "(UID FLAGS BODY.PEEK[])")?;
        let item = fetched
            .iter()
            .find(|item| item.uid == Some(uid) && item.body().is_some())
            .context("Message is no longer on the server")?;
        let mut message = parser::parse(
            uid,
            item.body().context("Missing message body")?,
            item.flags().contains(&imap::types::Flag::Seen),
            true,
        )?;
        message.is_flagged = item.flags().contains(&imap::types::Flag::Flagged);
        Ok(message)
    }

    pub fn mark_read(&mut self, folder: &str, uid: u32) -> Result<()> {
        self.session.select(folder)?;
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
