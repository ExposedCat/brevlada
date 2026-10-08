use super::{accounts, avatar_queue::AvatarQueue, connections::Connections, storage::Storage};
use crate::models::{Account, Draft, Message, SentMessage};
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

pub enum Command {
    FlushNotifications,
    NotificationsHandled {
        account: String,
        folder: String,
        validity: u32,
        uids: Vec<u32>,
    },
    OpenNotification {
        account: String,
        validity: u32,
        uid: u32,
    },
    CalendarHistory {
        account: Account,
        uid: String,
        generation: u64,
    },
    CalendarReply {
        account: Account,
        message: Box<Message>,
        event: Box<crate::models::calendar::Event>,
        response: crate::models::calendar::Response,
    },
    ParcelHistory {
        account: Account,
        folder: String,
        sender: String,
        identifier: crate::models::parcel::Identifier,
        generation: u64,
        selection: u64,
    },
    Unsubscribe {
        account: Account,
        folder: String,
        uid: u32,
        message_id: String,
    },
    SaveDraft {
        account: Account,
        draft: Draft,
        target: Option<crate::models::draft::Target>,
        request: u64,
    },
    Compose {
        account: Account,
        draft: Draft,
        target: Option<crate::models::draft::Target>,
        request: u64,
    },
    DeleteDraft {
        account: Account,
        target: crate::models::draft::Target,
        request: u64,
    },
    RelatedSent {
        account: Account,
        folder: String,
        messages: Vec<Message>,
        generation: u64,
        selection: u64,
    },
    SentBody {
        account: Account,
        folder: String,
        uid: u32,
        generation: u64,
        selection: u64,
    },
    UndoSpam {
        account: Account,
        folder: String,
        originals: Vec<Message>,
    },
    SenderAction {
        account: Account,
        folder: String,
        sender: crate::models::action_target::ActionTarget,
        action: crate::models::sender_action::SenderAction,
    },
    Discover,
    Sync,
    Folders(Account),
    Load {
        account: Account,
        folder: String,
        generation: u64,
    },
    LoadSenderPage {
        account: Account,
        folder: String,
        sender: String,
        generation: u64,
        sender_generation: u64,
        before: Option<(i64, u32)>,
        limit: usize,
    },
}

pub enum Event {
    NewMail(String, String, u32, Vec<Message>),
    NotificationOpened(String, Result<Option<Message>, String>),
    CalendarHistory(u64, String, Result<Vec<Message>, String>),
    CalendarReplied {
        account: String,
        uid: String,
        recurrence_id: Option<String>,
        result: Result<Message, String>,
    },
    ParcelHistory(
        u64,
        u64,
        String,
        crate::models::parcel::Identifier,
        Vec<Message>,
    ),
    Unsubscribe {
        account: String,
        folder: String,
        uid: u32,
        message_id: String,
        result: Result<super::unsubscribe::Outcome, String>,
    },
    DraftSaved(u64, Result<crate::models::draft::Saved, String>),
    Composed(u64, Result<crate::models::draft::Outcome, String>),
    DraftDeleted(u64, Result<(), String>),
    SentCacheChanged(String),
    RelatedSent(u64, u64, Vec<SentMessage>),
    SentBody(u64, u64, String, u32, Result<Message, String>),
    SpamUndoFinished {
        account: String,
        folder: String,
        originals: Vec<Message>,
        restored: Vec<Message>,
        messages: Option<Vec<Message>>,
        error: Option<String>,
    },
    SenderActionFinished {
        account: String,
        folder: String,
        sender: crate::models::action_target::ActionTarget,
        messages: Option<Vec<Message>>,
        error: Option<String>,
        removed: Vec<Message>,
    },
    Accounts(Vec<Account>),
    SidebarReady,
    Folders(String, Vec<String>),
    Unread(String, Vec<(String, bool)>),
    UnreadSnapshot(String, Vec<(String, bool)>),
    Messages(u64, Vec<Message>, bool),
    SenderPage(u64, u64, Vec<Message>, bool),
    SenderPageError(u64, u64, String),
    Body(u64, u64, Message),
    BodyError(u64, u64, u32, String),
    Preview(u64, u64, Message),
    PreviewError(u64, u64, u32),
    CacheList(String, String, Vec<Message>),
    CacheBody(String, String, Message),
    Avatar(String, Option<Vec<u8>>),
    Error(Option<u64>, String),
}

