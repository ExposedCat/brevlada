use super::conversation_query::query;
pub(super) use super::conversation_query::{Term, terms};
use super::{mail::Mail, mail_headers, special_use};
use crate::models::Message;
use anyhow::{Context, Result, ensure};
use imap::{Session, types::NameAttribute};
use std::io::{Read, Write};

fn folders<T: Read + Write>(session: &mut Session<T>) -> Result<Vec<String>> {
    let listed = session.list(None, Some("*"))?;
    let excluded: Vec<_> = listed
        .iter()
        .filter(|folder| super::mail_sync::excluded_folder(folder.name(), folder.attributes()))
        .map(|folder| (folder.name(), folder.delimiter()))
        .collect();
    let included = |name: &str| {
        !excluded.iter().any(|(parent, delimiter)| {
            name == *parent
                || delimiter
                    .is_some_and(|delimiter| name.starts_with(&format!("{parent}{delimiter}")))
        })
    };
    let all = listed.iter().find(|folder| {
        !folder.attributes().contains(&NameAttribute::NoSelect)
            && included(folder.name())
            && special_use::marked(folder.attributes(), "\\All")
    });
    let mut folders: Vec<_> = listed
        .iter()
        .filter(|folder| {
            !folder.attributes().contains(&NameAttribute::NoSelect)
                && included(folder.name())
                && (all.is_none()
                    || all.is_some_and(|all| all.name() == folder.name())
                    || special_use::marked(folder.attributes(), "\\Drafts"))
        })
        .map(|folder| folder.name().to_owned())
        .collect();
    folders.sort();
    Ok(folders)
}

fn search<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    terms: &[Term],
) -> Result<(u32, Vec<u32>)> {
    ensure!(!terms.is_empty(), "Conversation search needs a target");
    let mailbox = session.examine(folder)?;
    let validity = mailbox
        .uid_validity
        .context("Missing UIDVALIDITY during conversation search")?;
    let literal_limit = if terms.iter().any(Term::needs_utf8) {
        let capabilities = session.capabilities()?;
        if capabilities.has_str("LITERAL+") {
            usize::MAX
        } else if capabilities.has_str("LITERAL-") {
            4096
        } else {
            0
        }
    } else {
        0
    };
    let search = query(terms, literal_limit);
    ensure!(
        !search.is_empty(),
        "Server cannot search these conversation identifiers"
    );
    let result = session.uid_search(&search);
    let result = match result {
        Err(imap::Error::Bad(_) | imap::Error::No(_)) if !search.is_ascii() => {
            let ascii = query(terms, 0);
            ensure!(
                !ascii.is_empty(),
                "Server does not support UTF-8 conversation searches"
            );
            session.uid_search(ascii)
        }
        result => result,
    };
    let mut uids: Vec<_> = result
        .with_context(|| format!("Searching conversation history in {folder}"))?
        .into_iter()
        .collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    Ok((validity, uids))
}

fn present<T: Read + Write>(
    session: &mut Session<T>,
    folder: &str,
    validity: u32,
    uids: &[u32],
) -> Result<Vec<u32>> {
    ensure!(
        !uids.is_empty() && !uids.contains(&0),
        "Conversation reconciliation needs mailbox UIDs"
    );
    ensure!(
        session.examine(folder)?.uid_validity == Some(validity),
        "Mailbox changed during conversation reconciliation"
    );
    let sequence = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetched = session.uid_fetch(sequence, "(UID)")?;
    Ok(fetched
        .iter()
        .filter_map(|item| item.uid)
        .filter(|uid| uids.contains(uid))
        .collect())
}

