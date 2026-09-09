use super::{accounts, connections::Connections, storage::Storage};
use crate::models::{Account, Message};
use anyhow::Result;
use std::{path::PathBuf, sync::mpsc};

pub enum Command {
    Discover,
    Folders(Account),
    SyncInbox(Account),
    Load {
        account: Account,
        folder: String,
        generation: u64,
    },
}

pub enum Event {
    Accounts(Vec<Account>),
    Folders(String, Vec<String>),
    Messages(u64, Vec<Message>, bool),
    Body(u64, u64, Message),
    BodyError(u64, u64, u32, String),
    Error(Option<u64>, String),
}

#[derive(Clone)]
pub struct BodyRequest {
    pub account: Account,
    pub folder: String,
    pub uid: u32,
    pub generation: u64,
    pub selection: u64,
}

pub struct Worker {
    commands: mpsc::Sender<Command>,
    bodies: super::body_queue::BodyQueue,
}

impl Worker {
    #[cfg(test)]
    pub fn disconnected() -> Self {
        let (commands, _) = mpsc::channel();
        Self {
            commands,
            bodies: super::body_queue::BodyQueue::default(),
        }
    }

    pub fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| anyhow::anyhow!("Mail service stopped"))
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
    }
}

pub fn start(path: PathBuf) -> (Worker, async_channel::Receiver<Event>) {
    let (sender, receiver) = mpsc::channel();
    let (events, results) = async_channel::unbounded();
    let bodies = super::body_queue::BodyQueue::default();
    for _ in 0..2 {
        super::body_worker::start(path.clone(), bodies.clone(), events.clone());
    }
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
            if let Err(error) = execute(command, &mut storage, &mut connections, &events)
                && events
                    .send_blocking(Event::Error(generation, error.to_string()))
                    .is_err()
            {
                break;
            }
        }
    });
    (
        Worker {
            commands: sender,
            bodies,
        },
        results,
    )
}

fn execute(
    command: Command,
    storage: &mut Storage,
    connections: &mut Connections,
    events: &async_channel::Sender<Event>,
) -> Result<()> {
    match command {
        Command::Discover => {
            events.send_blocking(Event::Accounts(accounts::discover()?))?;
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
        Command::SyncInbox(account) => {
            let (validity, messages, uids) =
                connections.execute(&account, |mail| mail.headers("INBOX"))?;
            storage.reconcile(&account.email, "INBOX", validity, messages, &uids)?;
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
        }
    }
    Ok(())
}
