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
    AccountSettings {
        account: Account,
        path: PathBuf,
        reply: async_channel::Sender<Result<crate::models::settings::AccountFolders, String>>,
    },
    SetAccountArchive {
        account: Account,
        path: PathBuf,
        folder: Option<String>,
        share: bool,
        reply: async_channel::Sender<Result<crate::models::settings::AccountFolders, String>>,
    },
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
    NotificationAction {
        account: String,
        validity: u32,
        uid: u32,
        action: crate::models::sender_action::SenderAction,
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
        from: Account,
        draft: Draft,
        target: Option<crate::models::draft::Target>,
        request: u64,
    },
    Compose {
        account: Account,
        source_account: Account,
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
        outgoing: bool,
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
        mark_read: bool,
        load_inline: bool,
    },
    UndoAction(crate::models::undo::Action),
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
    SentFolder {
        account: Account,
        generation: u64,
    },
    Load {
        account: Account,
        folder: String,
        generation: u64,
    },
    LoadSenderPage {
        account: Account,
        folder: String,
        sender: String,
        outgoing: bool,
        own: std::collections::HashSet<String>,
        generation: u64,
        sender_generation: u64,
        before: Option<(i64, u32)>,
        limit: usize,
    },
}

pub enum Event {
    NewMail {
        account: String,
        folder: String,
        validity: u32,
        messages: Vec<Message>,
        icons: std::collections::HashMap<String, Vec<u8>>,
    },
    NotificationOpened(String, Result<Option<Message>, String>),
    NotificationActionStarted {
        account: String,
        validity: u32,
        uid: u32,
        action: crate::models::sender_action::SenderAction,
    },
    NotificationActionFinished(String, u32, u32, Option<String>),
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
    ConversationHistory(u64, u64, Vec<SentMessage>),
    RelatedSentError(u64, u64, String),
    SentBody(u64, u64, String, u32, Result<Message, String>),
    UndoActionFinished(crate::models::undo::Action),
    ActionUndoReady {
        account: Account,
        folder: String,
        sender: crate::models::action_target::ActionTarget,
        destination: Option<String>,
        removed: Vec<Message>,
    },
    MessagesRestored {
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
    SentFolder(u64, Account, String),
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
    CacheChanged(String, String),
    CachePruned(String, String, Vec<Message>),
    CacheBody(String, String, Message),
    Avatar(String, Option<Vec<u8>>),
    AvatarError(String),
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
    pub load_inline: bool,
    pub related: bool,
}

impl BodyRequest {
    pub fn opened(&self) -> bool {
        self.mark_read || self.related
    }
}

pub struct Worker {
    #[cfg(test)]
    record_commands: bool,
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
                record_commands: true,
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
        #[cfg(test)]
        if self.record_commands && matches!(command, Command::SentBody { .. }) {
            return self
                .commands
                .send(command)
                .map_err(|_| anyhow::anyhow!("Mail service stopped"));
        }
        if let Command::SentBody {
            account,
            folder,
            uid,
            generation,
            selection,
            mark_read,
            load_inline,
        } = command
        {
            self.bodies.push(BodyRequest {
                account,
                folder,
                uid,
                generation,
                selection,
                mark_read,
                load_inline,
                related: true,
            });
            return Ok(());
        }
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
    pub fn sent_folder(&self, account: &str) -> Option<String> {
        self.background.sent_folder(account)
    }
    pub fn sent_folders(&self) -> std::collections::HashMap<String, String> {
        self.background.sent_folders()
    }
    pub fn select(&self, selection: u64) {
        self.bodies.select(selection);
    }
    pub fn body(&self, request: BodyRequest) {
        self.bodies.push(request);
    }
    pub fn order_bodies(&self, order: &[(String, u32)]) {
        self.bodies.order(order);
    }
    pub fn conversation_pending(&self, selection: u64, pending: bool) {
        self.bodies.conversation(selection, pending);
    }
    pub fn body_handled(&self, selection: u64, folder: &str, uid: u32, related: bool) {
        self.bodies.handled(selection, folder, uid, related);
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
    let background = super::sync_queue::SyncQueue::default();
    let bodies = super::body_queue::BodyQueue::with_background(background.clone());
    super::body_worker::start(path.clone(), bodies.clone(), events.clone());
    let avatars = AvatarQueue::default();
    super::avatar_worker::start(path.clone(), avatars.clone(), events.clone());
    super::sync_worker::start(path.clone(), background.clone(), events.clone());
    start_commands(path.clone(), receiver, events.clone(), background.clone());
    super::parcel_worker::start(
        path.clone(),
        parcel_receiver,
        bodies.clone(),
        events.clone(),
    );
    let (history_sender, history_receiver) = mpsc::channel();
    super::conversation_worker::start(
        path.clone(),
        history_receiver,
        bodies.clone(),
        events.clone(),
    );
    start_cache_reads(path, cache_receiver, events, history_sender);
    (
        Worker {
            #[cfg(test)]
            record_commands: false,
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
    history: mpsc::Sender<Command>,
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
                        outgoing,
                        own,
                        generation,
                        sender_generation,
                        before,
                        limit,
                    } => {
                        let mut messages = storage.correspondent_page(
                            &account.email,
                            folder,
                            sender,
                            *before,
                            *limit + 1,
                            &crate::models::senders::Correspondence {
                                outgoing: *outgoing,
                                own: own.clone(),
                            },
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
                        outgoing,
                        messages,
                        generation,
                        selection,
                    } => Event::RelatedSent(
                        *generation,
                        *selection,
                        storage
                            .related_messages(&account.email, folder, messages, *outgoing)?
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
                Err(error) => match &command {
                    Command::LoadSenderPage {
                        generation,
                        sender_generation,
                        ..
                    } => Event::SenderPageError(*generation, *sender_generation, error.to_string()),
                    Command::RelatedSent {
                        generation,
                        selection,
                        ..
                    } => Event::RelatedSentError(*generation, *selection, error.to_string()),
                    _ => continue,
                },
            };
            if events.send_blocking(event).is_err() {
                break;
            }
            if matches!(command, Command::RelatedSent { .. }) {
                let _ = history.send(command);
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
                Command::Load { generation, .. } | Command::SentFolder { generation, .. } => {
                    Some(*generation)
                }
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
        Command::AccountSettings {
            account,
            path,
            reply,
        } => {
            let result = super::account_settings::load(&account, &path)
                .map_err(|error| format!("{error:#}"));
            let _ = reply.send_blocking(result);
        }
        Command::SetAccountArchive {
            account,
            path,
            folder,
            share,
            reply,
        } => {
            let result = super::account_settings::save(&account, &path, folder, share)
                .map_err(|error| format!("{error:#}"));
            let _ = reply.send_blocking(result);
        }
        Command::FlushNotifications => {
            for (account, folder, validity) in storage.notification_folders()? {
                let messages = storage.pending_notifications(&account, &folder, validity)?;
                if !messages.is_empty() {
                    let icons = storage.notification_icons(&messages);
                    events.send_blocking(Event::NewMail {
                        account,
                        folder,
                        validity,
                        messages,
                        icons,
                    })?;
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
                Ok(storage.message(&account, "INBOX", uid)?.map(|mut message| {
                    message.prepare_display();
                    message
                }))
            })()
            .map_err(|error| error.to_string());
            events.send_blocking(Event::NotificationOpened(account, result))?;
        }
        Command::NotificationAction {
            account,
            validity,
            uid,
            action,
        } => {
            let result = (|| -> Result<_> {
                anyhow::ensure!(
                    storage.validity(&account, "INBOX")? == Some(validity),
                    "This notification is no longer current; refresh your inbox"
                );
                let message = storage
                    .message(&account, "INBOX", uid)?
                    .ok_or_else(|| anyhow::anyhow!("This message is no longer available"))?;
                let sender = crate::models::action_target::ActionTarget::Messages(vec![(
                    uid,
                    message.message_id,
                )]);
                let account = accounts::discover()?
                    .into_iter()
                    .find(|item| item.email == account)
                    .ok_or_else(|| anyhow::anyhow!("This account is no longer available"))?;
                let outcome = super::sender_actions::execute(
                    &account,
                    "INBOX",
                    &sender,
                    action,
                    storage,
                    events,
                    Some(validity),
                )?;
                outcome.refresh_destination(&account, background);
                Ok((sender, outcome))
            })();
            let error = match result {
                Ok((sender, outcome)) => {
                    let error = outcome.error.clone();
                    events.send_blocking(Event::SenderActionFinished {
                        account: account.clone(),
                        folder: "INBOX".into(),
                        sender,
                        messages: outcome.messages,
                        error: outcome.error,
                        removed: outcome.removed,
                    })?;
                    background.refresh();
                    error
                }
                Err(error) => Some(error.to_string()),
            };
            events.send_blocking(Event::NotificationActionFinished(
                account, validity, uid, error,
            ))?;
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
            from,
            draft,
            target,
            request,
        } => {
            let result = super::draft_save::save(&account, &from, &draft, target.as_ref(), storage)
                .map_err(|error| error.to_string());
            events.send_blocking(Event::DraftSaved(request, result))?;
        }
        Command::Compose {
            account,
            source_account,
            draft,
            target,
            request,
        } => {
            let result = super::drafts::prepare_send(&source_account, &draft, target.as_ref())
                .and_then(|draft| super::smtp::send(&account, &draft))
                .map(|message| {
                    if let Err(error) = storage.store_sent(&account.email, &message) {
                        eprintln!("Could not cache sent message: {error}");
                    }
                    let cleanup_error = target.as_ref().and_then(|target| {
                        super::drafts::delete(&source_account, target, storage)
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
        Command::SentBody { .. } => unreachable!("opened conversation bodies use the body queue"),
        Command::UndoAction(mut action) => {
            for batch in &mut action.batches {
                let outcome = super::sender_actions::undo_action(batch, storage, events)
                    .unwrap_or_else(|error| super::sender_actions::UndoOutcome {
                        messages: None,
                        restored: Vec::new(),
                        error: Some(error.to_string()),
                    });
                events.send_blocking(Event::MessagesRestored {
                    account: batch.account.email.clone(),
                    folder: batch.folder.clone(),
                    originals: batch.messages.clone(),
                    restored: outcome.restored.clone(),
                    messages: outcome.messages,
                    error: outcome.error,
                })?;
                batch
                    .messages
                    .retain(|message| !outcome.restored.contains(message));
                background.refresh_folder(&batch.account, &batch.destination);
                background.refresh_folder(&batch.account, &batch.folder);
            }
            action.batches.retain(|batch| !batch.messages.is_empty());
            events.send_blocking(Event::UndoActionFinished(action))?;
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
            events.send_blocking(Event::MessagesRestored {
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
            let outcome = super::sender_actions::execute(
                &account,
                &folder,
                &sender,
                action.clone(),
                storage,
                events,
                None,
            );
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => super::sender_actions::Outcome {
                    messages: None,
                    error: Some(error.to_string()),
                    removed: Vec::new(),
                    destination: None,
                },
            };
            outcome.refresh_destination(&account, background);
            let undo = action.undoable_move().then(|| Event::ActionUndoReady {
                account: account.clone(),
                folder: folder.clone(),
                sender: sender.clone(),
                destination: outcome.destination.clone(),
                removed: outcome.removed.clone(),
            });
            events.send_blocking(Event::SenderActionFinished {
                account: account.email,
                folder,
                sender,
                messages: outcome.messages,
                error: outcome.error,
                removed: outcome.removed,
            })?;
            if let Some(undo) = undo {
                events.send_blocking(undo)?;
            }
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
        Command::SentFolder {
            account,
            generation,
        } => {
            let folder = if let Some(folder) = background.sent_folder(&account.email) {
                folder
            } else {
                let discovered = connections.execute(&account, |mail| mail.sync_folders())?;
                storage.store_folders(&account.email, &discovered.all)?;
                discovered.sent.ok_or_else(|| {
                    anyhow::anyhow!("No sent mail folder found for {}", account.email)
                })?
            };
            background.set_sent_folder(&account.email, &folder);
            events.send_blocking(Event::SentFolder(generation, account, folder))?;
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
    fn stale_or_missing_notification_targets_fail_before_accessing_accounts_or_mail() {
        use crate::models::sender_action::SenderAction;

        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage.inventory("account", "INBOX", 8, &[]).unwrap();
        storage
            .store_headers(
                "account",
                "INBOX",
                8,
                &[Message {
                    uid: 1,
                    uid_validity: Some(8),
                    message_id: "new-message".into(),
                    ..Message::default()
                }],
            )
            .unwrap();
        let (events, results) = async_channel::unbounded();
        for action in [
            SenderAction::MarkRead,
            SenderAction::Archive,
            SenderAction::Delete,
        ] {
            for (validity, uid, expected_error) in
                [(7, 1, "no longer current"), (8, 2, "no longer available")]
            {
                execute(
                    Command::NotificationAction {
                        account: "account".into(),
                        validity,
                        uid,
                        action: action.clone(),
                    },
                    &mut storage,
                    &mut Connections::default(),
                    &events,
                    &super::super::sync_queue::SyncQueue::default(),
                )
                .unwrap();
                let Event::NotificationActionFinished(
                    account,
                    actual_validity,
                    actual_uid,
                    Some(error),
                ) = results.try_recv().unwrap()
                else {
                    panic!("Expected a failed notification action")
                };
                assert_eq!(
                    (account.as_str(), actual_validity, actual_uid),
                    ("account", validity, uid)
                );
                assert!(error.contains(expected_error), "{error}");
                assert!(results.try_recv().is_err());
                assert!(
                    !storage
                        .message("account", "INBOX", 1)
                        .unwrap()
                        .unwrap()
                        .is_read
                );
            }
        }
    }

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
        let (history_sender, _history_receiver) = mpsc::channel();
        start_cache_reads(path.clone(), cache_receiver, events, history_sender);
        let worker = Worker {
            record_commands: true,
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
                outgoing: false,
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
