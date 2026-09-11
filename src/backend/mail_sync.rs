use super::{mail::Mail, parser};
use crate::models::Message;
use anyhow::{Context, Result, ensure};
use imap::types::{Flag, NameAttribute};

#[derive(Clone, Copy)]
pub struct Flags {
    pub uid: u32,
    pub read: bool,
    pub flagged: bool,
}

impl Mail {
    pub fn sync_folders(&mut self) -> Result<(Vec<String>, Vec<String>)> {
        let listed = self.session.list(None, Some("*"))?;
        let excluded: Vec<_> = listed
            .iter()
            .filter(|folder| excluded_folder(folder.name(), folder.attributes()))
            .map(|folder| (folder.name(), folder.delimiter()))
            .collect();
        let mut all = Vec::new();
        let mut sync = Vec::new();
        for folder in listed.iter() {
            if folder.attributes().contains(&NameAttribute::NoSelect) {
                continue;
            }
            let name = folder.name().to_owned();
            if !excluded.iter().any(|(parent, delimiter)| {
                name == *parent
                    || delimiter
                        .is_some_and(|delimiter| name.starts_with(&format!("{parent}{delimiter}")))
            }) {
                sync.push(name.clone());
            }
            all.push(name);
        }
        all.sort();
        sync.sort();
        Ok((all, sync))
    }

    pub fn inventory(&mut self, folder: &str) -> Result<(u32, Vec<Flags>)> {
        let mailbox = self.session.select(folder)?;
        let validity = mailbox.uid_validity.context("Missing UIDVALIDITY")?;
        if mailbox.exists == 0 {
            return Ok((validity, Vec::new()));
        }
        let fetched = self.session.uid_fetch("1:*", "(UID FLAGS)")?;
        let flags = fetched
            .iter()
            .map(|item| {
                Ok(Flags {
                    uid: item.uid.context("Missing message UID")?,
                    read: item.flags().contains(&Flag::Seen),
                    flagged: item.flags().contains(&Flag::Flagged),
                })
            })
            .collect::<Result<_>>()?;
        Ok((validity, flags))
    }

    pub fn header_batch(
        &mut self,
        folder: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<Vec<Message>> {
        ensure!(
            self.session.select(folder)?.uid_validity == Some(validity),
            "Mailbox changed during sync"
        );
        let sequence = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fetched = self
            .session
            .uid_fetch(sequence, "(UID FLAGS BODY.PEEK[HEADER])")?;
        fetched
            .iter()
            .map(|item| {
                let mut message = parser::parse(
                    item.uid.context("Missing message UID")?,
                    item.header().context("Missing message headers")?,
                    item.flags().contains(&Flag::Seen),
                    false,
                )?;
                message.is_flagged = item.flags().contains(&Flag::Flagged);
                Ok(message)
            })
            .collect()
    }

    pub fn cached_body(&mut self, folder: &str, validity: u32, uid: u32) -> Result<Message> {
        self.body_with_validity(folder, uid, Some(validity))
    }
}

fn excluded_folder(name: &str, attributes: &[NameAttribute<'_>]) -> bool {
    attributes.iter().any(|attribute| {
        matches!(attribute, NameAttribute::Custom(value)
        if value.eq_ignore_ascii_case("\\Trash") || value.eq_ignore_ascii_case("\\Junk"))
    }) || name.split(['/', '.']).any(|part| {
        matches!(
            part.to_lowercase().as_str(),
            "trash"
                | "spam"
                | "junk"
                | "junk e-mail"
                | "junk email"
                | "deleted items"
                | "deleted messages"
                | "bin"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_server_special_use_and_common_nested_names() {
        assert!(excluded_folder(
            "Papierkorb",
            &[NameAttribute::from("\\Trash")]
        ));
        assert!(excluded_folder(
            "Unwanted",
            &[NameAttribute::from("\\Junk")]
        ));
        assert!(excluded_folder("[Gmail]/Spam", &[]));
        assert!(excluded_folder("INBOX.Trash.Old", &[]));
        assert!(!excluded_folder("Work/Spam research", &[]));
        assert!(!excluded_folder("INBOX", &[]));
    }
}
