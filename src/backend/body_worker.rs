use super::{
    body_queue::BodyQueue,
    connections::Connections,
    storage::Storage,
    worker::{BodyRequest, Event},
};
use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn start(path: PathBuf, queue: BodyQueue, events: async_channel::Sender<Event>) {
    std::thread::spawn(move || {
        let mut connections = Connections::default();
        while let Some(request) = queue.pop() {
            if !queue.current(request.selection) {
                continue;
            }
            if let Err(error) = load(&path, &request, &queue, &events, &mut connections)
                && queue.current(request.selection)
            {
                let event = if request.mark_read {
                    Event::BodyError(
                        request.generation,
                        request.selection,
                        request.uid,
                        error.to_string(),
                    )
                } else {
                    Event::PreviewError(request.generation, request.selection, request.uid)
                };
                let _ = events.send_blocking(event);
            }
        }
    });
}

fn load(
    path: &std::path::Path,
    request: &BodyRequest,
    queue: &BodyQueue,
    events: &async_channel::Sender<Event>,
    connections: &mut Connections,
) -> Result<()> {
    let mut storage = Storage::open(path)?;
    let validity = storage.validity(&request.account.email, &request.folder)?;
    let mut message = storage
        .message(&request.account.email, &request.folder, request.uid)?
        .context("Message is no longer in this folder")?;
    if !message.body_loaded {
        let fetched =
            connections.execute_body(&request.account, queue, request.selection, |mail| {
                mail.body_with_validity(&request.folder, request.uid, validity)
            })?;
        anyhow::ensure!(
            super::parser::normalize_message_id(&message.message_id).is_empty()
                || super::parser::normalize_message_id(&message.message_id)
                    == super::parser::normalize_message_id(&fetched.message_id),
            "The mailbox changed. Reload the folder."
        );
        message = storage
            .store_body(&request.account.email, &request.folder, validity, &fetched)?
            .context("Mailbox changed while loading message")?;
    }
    if !queue.current(request.selection) {
        return Ok(());
    }
    if !request.mark_read {
        events.send_blocking(Event::Preview(
            request.generation,
            request.selection,
            message,
        ))?;
        return Ok(());
    }
    events.send_blocking(Event::Body(
        request.generation,
        request.selection,
        message.clone(),
    ))?;
    if !message.is_read {
        connections.execute_body(&request.account, queue, request.selection, |mail| {
            mail.mark_read(&request.folder, request.uid, validity)
        })?;
        message = storage
            .mark_read_cached(
                &request.account.email,
                &request.folder,
                validity,
                request.uid,
            )?
            .context("Mailbox changed while marking message read")?;
        if queue.current(request.selection) {
            events.send_blocking(Event::Body(request.generation, request.selection, message))?;
        }
        let unread =
            connections.execute_body(&request.account, queue, request.selection, |mail| {
                mail.has_unread(&request.folder)
            })?;
        storage.store_unread(&request.account.email, &request.folder, unread)?;
        events.send_blocking(Event::Unread(
            request.account.email.clone(),
            vec![(request.folder.clone(), unread)],
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, Message};

    #[test]
    fn reads_latest_cached_body_and_ignores_cancelled_selection() {
        let path = std::env::temp_dir().join(format!(
            "brevlada-body-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = Storage::open(&path).unwrap();
        let request = BodyRequest {
            account: Account {
                path: String::new(),
                email: "a".into(),
                name: String::new(),
                host: String::new(),
                username: String::new(),
                port: 993,
                ssl: true,
                tls: false,
                oauth2: true,
            },
            folder: "INBOX".into(),
            uid: 1,
            generation: 1,
            selection: 1,
            mark_read: true,
        };
        storage
            .store(
                "a",
                "INBOX",
                &Message {
                    uid: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        storage
            .store(
                "a",
                "INBOX",
                &Message {
                    uid: 1,
                    body_loaded: true,
                    is_read: true,
                    body_text: "Already fetched".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let queue = BodyQueue::default();
        let (events, received) = async_channel::unbounded();
        queue.select(1);
        load(
            &path,
            &request,
            &queue,
            &events,
            &mut Connections::default(),
        )
        .unwrap();
        match received.try_recv().unwrap() {
            Event::Body(1, 1, message) => assert_eq!(message.body_text, "Already fetched"),
            _ => panic!("Expected cached body"),
        }
        let mut unread = storage.message("a", "INBOX", 1).unwrap().unwrap();
        unread.is_read = false;
        storage.store("a", "INBOX", &unread).unwrap();
        let preview = BodyRequest {
            mark_read: false,
            ..request.clone()
        };
        load(
            &path,
            &preview,
            &queue,
            &events,
            &mut Connections::default(),
        )
        .unwrap();
        match received.try_recv().unwrap() {
            Event::Preview(1, 1, message) => {
                assert_eq!(message.body_text, "Already fetched");
                assert!(!message.is_read);
            }
            _ => panic!("Expected an unread preview"),
        }
        assert!(!storage.message("a", "INBOX", 1).unwrap().unwrap().is_read);
        assert!(received.try_recv().is_err());
        queue.select(2);
        load(
            &path,
            &request,
            &queue,
            &events,
            &mut Connections::default(),
        )
        .unwrap();
        assert!(received.try_recv().is_err());
        drop(storage);
        std::fs::remove_file(path).unwrap();
    }
}
