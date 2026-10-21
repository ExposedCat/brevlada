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
                let event = if request.related {
                    Event::SentBody(
                        request.generation,
                        request.selection,
                        request.folder.clone(),
                        request.uid,
                        Err(error.to_string()),
                    )
                } else if request.mark_read {
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
            queue.complete(request.selection);
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
    super::mail_sync::ensure_draft_id(&mut message, &request.folder);
    if !message.body_loaded
        || (request.load_inline
            && message.body_html.contains("cid:")
            && !message.inline_media_loaded)
    {
        let mut fetched = match storage.cached_body_copy(
            &request.account.email,
            &message,
            request.load_inline,
        )? {
            Some(cached) => cached,
            None => connections.execute_body_priority(
                &request.account,
                queue,
                request.selection,
                request.opened(),
                |mail| mail.body_with_validity(&request.folder, request.uid, validity, true),
            )?,
        };
        fetched.is_draft |= message.is_draft;
        if fetched.is_draft && fetched.message_id.is_empty() {
            fetched.message_id = message.message_id.clone();
        }
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
    message.prepare_display();
    if !request.mark_read && !request.related {
        events.send_blocking(Event::Preview(
            request.generation,
            request.selection,
            message,
        ))?;
        return Ok(());
    }
    events.send_blocking(body_event(request, message.clone()))?;
    if !request.mark_read {
        return Ok(());
    }
    if !message.is_read {
        connections.execute_body_priority(
            &request.account,
            queue,
            request.selection,
            request.opened(),
            |mail| mail.mark_read(&request.folder, request.uid, validity),
        )?;
        message = storage
            .mark_read_cached(
                &request.account.email,
                &request.folder,
                validity,
                request.uid,
            )?
            .context("Mailbox changed while marking message read")?;
        message.prepare_display();
        if queue.current(request.selection) {
            events.send_blocking(body_event(request, message))?;
        }
        let unread = connections.execute_body_priority(
            &request.account,
            queue,
            request.selection,
            request.opened(),
            |mail| mail.has_unread(&request.folder),
        )?;
        storage.store_unread(&request.account.email, &request.folder, unread)?;
        events.send_blocking(Event::Unread(
            request.account.email.clone(),
            vec![(request.folder.clone(), unread)],
        ))?;
    }
    Ok(())
}

fn body_event(request: &BodyRequest, message: crate::models::Message) -> Event {
    if request.related {
        Event::SentBody(
            request.generation,
            request.selection,
            request.folder.clone(),
            request.uid,
            Ok(message),
        )
    } else {
        Event::Body(request.generation, request.selection, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, Message};

    #[test]
    fn opens_other_folder_and_local_sent_copies_without_network_and_persists_them() {
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
                smtp: None,
            },
            folder: "INBOX".into(),
            uid: 1,
            generation: 1,
            selection: 1,
            mark_read: true,
            load_inline: false,
            related: false,
        };
        storage
            .store(
                "a",
                "INBOX",
                &Message {
                    uid: 1,
                    message_id: "cached@example.com".into(),
                    is_read: true,
                    is_flagged: true,
                    ..Default::default()
                },
            )
            .unwrap();
        storage
            .store(
                "a",
                "All Mail",
                &Message {
                    uid: 999,
                    message_id: "<cached@example.com>".into(),
                    body_loaded: true,
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
            Event::Body(1, 1, message) => {
                assert_eq!(message.body_text, "Already fetched");
                assert_eq!(message.uid, 1);
                assert!(message.is_read && message.is_flagged);
            }
            _ => panic!("Expected cached body"),
        }
        let mut unread = storage.message("a", "INBOX", 1).unwrap().unwrap();
        unread.is_read = false;
        storage.store("a", "INBOX", &unread).unwrap();
        let preview = BodyRequest {
            mark_read: false,
            related: false,
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

        // A freshly sent message is reusable even before folder sync downloads it.
        storage
            .store_sent(
                "a",
                &Message {
                    message_id: "local@example.com".into(),
                    body_loaded: true,
                    body_text: "Local sent body".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        storage
            .store(
                "a",
                "Sent",
                &Message {
                    uid: 7,
                    message_id: "local@example.com".into(),
                    is_read: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let related = BodyRequest {
            folder: "Sent".into(),
            uid: 7,
            selection: 2,
            related: true,
            ..request
        };
        load(
            &path,
            &related,
            &queue,
            &events,
            &mut Connections::default(),
        )
        .unwrap();
        match received.try_recv().unwrap() {
            Event::SentBody(1, 2, folder, 7, Ok(message)) => {
                assert_eq!(folder, "Sent");
                assert_eq!(message.body_text, "Local sent body");
                assert!(message.is_read);
            }
            _ => panic!("Expected a cached related body"),
        }
        drop(storage);
        let storage = Storage::open(&path).unwrap();
        let inbox = storage.message("a", "INBOX", 1).unwrap().unwrap();
        assert_eq!(inbox.body_text, "Already fetched");
        assert!(inbox.body_loaded && inbox.is_flagged && !inbox.is_read);
        let sent = storage.message("a", "Sent", 7).unwrap().unwrap();
        assert_eq!(sent.body_text, "Local sent body");
        assert!(sent.body_loaded && sent.is_read);
        drop(storage);
        std::fs::remove_file(path).unwrap();
    }
}