#[derive(Clone)]
pub struct BodyRequest {
    pub account: Account,
    pub folder: String,
    pub uid: u32,
    pub generation: u64,
    pub selection: u64,
    pub mark_read: bool,
}

pub struct Worker {
    request: AtomicU64,
    commands: mpsc::Sender<Command>,
    cache_commands: mpsc::Sender<Command>,
    parcel_commands: mpsc::Sender<Command>,
    background: super::sync_queue::SyncQueue,
    bodies: super::body_queue::BodyQueue,
    avatars: AvatarQueue,
}

impl Worker {
    #[cfg(test)]
    pub fn disconnected() -> Self {
        Self::recording().0
    }

    #[cfg(test)]
    pub fn recording() -> (Self, mpsc::Receiver<Command>) {
        let (commands, receiver) = mpsc::channel();
        (
            Self {
                request: AtomicU64::default(),
                background: super::sync_queue::SyncQueue::default(),
                cache_commands: commands.clone(),
                parcel_commands: commands.clone(),
                commands,
                bodies: super::body_queue::BodyQueue::default(),
                avatars: AvatarQueue::default(),
            },
            receiver,
        )
    }

    pub fn send(&self, command: Command) -> Result<()> {
        let channel = if matches!(command, Command::ParcelHistory { .. }) {
            &self.parcel_commands
        } else if matches!(
            command,
            Command::LoadSenderPage { .. }
                | Command::RelatedSent { .. }
                | Command::CalendarHistory { .. }
        ) {
            &self.cache_commands
        } else {
            &self.commands
        };
        channel
            .send(command)
            .map_err(|_| anyhow::anyhow!("Mail service stopped"))
    }
    pub fn next_request(&self) -> u64 {
        self.request.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub fn register(&self, account: Account, expanded: bool) {
        self.avatars.register(account.clone());
        self.background.register(account, expanded);
    }
    pub fn retain_accounts(&self, accounts: &[Account]) {
        self.background.retain_accounts(accounts);
        self.avatars.retain_accounts(accounts);
    }
    /// Handle used by the sender list to ask for avatars as rows appear.
    pub fn avatars(&self) -> AvatarQueue {
        self.avatars.clone()
    }
    pub fn expanded(&self, email: &str, expanded: bool) {
        self.background.expanded(email, expanded);
    }
    pub fn sync(&self) {
        let _ = self.send(Command::Sync);
    }
    pub fn sync_status(&self) -> Option<crate::models::sync::Status> {
        self.background.status()
    }
    pub fn focus(&self, account: &str, folder: &str) {
        self.background.focus(account, folder);
    }
    pub fn select(&self, selection: u64) {
        self.bodies.select(selection);
    }
    pub fn body(&self, request: BodyRequest) {
        self.bodies.push(request);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Worker {
    pub fn stop(&self) {
        self.bodies.close();
        self.background.close();
        self.avatars.close();
    }
}

pub fn start(path: PathBuf) -> (Worker, async_channel::Receiver<Event>) {
    let (sender, receiver) = mpsc::channel();
    let (cache_sender, cache_receiver) = mpsc::channel();
    let (parcel_sender, parcel_receiver) = mpsc::channel();
    let (events, results) = async_channel::unbounded();
    let bodies = super::body_queue::BodyQueue::default();
    for _ in 0..2 {
        super::body_worker::start(path.clone(), bodies.clone(), events.clone());
    }
    let avatars = AvatarQueue::default();
    for _ in 0..2 {
        super::avatar_worker::start(path.clone(), avatars.clone(), events.clone());
    }
    let background = super::sync_queue::SyncQueue::default();
    super::sync_worker::start(path.clone(), background.clone(), events.clone());
    start_commands(path.clone(), receiver, events.clone(), background.clone());
    super::parcel_worker::start(
        path.clone(),
        parcel_receiver,
        bodies.clone(),
        events.clone(),
    );
    start_cache_reads(path, cache_receiver, events);
    (
        Worker {
            request: AtomicU64::default(),
            commands: sender,
            cache_commands: cache_sender,
            parcel_commands: parcel_sender,
            background,
            bodies,
            avatars,
        },
        results,
    )
}

fn start_cache_reads(
    path: PathBuf,
    receiver: mpsc::Receiver<Command>,
    events: async_channel::Sender<Event>,
) {
    std::thread::spawn(move || {
        let mut storage = None;
        while let Ok(command) = receiver.recv() {
            let result = (|| -> Result<Event> {
                if storage.is_none() {
                    storage = Some(Storage::open_read_only(&path)?);
                }
                let storage = storage.as_ref().unwrap();
                Ok(match &command {
                    Command::LoadSenderPage {
                        account,
                        folder,
                        sender,
                        generation,
                        sender_generation,
                        before,
                        limit,
                    } => {
                        let mut messages = storage.sender_page(
                            &account.email,
                            folder,
                            sender,
                            *before,
                            *limit + 1,
                        )?;
                        let has_more = messages.len() > *limit;
                        messages.truncate(*limit);
                        Event::SenderPage(*generation, *sender_generation, messages, has_more)
                    }
                    Command::CalendarHistory {
                        account,
                        uid,
                        generation,
                    } => Event::CalendarHistory(
                        *generation,
                        uid.clone(),
                        storage
                            .calendar_messages(&account.email, uid)
                            .map_err(|error| error.to_string()),
                    ),
                    Command::RelatedSent {
                        account,
                        folder,
                        messages,
                        generation,
                        selection,
                    } => Event::RelatedSent(
                        *generation,
                        *selection,
                        storage
                            .related_sent(&account.email, folder, messages)?
                            .into_iter()
                            .map(|mut item| {
                                item.message.prepare_display();
                                item
                            })
                            .collect(),
                    ),
                    _ => unreachable!("only cache commands reach this worker"),
                })
            })();
            let event = match result {
                Ok(event) => event,
                Err(error) => match command {
                    Command::LoadSenderPage {
                        generation,
                        sender_generation,
                        ..
                    } => Event::SenderPageError(generation, sender_generation, error.to_string()),
                    Command::RelatedSent { .. } => {
                        Event::Error(None, format!("Could not load outgoing messages: {error}"))
                    }
                    _ => continue,
                },
            };
            if events.send_blocking(event).is_err() {
                break;
            }
        }
    });
}

fn start_commands(
    path: PathBuf,
    receiver: mpsc::Receiver<Command>,
    events: async_channel::Sender<Event>,
    background: super::sync_queue::SyncQueue,
) {
    std::thread::spawn(move || {
        let mut storage = match Storage::open(&path) {
            Ok(storage) => storage,
            Err(error) => {
                let _ = events.send_blocking(Event::Error(
                    None,
                    format!("Could not open email cache: {error}"),
                ));
                return;
            }
        };
        let mut connections = Connections::default();
        while let Ok(command) = receiver.recv() {
            let generation = match &command {
                Command::Load { generation, .. } => Some(*generation),
                _ => None,
            };
            if let Err(error) = execute(
                command,
                &mut storage,
                &mut connections,
                &events,
                &background,
            ) && events
                .send_blocking(Event::Error(generation, error.to_string()))
                .is_err()
            {
                break;
            }
        }
    });
}

fn execute(
    command: Command,
    storage: &mut Storage,
    connections: &mut Connections,
    events: &async_channel::Sender<Event>,
    background: &super::sync_queue::SyncQueue,
) -> Result<()> {
    match command {
        Command::FlushNotifications => {
            for (account, folder, validity) in storage.notification_folders()? {
                let messages = storage.pending_notifications(&account, &folder, validity)?;
                if !messages.is_empty() {
                    events.send_blocking(Event::NewMail(account, folder, validity, messages))?;
                }
            }
        }
        Command::NotificationsHandled {
            account,
            folder,
            validity,
            uids,
        } => {
            storage.acknowledge_notifications(&account, &folder, validity, &uids)?;
        }
        Command::OpenNotification {
            account,
            validity,
            uid,
        } => {
            let result = (|| -> Result<Option<Message>> {
                if storage.validity(&account, "INBOX")? != Some(validity) {
                    return Ok(None);
                }
                storage.message(&account, "INBOX", uid)
            })()
            .map_err(|error| error.to_string());
            events.send_blocking(Event::NotificationOpened(account, result))?;
        }
        Command::CalendarReply {
            account,
            message,
            event,
            response,
        } => {
            let result = super::calendar::send_reply(&account, &message, &event, response)
                .inspect(|message| {
                    if let Err(error) = storage.store_sent(&account.email, message) {
                        eprintln!("Could not cache calendar response: {error}");
                    }
                })
                .map_err(|error| error.to_string());
            events.send_blocking(Event::CalendarReplied {
                account: account.email,
                uid: event.uid.clone().unwrap_or_default(),
                recurrence_id: event.recurrence_id.clone(),
                result,
            })?;
        }
        Command::Unsubscribe {
            account,
            folder,
            uid,
            message_id,
        } => {
            let result = super::unsubscribe::execute(
                &account,
                &folder,
                uid,
                &message_id,
                storage,
                connections,
            )
            .map_err(|error| error.to_string());
            events.send_blocking(Event::Unsubscribe {
                account: account.email,
                folder,
                uid,
                message_id,
                result,
            })?;
        }
        Command::SaveDraft {
            account,
            draft,
            target,
            request,
        } => {
            let result = super::draft_save::save(&account, &draft, target.as_ref(), storage)
                .map_err(|error| error.to_string());
            events.send_blocking(Event::DraftSaved(request, result))?;
        }
        Command::Compose {
            account,
            draft,
            target,
            request,
        } => {
            let result = super::drafts::prepare_send(&account, &draft, target.as_ref())
                .and_then(|draft| super::smtp::send(&account, &draft))
                .map(|message| {
                    if let Err(error) = storage.store_sent(&account.email, &message) {
                        eprintln!("Could not cache sent message: {error}");
                    }
                    let cleanup_error = target.as_ref().and_then(|target| {
                        super::drafts::delete(&account, target, storage)
                            .err()
                            .map(|error| error.to_string())
                    });
                    crate::models::draft::Outcome {
                        message,
                        cleanup_error,
                    }
                })
                .map_err(|error| error.to_string());
            let changed = result.is_ok();
            events.send_blocking(Event::Composed(request, result))?;
            if changed {
                events.send_blocking(Event::SentCacheChanged(account.email))?;
            }
        }
        Command::DeleteDraft {
            account,
            target,
            request,
        } => {
            let result = super::drafts::delete(&account, &target, storage)
                .map_err(|error| error.to_string());
            let deleted = result.is_ok();
            events.send_blocking(Event::DraftDeleted(request, result))?;
            if deleted {
                events.send_blocking(Event::SentCacheChanged(account.email))?;
            }
        }
        Command::SentBody {
            account,
            folder,
            uid,
            generation,
            selection,
        } => {
            let result = (|| {
                let validity = storage.validity(&account.email, &folder)?;
                let mut cached = storage
                    .message(&account.email, &folder, uid)?
                    .ok_or_else(|| anyhow::anyhow!("Outgoing message is no longer cached"))?;
                super::mail_sync::ensure_draft_id(&mut cached, &folder);
                if cached.body_loaded {
                    return Ok(cached);
                }
                let mut message = connections.execute(&account, |mail| {
                    mail.body_with_validity(&folder, uid, validity, true)
                })?;
                message.is_draft |= cached.is_draft;
                if message.is_draft && message.message_id.is_empty() {
                    message.message_id = cached.message_id.clone();
                }
                storage
                    .store_body(&account.email, &folder, validity, &message)?
                    .ok_or_else(|| anyhow::anyhow!("Outgoing mailbox changed while loading"))
            })()
            .map(|mut message| {
                message.prepare_display();
                message
            })
            .map_err(|error: anyhow::Error| error.to_string());
            events.send_blocking(Event::SentBody(generation, selection, folder, uid, result))?;
        }
        Command::UndoSpam {
            account,
            folder,
            originals,
        } => {
            let outcome =
                super::sender_actions::undo_spam(&account, &folder, &originals, storage, events)
                    .unwrap_or_else(|error| super::sender_actions::UndoOutcome {
                        messages: None,
                        restored: Vec::new(),
                        error: Some(error.to_string()),
                    });
            events.send_blocking(Event::SpamUndoFinished {
                account: account.email,
                folder,
                originals,
                restored: outcome.restored,
                messages: outcome.messages,
                error: outcome.error,
            })?;
            background.refresh();
        }
        Command::SenderAction {
            account,
            folder,
            sender,
            action,
        } => {
            let outcome =
                super::sender_actions::execute(&account, &folder, &sender, action, storage, events);
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => super::sender_actions::Outcome {
                    messages: None,
                    error: Some(error.to_string()),
                    removed: Vec::new(),
                },
            };
            events.send_blocking(Event::SenderActionFinished {
                account: account.email,
                folder,
                sender,
                messages: outcome.messages,
                error: outcome.error,
                removed: outcome.removed,
            })?;
            background.refresh();
        }
        Command::Discover => {
            background.restore_started(storage.last_sync_started()?);
            let accounts = accounts::discover()?;
            for account in &accounts {
                background.restore_folders(&account.email, storage.sync_folders(&account.email)?);
            }
            events.send_blocking(Event::Accounts(accounts.clone()))?;
            for account in accounts {
                events.send_blocking(Event::UnreadSnapshot(
                    account.email.clone(),
                    storage.unread(&account.email)?,
                ))?;
                let folders = storage.folders(&account.email)?;
                if !folders.is_empty() {
                    events.send_blocking(Event::Folders(account.email, folders))?;
                }
            }
            events.send_blocking(Event::SidebarReady)?;
        }
        Command::Sync => {
            if background.needs_start_time() {
                let started = std::time::SystemTime::now();
                storage.record_sync_started(started)?;
                background.restore_started(Some(started));
            }
            background.refresh();
        }
        Command::Folders(account) => {
            let cached = storage.folders(&account.email)?;
            if !cached.is_empty() {
                events.send_blocking(Event::Folders(account.email.clone(), cached))?;
            }
            let folders = connections.execute(&account, |mail| mail.folders())?;
            storage.store_folders(&account.email, &folders)?;
            events.send_blocking(Event::Folders(account.email, folders))?;
        }
        Command::Load {
            account,
            folder,
            generation,
        } => {
            events.send_blocking(Event::Messages(
                generation,
                storage.sender_headers(&account.email, &folder)?,
                true,
            ))?;
            let revision = storage.read_revision()?;
            let (validity, messages, uids) =
                connections.execute(&account, |mail| mail.headers(&folder))?;
            storage.reconcile_since(
                &account.email,
                &folder,
                validity,
                messages,
                &uids,
                revision,
            )?;
            events.send_blocking(Event::Messages(
                generation,
                storage.sender_headers(&account.email, &folder)?,
                false,
            ))?;
            let unread = connections.execute(&account, |mail| mail.has_unread(&folder))?;
            storage.store_unread(&account.email, &folder, unread)?;
            events.send_blocking(Event::Unread(account.email, vec![(folder, unread)]))?;
        }
        Command::LoadSenderPage { .. }
        | Command::RelatedSent { .. }
        | Command::ParcelHistory { .. }
        | Command::CalendarHistory { .. } => {
            unreachable!("cache reads use cache worker")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cached_draft_bodies_without_waiting_for_the_network_worker() {
        let path = std::env::temp_dir().join(format!(
            "brevlada-draft-cache-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let received = Message {
            uid: 1,
            message_id: "incoming".into(),
            sender: "other@example.com".into(),
            ..Default::default()
        };
        let draft = Message {
            uid: 42,
            uid_validity: Some(7),
            message_id: "draft".into(),
            references: vec!["incoming".into()],
            is_draft: true,
            body_loaded: true,
            body_text: "Cached body".into(),
            body_html: "<b>Cached body</b>".into(),
            cc: "cc@example.com".into(),
            ..Default::default()
        };
        {
            let storage = Storage::open(&path).unwrap();
            storage.store("me@example.com", "Drafts", &draft).unwrap();
        }
        let (commands, network_commands) = mpsc::channel();
        let (cache_commands, cache_receiver) = mpsc::channel();
        let (events, results) = async_channel::unbounded();
        start_cache_reads(path.clone(), cache_receiver, events);
        let worker = Worker {
            request: AtomicU64::default(),
            parcel_commands: commands.clone(),
            commands,
            cache_commands,
            background: super::super::sync_queue::SyncQueue::default(),
            bodies: super::super::body_queue::BodyQueue::default(),
            avatars: AvatarQueue::default(),
        };
        // The network queue has no consumer: a cache lookup must still finish.
        worker
            .send(Command::RelatedSent {
                account: Account {
                    email: "me@example.com".into(),
                    path: String::new(),
                    name: String::new(),
                    host: String::new(),
                    username: String::new(),
                    port: 993,
                    ssl: true,
                    tls: false,
                    oauth2: false,
                    smtp: None,
                },
                folder: "INBOX".into(),
                messages: vec![received],
                generation: 3,
                selection: 9,
            })
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            match results.try_recv() {
                Ok(event) => break event,
                Err(async_channel::TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(error) => panic!("Cache read did not complete: {error}"),
            }
        };
        let Event::RelatedSent(3, 9, related) = result else {
            panic!("Expected cached drafts")
        };
        assert_eq!(related.len(), 1);
        let mut draft = draft;
        draft.prepare_display();
        assert_eq!(related[0].message, draft);
        assert!(network_commands.try_recv().is_err());
        drop(worker);
        let _ = std::fs::remove_file(path);
    }
}
