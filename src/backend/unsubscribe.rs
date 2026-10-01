use super::{connections::Connections, parser::normalize_message_id, storage::Storage};
use crate::models::Account;
use anyhow::{Context, Result, ensure};

pub enum Outcome {
    Open(String),
    Done,
}

pub fn execute(
    account: &Account,
    folder: &str,
    uid: u32,
    message_id: &str,
    storage: &Storage,
    connections: &mut Connections,
) -> Result<Outcome> {
    let message = storage
        .message(&account.email, folder, uid)?
        .context("Message is no longer cached")?;
    ensure!(
        normalize_message_id(&message.message_id) == normalize_message_id(message_id),
        "The mailbox changed. Reload the folder."
    );
    let mut unsubscribe = message
        .unsubscribe
        .context("No unsubscribe link in this message")?;
    if unsubscribe.one_click.is_none() {
        let validity = storage.validity(&account.email, folder)?;
        let header = connections.execute(account, |mail| {
            mail.header_with_validity(folder, uid, validity)
        })?;
        ensure!(
            normalize_message_id(&header.message_id) == normalize_message_id(message_id),
            "The mailbox changed. Reload the folder."
        );
        storage.store_unsubscribe(&account.email, folder, validity, &header)?;
        unsubscribe = header
            .unsubscribe
            .context("No unsubscribe link in this message")?;
    }
    if unsubscribe.one_click == Some(true) {
        match super::http::unsubscribe(&unsubscribe.url)? {
            super::http::UnsubscribeResponse::Done => Ok(Outcome::Done),
            super::http::UnsubscribeResponse::UseBrowser => Ok(Outcome::Open(unsubscribe.url)),
        }
    } else {
        Ok(Outcome::Open(unsubscribe.url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Message;

    #[test]
    fn leaves_get_and_mailto_links_for_the_desktop_handler_and_rejects_replaced_messages() {
        let account = Account {
            path: String::new(),
            email: "a".into(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        for url in [
            "https://example.org/preferences",
            "mailto:leave@example.org",
        ] {
            let raw = format!("Message-ID: <same>\r\nList-Unsubscribe: <{url}>\r\n\r\n");
            let message = super::super::parser::parse(2, raw.as_bytes(), false, false).unwrap();
            storage.store("a", "INBOX", &message).unwrap();
            let result = execute(
                &account,
                "INBOX",
                2,
                "same",
                &storage,
                &mut Connections::default(),
            )
            .unwrap();
            assert!(matches!(result, Outcome::Open(link) if link == url));
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        message_id: "different".into(),
                        ..message
                    },
                )
                .unwrap();
            let result = execute(
                &account,
                "INBOX",
                2,
                "same",
                &storage,
                &mut Connections::default(),
            );
            assert!(result.is_err());
        }
    }
}
