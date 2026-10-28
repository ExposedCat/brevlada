use super::{
    body_queue::BodyQueue,
    connections::Connections,
    mail_conversation,
    storage::{Storage, reconciliation::Snapshot},
    worker::{Command, Event},
};
use crate::models::SentMessage;
use anyhow::Result;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

type VerificationKey = (String, String, u32, u32, String);

const RECHECK: Duration = Duration::from_secs(300);

#[derive(Default)]
struct History {
    connections: Connections,
    folders: HashMap<String, (Instant, Vec<String>)>,
    searched: HashMap<(String, String, String), Instant>,
    verified: HashMap<VerificationKey, Instant>,
}

pub(super) fn start(
    path: PathBuf,
    commands: mpsc::Receiver<Command>,
    queue: BodyQueue,
    events: async_channel::Sender<Event>,
) {
    std::thread::spawn(move || {
        let mut history = History::default();
        while let Ok(mut command) = commands.recv() {
            for newer in commands.try_iter() {
                command = newer;
            }
            let Command::RelatedSent { selection, .. } = &command else {
                continue;
            };
            if !queue.current(*selection) {
                continue;
            }
            if let Err(error) = history.load(&path, &command, &queue, &events)
                && queue.current(*selection)
            {
                eprintln!("Could not load conversation history: {error}");
            }
        }
    });
}

impl History {
    fn load(
        &mut self,
        path: &std::path::Path,
        command: &Command,
        queue: &BodyQueue,
        events: &async_channel::Sender<Event>,
    ) -> Result<()> {
        let Command::RelatedSent {
            account,
            folder,
            outgoing,
            messages,
            selection,
            ..
        } = command
        else {
            return Ok(());
        };
        let storage = Storage::open(path)?;
        let mut related = storage.related_messages(&account.email, folder, messages, *outgoing)?;
        if mail_conversation::terms(&account.email, messages).is_empty() {
            return Ok(());
        }
        self.searched
            .retain(|_, checked| checked.elapsed() < RECHECK);
        if self.searched.len() > 4096 {
            self.searched.clear();
        }
        if self
            .folders
            .get(&account.email)
            .is_none_or(|(checked, _)| checked.elapsed() >= RECHECK)
        {
            let folders = self
                .connections
                .execute_body(account, queue, *selection, |mail| {
                    mail.conversation_folders()
                })?;
            self.folders
                .insert(account.email.clone(), (Instant::now(), folders));
        }
        let folders = self.folders[&account.email].1.clone();
        loop {
            let terms = mail_conversation::terms(
                &account.email,
                messages
                    .iter()
                    .chain(related.iter().map(|item| &item.message)),
            );
            let mut searched = false;
            for target in &folders {
                if !queue.current(*selection) {
                    return Ok(());
                }
                let pending: Vec<_> = terms
                    .iter()
                    .filter(|term| {
                        !self.searched.contains_key(&(
                            account.email.clone(),
                            target.clone(),
                            (*term).clone(),
                        ))
                    })
                    .cloned()
                    .collect();
                for terms in pending.chunks(16) {
                    searched = true;
                    let expected = storage.validity(&account.email, target)?;
                    let (validity, uids) =
                        self.connections
                            .execute_body(account, queue, *selection, |mail| {
                                mail.conversation_uids(target, terms)
                            })?;
                    if !queue.current(*selection) {
                        return Ok(());
                    }
                    if expected != Some(validity)
                        && !storage.store_conversation_headers(
                            &account.email,
                            target,
                            expected,
                            validity,
                            &[],
                        )?
                    {
                        return Ok(());
                    }
                    if expected != Some(validity) {
                        update(&storage, command, &mut related, queue, events)?;
                    }
                    let missing = storage.missing_conversation_uids(
                        &account.email,
                        target,
                        validity,
                        &uids,
                    )?;
                    let mut expected = Some(validity);
                    for batch in missing.chunks(64) {
                        let headers =
                            self.connections
                                .execute_body(account, queue, *selection, |mail| {
                                    mail.conversation_headers(target, validity, batch)
                                })?;
                        if !queue.current(*selection) {
                            return Ok(());
                        }
                        if !storage.store_conversation_headers(
                            &account.email,
                            target,
                            expected,
                            validity,
                            &headers,
                        )? {
                            return Ok(());
                        }
                        expected = Some(validity);
                        update(&storage, command, &mut related, queue, events)?;
                    }
                    for term in terms {
                        self.searched.insert(
                            (account.email.clone(), target.clone(), term.clone()),
                            Instant::now(),
                        );
                    }
                }
            }
            if !searched {
                self.reconcile(&storage, command, &mut related, queue, events)?;
                return Ok(());
            }
        }
    }

