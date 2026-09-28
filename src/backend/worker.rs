use super::{accounts, avatar_queue::AvatarQueue, connections::Connections, storage::Storage};
use crate::models::{Account, Draft, Message, SentMessage};
use anyhow::Result;
use std::{collections::HashSet, path::PathBuf, sync::mpsc};

pub enum Command {
    Send {
        account: Account,
        draft: Draft,
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
}

pub enum Event {
    Sent(Result<(), String>),
    SentCacheChanged(String),
    RelatedSent(u64, u64, Vec<SentMessage>),
    SentBody(u64, u64, String, u32, Result<Message, String>),
    SenderActionFinished {
        account: String,
        folder: String,
        sender: crate::models::action_target::ActionTarget,
        messages: Option<Vec<Message>>,
        error: Option<String>,
    },
    Accounts(Vec<Account>),
    SidebarReady,
    Folders(String, Vec<String>),
    Unread(String, Vec<(String, bool)>),
    UnreadSnapshot(String, Vec<(String, bool)>),
    Messages(u64, Vec<Message>, bool),
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
    commands: mpsc::Sender<Command>,
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
                background: super::sync_queue::SyncQueue::default(),
                commands,
                bodies: super::body_queue::BodyQueue::default(),
                avatars: AvatarQueue::default(),
            },
            receiver,
        )
    }

    pub fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| anyhow::anyhow!("Mail service stopped"))
    }
    pub fn register(&self, account: Account, expanded: bool) {
        self.avatars.register(account.clone());
        self.background.register(account, expanded);
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
        self.bodies.close();
        self.background.close();
        self.avatars.close();
    }
}

pub fn start(path: PathBuf) -> (Worker, async_channel::Receiver<Event>) {
    let (sender, receiver) = mpsc::channel();
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
    start_commands(path, receiver, events, background.clone());
    (
        Worker {
            commands: sender,
            background,
            bodies,
            avatars,
        },
        results,
    )
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
        let mut sent_loaded = HashSet::new();
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
                &mut sent_loaded,
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
    sent_loaded: &mut HashSet<String>,
) -> Result<()> {
    match command {
        Command::Send { account, draft } => {
            let result = super::smtp::send(&account, &draft).map_err(|error| error.to_string());
            match result {
                Ok(message) => {
                    if let Err(error) = storage.store_sent(&account.email, &message) {
                        eprintln!("Could not cache sent message: {error}");
                    }
                    events.send_blocking(Event::Sent(Ok(())))?;
                    events.send_blocking(Event::SentCacheChanged(account.email))?;
                    background.refresh();
                }
                Err(error) => events.send_blocking(Event::Sent(Err(error)))?,
            }
        }
        Command::RelatedSent {
            account,
            folder,
            messages,
            generation,
            selection,
        } => {
            if !sent_loaded.contains(&account.email) {
                let fresh = connections.execute(&account, |mail| {
                    let (_, _, sent) = mail.sync_folders()?;
                    sent.map(|folder| {
                        mail.headers(&folder)
                            .map(|(validity, messages, uids)| (folder, validity, messages, uids))
                    })
                    .transpose()
                });
                match fresh {
                    Ok(Some((sent, validity, headers, uids))) => {
                        match storage.reconcile(&account.email, &sent, validity, headers, &uids) {
                            Ok(()) => {
                                sent_loaded.insert(account.email.clone());
                            }
                            Err(error) => eprintln!("Could not cache sent headers: {error}"),
                        }
                    }
                    Ok(None) => {
                        sent_loaded.insert(account.email.clone());
                    }
                    Err(error) => eprintln!("Could not load sent headers: {error}"),
                }
            }
            let related = match storage.related_sent(&account.email, &folder, &messages) {
                Ok(related) => related,
                Err(error) => {
                    events.send_blocking(Event::Error(
                        None,
                        format!("Could not load sent messages: {error}"),
                    ))?;
                    Vec::new()
                }
            };
            events.send_blocking(Event::RelatedSent(generation, selection, related))?;
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
                let cached = storage
                    .message(&account.email, &folder, uid)?
                    .ok_or_else(|| anyhow::anyhow!("Sent message is no longer cached"))?;
                if cached.body_loaded {
                    return Ok(cached);
                }
                let message = connections.execute(&account, |mail| {
                    mail.body_with_validity(&folder, uid, validity)
                })?;
                storage
                    .store_body(&account.email, &folder, validity, &message)?
                    .ok_or_else(|| anyhow::anyhow!("Sent mailbox changed while loading"))
            })()
            .map_err(|error: anyhow::Error| error.to_string());
            events.send_blocking(Event::SentBody(generation, selection, folder, uid, result))?;
        }
        Command::SenderAction {
            account,
            folder,
            sender,
            action,
        } => {
            let outcome =
                super::sender_actions::execute(&account, &folder, &sender, action, storage, events);
            let (messages, error) = match outcome {
                Ok(outcome) => outcome,
                Err(error) => (None, Some(error.to_string())),
            };
            events.send_blocking(Event::SenderActionFinished {
                account: account.email,
                folder,
                sender,
                messages,
                error,
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
                storage.messages(&account.email, &folder)?,
                true,
            ))?;
            let (validity, messages, uids) =
                connections.execute(&account, |mail| mail.headers(&folder))?;
            storage.reconcile(&account.email, &folder, validity, messages, &uids)?;
            events.send_blocking(Event::Messages(
                generation,
                storage.messages(&account.email, &folder)?,
                false,
            ))?;
            let unread = connections.execute(&account, |mail| mail.has_unread(&folder))?;
            storage.store_unread(&account.email, &folder, unread)?;
            events.send_blocking(Event::Unread(account.email, vec![(folder, unread)]))?;
        }
    }
    Ok(())
}
