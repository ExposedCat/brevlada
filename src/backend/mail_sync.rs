use super::mail::Mail;
use crate::models::Message;
use anyhow::{Context, Result, ensure};
use imap::types::{Flag, NameAttribute};

pub struct Folders {
    pub all: Vec<String>,
    pub sync: Vec<String>,
    pub sent: Option<String>,
    pub drafts: Option<String>,
}

fn named_folder(folder: &str, names: &[&str]) -> bool {
    names.contains(
        &folder
            .rsplit(['/', '.'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
    )
}

fn has_special_use(attributes: &[NameAttribute<'_>], value: &str) -> bool {
    attributes.iter().any(|attribute| {
        matches!(attribute, NameAttribute::Custom(name) if name.eq_ignore_ascii_case(value))
    })
}

fn is_draft(folder: &str, flags: &[Flag<'_>], drafts: Option<&str>) -> bool {
    flags.contains(&Flag::Draft)
        || drafts == Some(folder)
        || named_folder(folder, &["draft", "drafts"])
}

pub(super) fn apply_flags(
    message: &mut Message,
    folder: &str,
    flags: &[Flag<'_>],
    drafts: Option<&str>,
) {
    message.is_flagged = flags.contains(&Flag::Flagged);
    message.is_draft = is_draft(folder, flags, drafts);
    ensure_draft_id(message, folder);
}

pub(super) fn ensure_draft_id(message: &mut Message, folder: &str) {
    if message.is_draft && message.message_id.is_empty() {
        // Some clients do not assign a Message-ID until the draft is sent.
        message.message_id = format!("brevlada-draft:{}:{}", folder, message.uid);
    }
}

#[derive(Clone, Copy)]
pub struct Flags {
    pub uid: u32,
    pub read: bool,
    pub flagged: bool,
    pub draft: bool,
}

impl Mail {
    pub fn sync_folders(&mut self) -> Result<Folders> {
        let listed = self.session.list(None, Some("*"))?;
        let special = |attribute: &str, names: &[&str]| {
            listed
                .iter()
                .filter(|folder| !folder.attributes().contains(&NameAttribute::NoSelect))
                .find(|folder| has_special_use(folder.attributes(), attribute))
                .or_else(|| {
                    listed
                        .iter()
                        .filter(|folder| !folder.attributes().contains(&NameAttribute::NoSelect))
                        .find(|folder| named_folder(folder.name(), names))
                })
                .map(|folder| folder.name().to_owned())
        };
        let sent = special(
            "\\Sent",
            &["sent", "sent mail", "sent items", "sent messages"],
        );
        let drafts = special("\\Drafts", &["draft", "drafts"]);
        self.drafts_folder = drafts.clone();
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
        Ok(Folders {
            all,
            sync,
            sent,
            drafts,
        })
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
            .filter(|item| item.uid.is_some())
            .map(|item| {
                Ok(Flags {
                    uid: item.uid.context("Missing message UID")?,
                    read: item.flags().contains(&Flag::Seen),
                    flagged: item.flags().contains(&Flag::Flagged),
                    draft: is_draft(folder, item.flags(), self.drafts_folder.as_deref()),
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
        super::mail_headers::fetch(
            &mut self.session,
            folder,
            self.drafts_folder.as_deref(),
            validity,
            uids,
        )
    }

    pub fn cached_body(&mut self, folder: &str, validity: u32, uid: u32) -> Result<Message> {
        self.body_with_validity(folder, uid, Some(validity), false)
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
    use crate::backend::parser;

    #[test]
    fn recognizes_drafts_and_gives_idless_headers_a_stable_identity() {
        assert!(has_special_use(
            &[NameAttribute::from("\\drafts")],
            "\\Drafts"
        ));
        assert!(is_draft("Entwürfe", &[], Some("Entwürfe")));
        assert!(is_draft("Work", &[Flag::Draft], None));
        assert!(is_draft("[Gmail]/Drafts", &[], None));
        assert!(!is_draft("Drafts research", &[], None));
        let mut header = parser::parse(7, b"To: other@example.com\r\n\r\n", false, false).unwrap();
        apply_flags(&mut header, "Entwürfe", &[], Some("Entwürfe"));
        let mut body =
            parser::parse(7, b"To: other@example.com\r\n\r\nHello", false, true).unwrap();
        apply_flags(&mut body, "Entwürfe", &[Flag::Draft], None);
        assert!(header.is_draft);
        assert!(!header.message_id.is_empty());
        assert_eq!(header.message_id, body.message_id);
    }

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