    fn reconcile(
        &mut self,
        storage: &Storage,
        command: &Command,
        related: &mut Vec<SentMessage>,
        queue: &BodyQueue,
        events: &async_channel::Sender<Event>,
    ) -> Result<()> {
        let Command::RelatedSent {
            account,
            messages,
            selection,
            ..
        } = command
        else {
            return Ok(());
        };
        self.verified
            .retain(|_, checked| checked.elapsed() < RECHECK);
        if self.verified.len() > 4096 {
            self.verified.clear();
        }
        let snapshots = storage.conversation_snapshots(
            &account.email,
            messages
                .iter()
                .chain(related.iter().map(|item| &item.message)),
        )?;
        for mut snapshot in snapshots {
            snapshot.messages.retain(|message| {
                !self.verified.contains_key(&verification_key(
                    &snapshot.account,
                    &snapshot.folder,
                    snapshot.validity,
                    message,
                ))
            });
            for batch in snapshot.messages.chunks(128) {
                if !queue.current(*selection) {
                    return Ok(());
                }
                let uids: Vec<_> = batch.iter().map(|message| message.uid).collect();
                let present =
                    self.connections
                        .execute_body(account, queue, *selection, |mail| {
                            mail.conversation_present(&snapshot.folder, snapshot.validity, &uids)
                        })?;
                if !queue.current(*selection) {
                    return Ok(());
                }
                let batch = Snapshot {
                    account: snapshot.account.clone(),
                    folder: snapshot.folder.clone(),
                    validity: snapshot.validity,
                    messages: batch.to_vec(),
                };
                let removed = storage.reconcile_conversation(&batch, &present)?;
                for message in batch
                    .messages
                    .iter()
                    .filter(|message| present.contains(&message.uid))
                {
                    self.verified.insert(
                        verification_key(&batch.account, &batch.folder, batch.validity, message),
                        Instant::now(),
                    );
                }
                if !removed.is_empty() {
                    events.send_blocking(Event::CachePruned(
                        batch.account,
                        batch.folder,
                        removed,
                    ))?;
                    update(storage, command, related, queue, events)?;
                }
            }
        }
        Ok(())
    }
}

fn verification_key(
    account: &str,
    folder: &str,
    validity: u32,
    message: &crate::models::Message,
) -> VerificationKey {
    (
        account.to_owned(),
        folder.to_owned(),
        validity,
        message.uid,
        message.message_id.clone(),
    )
}

fn update(
    storage: &Storage,
    command: &Command,
    related: &mut Vec<SentMessage>,
    queue: &BodyQueue,
    events: &async_channel::Sender<Event>,
) -> Result<()> {
    let Command::RelatedSent {
        account,
        folder,
        messages,
        outgoing,
        generation,
        selection,
    } = command
    else {
        return Ok(());
    };
    let updated = storage.related_messages(&account.email, folder, messages, *outgoing)?;
    if identities(related) != identities(&updated) && queue.current(*selection) {
        *related = updated;
        let mut displayed = related.clone();
        for item in &mut displayed {
            item.message.prepare_display();
        }
        events.send_blocking(Event::ConversationHistory(
            *generation,
            *selection,
            displayed,
        ))?;
    }
    Ok(())
}

fn identities(messages: &[SentMessage]) -> Vec<(&str, u32, Option<u32>, &str)> {
    messages
        .iter()
        .map(|item| {
            (
                item.folder.as_str(),
                item.message.uid,
                item.message.uid_validity,
                item.message.message_id.as_str(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, Message};

    #[test]
    fn reuses_recent_searches_and_rechecks_expired_results_without_blocking_cached_mail() {
        let account = Account {
            email: "me@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: false,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let messages = vec![Message {
            message_id: "latest".into(),
            subject: "Appointment".into(),
            sender: "salon@example.com".into(),
            recipients: account.email.clone(),
            ..Default::default()
        }];
        let terms = mail_conversation::terms(&account.email, &messages);
        let mut history = History::default();
        history.folders.insert(
            account.email.clone(),
            (Instant::now(), vec!["All Mail".into()]),
        );
        for term in &terms {
            history.searched.insert(
                (account.email.clone(), "All Mail".into(), term.clone()),
                Instant::now(),
            );
        }
        let command = Command::RelatedSent {
            account,
            folder: "INBOX".into(),
            outgoing: false,
            messages,
            generation: 1,
            selection: 2,
        };
        let queue = BodyQueue::default();
        queue.select(2);
        let (events, received) = async_channel::unbounded();
        for _ in 0..2 {
            history
                .load(std::path::Path::new(":memory:"), &command, &queue, &events)
                .unwrap();
        }
        assert!(received.try_recv().is_err());
        for checked in history.searched.values_mut() {
            *checked = Instant::now() - RECHECK;
        }
        let error = history
            .load(std::path::Path::new(":memory:"), &command, &queue, &events)
            .unwrap_err();
        assert!(error.to_string().contains("SSL or STARTTLS"));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn reconciliation_reuses_verified_uids_and_preserves_cache_on_network_failure() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "latest".into(),
            sender: "salon@example.com".into(),
            subject: "Appointment".into(),
            ..Default::default()
        };
        storage
            .store_conversation_headers(
                "me@example.com",
                "Archive",
                None,
                7,
                std::slice::from_ref(&message),
            )
            .unwrap();
        let command = Command::RelatedSent {
            account: Account {
                email: "me@example.com".into(),
                path: String::new(),
                name: String::new(),
                host: String::new(),
                username: String::new(),
                port: 993,
                ssl: false,
                tls: false,
                oauth2: false,
                smtp: None,
            },
            folder: "INBOX".into(),
            outgoing: false,
            messages: vec![message.clone()],
            generation: 1,
            selection: 2,
        };
        let mut history = History::default();
        history.verified.insert(
            verification_key("me@example.com", "Archive", 7, &message),
            Instant::now(),
        );
        let queue = BodyQueue::default();
        queue.select(2);
        let (events, received) = async_channel::unbounded();
        history
            .reconcile(&storage, &command, &mut vec![], &queue, &events)
            .unwrap();
        assert!(received.try_recv().is_err());
        for checked in history.verified.values_mut() {
            *checked = Instant::now() - RECHECK;
        }
        let error = history
            .reconcile(&storage, &command, &mut vec![], &queue, &events)
            .unwrap_err();
        assert!(error.to_string().contains("SSL or STARTTLS"));
        assert!(history.verified.is_empty());
        assert!(received.try_recv().is_err());
        assert!(
            storage
                .message("me@example.com", "Archive", 1)
                .unwrap()
                .is_some()
        );
    }
}
