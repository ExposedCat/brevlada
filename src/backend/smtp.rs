use super::accounts;
use crate::models::{Account, Draft, Message, SmtpSettings};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use mailparse::MailAddr;
use native_tls::TlsConnector;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

struct Client<S: Read + Write>(BufReader<S>);

impl<S: Read + Write> Client<S> {
    fn new(stream: S) -> Self {
        Self(BufReader::new(stream))
    }

    fn response(&mut self, expected: u16) -> Result<String> {
        let mut response = String::new();
        loop {
            let mut line = String::new();
            ensure!(
                self.0.read_line(&mut line)? != 0,
                "SMTP server closed the connection"
            );
            ensure!(line.len() >= 4, "Invalid SMTP response");
            let code = line[..3].parse::<u16>().context("Invalid SMTP status")?;
            response.push_str(&line);
            if line.as_bytes()[3] == b' ' {
                ensure!(
                    code == expected || (expected == 250 && code == 251),
                    "SMTP server: {}",
                    response.trim()
                );
                return Ok(response);
            }
            ensure!(line.as_bytes()[3] == b'-', "Invalid SMTP response");
        }
    }

    fn command(&mut self, line: &str, expected: u16) -> Result<String> {
        self.0.get_mut().write_all(line.as_bytes())?;
        self.0.get_mut().write_all(b"\r\n")?;
        self.0.get_mut().flush()?;
        self.response(expected)
    }

    fn write_data(&mut self, data: &str) -> Result<()> {
        for line in data.split("\r\n") {
            if line.starts_with('.') {
                self.0.get_mut().write_all(b".")?;
            }
            self.0.get_mut().write_all(line.as_bytes())?;
            self.0.get_mut().write_all(b"\r\n")?;
        }
        self.0.get_mut().write_all(b".\r\n")?;
        self.0.get_mut().flush()?;
        self.response(250)?;
        Ok(())
    }
}

pub fn send(account: &Account, draft: &Draft) -> Result<Message> {
    let settings = account
        .smtp
        .as_ref()
        .context("Sending is unavailable for this account")?;
    ensure!(!settings.host.is_empty(), "SMTP server is not configured");
    ensure!(
        settings.ssl || settings.tls,
        "SMTP encryption is not enabled in Online Accounts"
    );
    let to = addresses(&draft.to)?;
    ensure!(!to.is_empty(), "Enter a recipient");
    let cc = addresses(&draft.cc)?;
    let from = addresses(&account.email)?;
    ensure!(from.len() == 1, "Account email address is invalid");
    ensure!(
        !draft.subject.trim().is_empty() && !draft.text.trim().is_empty(),
        "Enter a subject and message"
    );
    let message = message(account, draft, &to, &cc)?;
    let socket = (settings.host.as_str(), settings.port)
        .to_socket_addrs()?
        .find_map(|address| TcpStream::connect_timeout(&address, Duration::from_secs(15)).ok())
        .context("Could not connect to the SMTP server")?;
    socket.set_read_timeout(Some(Duration::from_secs(30)))?;
    socket.set_write_timeout(Some(Duration::from_secs(30)))?;
    let tls = TlsConnector::new()?;
    if settings.ssl {
        let mut client = Client::new(tls.connect(&settings.host, socket)?);
        client.response(220)?;
        deliver(&mut client, account, settings, &from[0], &to, &cc, &message)?;
    } else {
        let mut client = Client::new(socket);
        client.response(220)?;
        let greeting = client.command("EHLO localhost", 250)?;
        ensure!(
            greeting.to_ascii_uppercase().contains("STARTTLS"),
            "SMTP server does not support STARTTLS"
        );
        client.command("STARTTLS", 220)?;
        let stream = client.0.into_inner();
        let mut client = Client::new(tls.connect(&settings.host, stream)?);
        deliver(&mut client, account, settings, &from[0], &to, &cc, &message)?;
    }
    super::parser::parse(0, message.as_bytes(), true, true)
}

fn deliver<S: Read + Write>(
    client: &mut Client<S>,
    account: &Account,
    settings: &SmtpSettings,
    from: &str,
    to: &[String],
    cc: &[String],
    message: &str,
) -> Result<()> {
    client.command("EHLO localhost", 250)?;
    if settings.auth {
        let username = if settings.username.is_empty() {
            &account.email
        } else {
            &settings.username
        };
        if settings.xoauth2 {
            let token = accounts::token(account)?;
            let payload =
                STANDARD.encode(format!("user={username}\x01auth=Bearer {token}\x01\x01"));
            client.command(&format!("AUTH XOAUTH2 {payload}"), 235)?;
        } else {
            let password = accounts::smtp_password(account)?;
            if settings.plain {
                let payload = STANDARD.encode(format!("\0{username}\0{password}"));
                client.command(&format!("AUTH PLAIN {payload}"), 235)?;
            } else if settings.login {
                client.command("AUTH LOGIN", 334)?;
                client.command(&STANDARD.encode(username), 334)?;
                client.command(&STANDARD.encode(password), 235)?;
            } else {
                bail!("SMTP account has no supported authentication method");
            }
        }
    }
    client.command(&format!("MAIL FROM:<{from}>"), 250)?;
    for recipient in to.iter().chain(cc) {
        client.command(&format!("RCPT TO:<{recipient}>"), 250)?;
    }
    client.command("DATA", 354)?;
    client.write_data(message)?;
    let _ = client.command("QUIT", 221);
    Ok(())
}

