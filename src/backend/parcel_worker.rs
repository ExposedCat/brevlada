use super::{
    body_queue::BodyQueue,
    connections::Connections,
    storage::Storage,
    worker::{Command, Event},
};
use crate::models::{Message, parcel};
use anyhow::Result;
use std::{path::PathBuf, sync::mpsc};

pub fn start(
    path: PathBuf,
    commands: mpsc::Receiver<Command>,
    queue: BodyQueue,
    events: async_channel::Sender<Event>,
) {
    std::thread::spawn(move || {
        let mut connections = Connections::default();
        while let Ok(Command::ParcelHistory {
            account,
            folder,
            sender,
            identifier,
            generation,
            selection,
        }) = commands.recv()
        {
            if identifier.value().trim().is_empty()
                || sender.is_empty()
                || !queue.current(selection)
            {
                continue;
            }
            let result = (|| -> Result<()> {
                let mut storage = Storage::open(&path)?;
                let mut messages = storage.parcel_sender_messages(
                    &account.email,
                    &folder,
                    &sender,
                    identifier.value(),
                )?;
                let send = |messages: &[Message]| -> Result<()> {
                    if queue.current(selection) {
                        events.send_blocking(Event::ParcelHistory(
                            generation,
                            selection,
                            sender.clone(),
                            identifier.clone(),
                            matching(messages, &sender, &identifier),
                        ))?;
                    }
                    Ok(())
                };
                send(&messages)?;
                // Search even with complete cached bodies: older headers or a new
                // delivery email may not have reached the local cache yet.
                let validity = storage.validity(&account.email, &folder)?;
                let uids = connections.execute_body(&account, &queue, selection, |mail| {
                    mail.delivery_uids(&folder, validity, &sender, identifier.value())
                })?;
                for uid in uids {
                    if !queue.current(selection) {
                        return Ok(());
                    }
                    if messages
                        .iter()
                        .any(|message| message.uid == uid && message.body_loaded)
                    {
                        continue;
                    }
                    let fetched =
                        connections.execute_body(&account, &queue, selection, |mail| {
                            mail.body_with_validity(&folder, uid, validity, false)
                        })?;
                    let cached = storage.message(&account.email, &folder, uid)?;
                    let loaded = storage.store_body(&account.email, &folder, validity, &fetched)?;
                    // A vanished or replaced cached message must not contribute stale updates.
                    if let Some(message) = loaded.or_else(|| cached.is_none().then_some(fetched)) {
                        messages.retain(|old| old.uid != uid);
                        messages.push(message);
                        send(&messages)?;
                    }
                }
                Ok(())
            })();
            if let Err(error) = result
                && queue.current(selection)
            {
                eprintln!("Could not load delivery history: {error}");
            }
        }
    });
}

fn matching(messages: &[Message], sender: &str, id: &parcel::Identifier) -> Vec<Message> {
    messages
        .iter()
        .filter_map(|message| {
            if crate::models::senders::key(message) != sender {
                return None;
            }
            let mut message = message.clone();
            if message.parcels.is_empty() {
                message.parcels = parcel::parse_message(&message);
            }
            message
                .parcels
                .retain(|parcel| parcel::identifier(parcel).as_ref() == Some(id));
            (!message.parcels.is_empty()).then_some(message)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_tracking_updates_without_an_order_number() {
        let message = |subject: &str, tracking: &str| Message {
            sender: "service.eu@ugreen.com".into(),
            subject: subject.into(),
            body_html: format!("<p>Tracking number: {tracking}</p>"),
            body_loaded: true,
            ..Default::default()
        };
        let matches = matching(
            &[
                message("Your order hub has already been shipped.", "CC996635912DE"),
                message("Your order hub has been delivered.", "CC996635912DE"),
                message("Your order hub has been delivered.", "CC996635912DE1"),
            ],
            "service.eu@ugreen.com",
            &parcel::Identifier::Tracking("CC996635912DE".into()),
        );
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .all(|message| message.parcels[0].order_number.is_none())
        );
    }

    #[test]
    fn filters_server_substring_matches_to_exact_sender_and_order() {
        let message = |sender: &str, order: &str| Message {
            sender: sender.into(),
            body_html: format!(
                "<script type='application/ld+json'>{{\"@type\":\"ParcelDelivery\",\"partOfOrder\":{{\"orderNumber\":\"{order}\"}},\"deliveryStatus\":\"Sent\"}}</script>"
            ),
            body_loaded: true,
            ..Default::default()
        };
        let matches = matching(
            &[
                message("shop@example.com", "123"),
                message("shop@example.com", "1234"),
                message("other-shop@example.com", "123"),
            ],
            "shop@example.com",
            &parcel::Identifier::Order("123".into()),
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].parcels[0].order_number.as_deref(), Some("123"));
    }
}