impl Mail {
    pub(super) fn conversation_present(
        &mut self,
        folder: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<Vec<u32>> {
        present(&mut self.session, folder, validity, uids)
    }

    pub(super) fn conversation_folders(&mut self) -> Result<Vec<String>> {
        folders(&mut self.session)
    }

    pub(super) fn conversation_uids(
        &mut self,
        folder: &str,
        terms: &[Term],
    ) -> Result<(u32, Vec<u32>)> {
        search(&mut self.session, folder, terms)
    }

    pub(super) fn conversation_headers(
        &mut self,
        folder: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<Vec<Message>> {
        ensure!(
            self.session.examine(folder)?.uid_validity == Some(validity),
            "Mailbox changed during conversation search"
        );
        mail_headers::fetch(
            &mut self.session,
            folder,
            self.drafts_folder.as_deref(),
            validity,
            uids,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::special_use::tests::session;

    #[test]
    fn uses_all_mail_once_and_excludes_trash_and_junk() {
        let (mut session, _) = session(concat!(
            "* LIST (\\All) \"/\" \"[Gmail]/All Mail\"\r\n",
            "* LIST () \"/\" INBOX\r\n",
            "* LIST (\\Sent) \"/\" Sent\r\n",
            "* LIST (\\Drafts) \"/\" Drafts\r\n",
            "* LIST (\\Trash) \"/\" Deleted\r\n",
            "a2 OK list\r\n"
        ));
        assert_eq!(
            folders(&mut session).unwrap(),
            ["Drafts", "[Gmail]/All Mail"]
        );
        let (mut session, _) = crate::backend::special_use::tests::session(concat!(
            "* LIST (\\Noselect) \"/\" Parent\r\n",
            "* LIST () \"/\" INBOX\r\n",
            "* LIST (\\Archive) \"/\" Archive\r\n",
            "* LIST () \"/\" Sent\r\n",
            "* LIST (\\Junk) \"/\" Junk\r\n",
            "* LIST () \"/\" Junk/Old\r\n",
            "a2 OK list\r\n"
        ));
        assert_eq!(folders(&mut session).unwrap(), ["Archive", "INBOX", "Sent"]);
    }

    #[test]
    fn searches_headers_with_quoted_values_and_never_fetches_bodies_or_sets_flags() {
        let message = Message {
            subject: "Re: Woman Haircut Appointment with Mark".into(),
            sender: "POP <pop@trichomania.cz>".into(),
            recipients: "me@example.com".into(),
            message_id: "reply".into(),
            references: vec!["previous".into()],
            ..Default::default()
        };
        let terms = terms("me@example.com", [&message]);
        let (mut session, commands) = session(
            "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n* SEARCH 2 3\r\na3 OK search\r\n",
        );
        assert_eq!(
            search(&mut session, "Archive", &terms).unwrap(),
            (7, vec![3, 2])
        );
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(sent.contains("EXAMINE \"Archive\""), "{sent}");
        assert!(
            sent.contains(
                "SUBJECT \"woman haircut appointment with mark\" OR FROM \"pop@trichomania.cz\""
            ),
            "{sent}"
        );
        assert!(sent.contains("HEADER References \"previous\""));
        assert!(!sent.contains("FETCH") && !sent.contains("STORE") && !sent.contains("SEARCH ALL"));
    }

    #[test]
    fn unicode_history_uses_advertised_literals_or_ascii_candidates() {
        let message = Message {
            subject: "Re: příští reservation".into(),
            sender: "salon@example.com".into(),
            ..Default::default()
        };
        let terms = terms("me@example.com", [&message]);
        for capability in [" LITERAL+", " LITERAL-", ""] {
            let (mut session, commands) = session(&format!(
                "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n* CAPABILITY IMAP4rev1{capability}\r\na3 OK caps\r\n* SEARCH 41\r\na4 OK search\r\n"
            ));
            assert_eq!(
                search(&mut session, "Archive", &terms).unwrap(),
                (7, vec![41])
            );
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            if capability.is_empty() {
                assert!(
                    sent.contains("UID SEARCH (SUBJECT \"reservation\""),
                    "{sent:?}"
                );
                assert!(sent.is_ascii());
            } else {
                assert!(
                    sent.contains("UID SEARCH CHARSET UTF-8 (SUBJECT {22+}\r\npříští reservation"),
                    "{sent:?}"
                );
            }
            assert!(
                !sent.contains("FETCH") && !sent.contains("STORE") && !sent.contains("SEARCH ALL")
            );
        }
    }

    #[test]
    fn servers_rejecting_utf8_are_retried_without_losing_candidate_uids() {
        let message = Message {
            subject: "予約".into(),
            sender: "salon@example.com".into(),
            ..Default::default()
        };
        let terms = terms("me@example.com", [&message]);
        for rejection in [
            "BAD Could not parse command",
            "NO [BADCHARSET (US-ASCII)] Unsupported charset",
        ] {
            let (mut session, commands) = session(&format!(
                "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n* CAPABILITY IMAP4rev1 LITERAL+\r\na3 OK caps\r\na4 {rejection}\r\n* SEARCH 41 42\r\na5 OK search\r\n"
            ));
            assert_eq!(
                search(&mut session, "Archive", &terms).unwrap(),
                (7, vec![42, 41])
            );
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(
                sent.contains("a4 UID SEARCH CHARSET UTF-8 (SUBJECT {6+}\r\n予約"),
                "{sent:?}"
            );
            assert!(
                sent.contains("a5 UID SEARCH (OR FROM \"salon@example.com\""),
                "{sent:?}"
            );
            assert!(!sent.contains("SEARCH ALL"));
        }
    }

    #[test]
    fn ascii_fallback_candidates_still_require_the_full_unicode_conversation_key() {
        let storage =
            crate::backend::storage::Storage::open(std::path::Path::new(":memory:")).unwrap();
        let latest = Message {
            uid: 1,
            subject: "予約".into(),
            message_id: "latest".into(),
            sender: "salon@example.com".into(),
            recipients: "me@example.com".into(),
            ..Default::default()
        };
        let matching = Message {
            uid: 2,
            subject: "Re: 予約".into(),
            message_id: "related".into(),
            ..latest.clone()
        };
        let unrelated = Message {
            uid: 3,
            subject: "確認".into(),
            message_id: "other".into(),
            ..latest.clone()
        };
        storage
            .store_conversation_headers(
                "me@example.com",
                "Archive",
                None,
                7,
                &[matching, unrelated],
            )
            .unwrap();
        let related = storage
            .related_messages("me@example.com", "INBOX", &[latest], false)
            .unwrap();
        assert_eq!(
            related
                .iter()
                .map(|item| item.message.uid)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn reconciliation_probes_exact_uids_without_bodies_or_flag_changes() {
        let (mut session, commands) = session(concat!(
            "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n",
            "* 1 FETCH (UID 41)\r\n* 2 FETCH (FLAGS (\\Seen))\r\n* 9 FETCH (UID 900)\r\na3 OK fetch\r\n"
        ));
        assert_eq!(
            present(&mut session, "All Mail", 7, &[41, 42]).unwrap(),
            [41]
        );
        let written = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(written.contains("UID FETCH 41,42 (UID)"), "{written}");
        assert!(
            !written.contains("BODY")
                && !written.contains("STORE")
                && !written.contains("SEARCH ALL")
        );
        let (mut session, _) = crate::backend::special_use::tests::session(
            "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\na3 OK fetch\r\n",
        );
        assert!(
            present(&mut session, "All Mail", 7, &[41])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reconciliation_rejects_failed_fetches_and_changed_uid_validity() {
        for response in [
            "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\na3 NO failed\r\n",
            "* OK [UIDVALIDITY 8] valid\r\na2 OK examine\r\n",
            "* OK [UIDVALIDITY 7] valid\r\na2 OK examine\r\n* 1 FETCH (UID 41)\r\na3 NO incomplete\r\n",
        ] {
            let (mut session, _) = session(response);
            assert!(present(&mut session, "All Mail", 7, &[41, 42]).is_err());
        }
    }
}