fn addresses(input: &str) -> Result<Vec<String>> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for address in mailparse::addrparse(input)
        .context("Invalid email address")?
        .iter()
    {
        match address {
            MailAddr::Single(single) => result.push(single.addr.clone()),
            MailAddr::Group(group) => {
                result.extend(group.addrs.iter().map(|item| item.addr.clone()))
            }
        }
    }
    for address in &result {
        ensure!(
            address.contains('@')
                && !address
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "<>:,;".contains(c)),
            "Invalid email address: {address}"
        );
    }
    Ok(result)
}

fn encoded(value: &str) -> String {
    let mut words = Vec::new();
    let mut chunk = String::new();
    for character in value.chars() {
        if chunk.len() + character.len_utf8() > 42 {
            words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(&chunk)));
            chunk.clear();
        }
        chunk.push(character);
    }
    if !chunk.is_empty() {
        words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk)));
    }
    words.join("\r\n ")
}

fn part(content_type: &str, body: &str) -> String {
    let mut result = format!(
        "Content-Type: {content_type}; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n"
    );
    let encoded = STANDARD.encode(body);
    for line in encoded.as_bytes().chunks(76) {
        result.push_str(std::str::from_utf8(line).unwrap());
        result.push_str("\r\n");
    }
    result
}

fn message(account: &Account, draft: &Draft, to: &[String], cc: &[String]) -> Result<String> {
    ensure!(!draft.subject.contains(['\r', '\n']), "Invalid subject");
    let from = &account.email;
    let sender = if account.name.trim().is_empty() {
        from.to_owned()
    } else {
        format!("{} <{from}>", encoded(&account.name))
    };
    let mut output = format!("From: {sender}\r\nTo: {}\r\n", to.join(", "));
    if !cc.is_empty() {
        output.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    let domain = from.rsplit_once('@').map(|(_, domain)| domain).unwrap();
    output.push_str(&format!(
        "Subject: {}\r\nDate: {}\r\nMessage-ID: <{}.{}@{}>\r\nMIME-Version: 1.0\r\n",
        encoded(draft.subject.trim()),
        chrono::Utc::now().to_rfc2822(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        domain
    ));
    if let Some(reply) = &draft.in_reply_to {
        let reply = super::parser::normalize_message_id(reply);
        ensure!(valid_message_id(reply), "Invalid reply message ID");
        output.push_str(&format!("In-Reply-To: <{reply}>\r\n"));
    }
    if !draft.references.is_empty() {
        let mut references = Vec::new();
        for reference in &draft.references {
            let reference = super::parser::normalize_message_id(reference);
            ensure!(valid_message_id(reference), "Invalid referenced message ID");
            if !references.contains(&reference) {
                references.push(reference);
            }
        }
        output.push_str(&format!(
            "References: {}\r\n",
            references
                .iter()
                .map(|id| format!("<{id}>"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if let Some(html) = &draft.html {
        let boundary = format!(
            "brevlada-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        output.push_str(&format!(
            "Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\n"
        ));
        output.push_str(&part("text/plain", &draft.text));
        output.push_str(&format!("--{boundary}\r\n"));
        output.push_str(&part("text/html", html));
        output.push_str(&format!("--{boundary}--\r\n"));
    } else {
        output.push_str(&part("text/plain", &draft.text));
    }
    Ok(output)
}

fn valid_message_id(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_reply_round_trips_with_thread_headers() {
        let account = Account {
            path: String::new(),
            email: "me@example.com".into(),
            name: "Me".into(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let draft = Draft {
            to: "other@example.com".into(),
            cc: String::new(),
            subject: "Re: topic".into(),
            text: "Reply".into(),
            html: Some("<p>Reply</p>".into()),
            in_reply_to: Some("incoming@example.com".into()),
            references: vec!["original@example.com".into(), "incoming@example.com".into()],
        };
        let bytes = message(&account, &draft, &[draft.to.clone()], &[]).unwrap();
        let parsed = super::super::parser::parse(0, bytes.as_bytes(), true, true).unwrap();
        assert!(parsed.message_id.contains("@example.com"));
        assert!(
            parsed
                .references
                .contains(&"incoming@example.com".to_owned())
        );
        assert!(
            parsed
                .references
                .contains(&"original@example.com".to_owned())
        );
        assert_eq!(parsed.body_text.trim(), "Reply");
        assert_eq!(parsed.body_html.trim(), "<p>Reply</p>");
    }
}
