use crate::models::{
    Account, Draft, Message,
    calendar::{self, Event, Response},
};
use anyhow::{Context, Result};

pub fn send_reply(
    account: &Account,
    original: &Message,
    event: &Event,
    response: Response,
) -> Result<Message> {
    super::smtp::send(account, &draft_reply(account, original, event, response)?)
}

fn draft_reply(
    account: &Account,
    original: &Message,
    event: &Event,
    response: Response,
) -> Result<Draft> {
    let calendar = calendar::reply(event, &account.email, response)?;
    let part = super::attachments::encode("response.ics", "text/calendar", calendar.as_bytes())?
        .replacen(
            "Content-Type: text/calendar",
            "Content-Type: text/calendar; method=REPLY; charset=UTF-8",
            1,
        );
    let mut references = original.references.clone();
    let in_reply_to = (!original.message_id.is_empty()).then(|| original.message_id.clone());
    if let Some(id) = &in_reply_to
        && !references.contains(id)
    {
        references.push(id.clone());
    }
    let draft = Draft {
        attachments_loaded: true,
        to: event
            .organizer_email
            .clone()
            .context("Organizer email is unavailable")?,
        cc: String::new(),
        subject: format!("Re: {}", original.subject),
        text: format!(
            "{}: {}\n\n{} responded {} to this invitation.",
            response.label(),
            event.name,
            account.email,
            response.label().to_lowercase()
        ),
        html: None,
        attachments: vec![part],
        attachment_names: vec!["response.ics".into()],
        removed_attachments: Vec::new(),
        attachment_source: None,
        in_reply_to,
        references,
    };
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_draft_addresses_the_organizer_and_carries_the_matching_mime_method() {
        let event = calendar::parse_ical("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART:20261002T120000Z\r\nORGANIZER:mailto:host@example.com\r\nATTENDEE:mailto:me@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").remove(0);
        let account = Account {
            path: String::new(),
            email: "me@example.com".into(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let original = Message {
            subject: "Meeting".into(),
            message_id: "invite@example.com".into(),
            ..Default::default()
        };
        let draft = draft_reply(&account, &original, &event, Response::Accepted).unwrap();
        assert_eq!(draft.to, "host@example.com");
        assert_eq!(draft.in_reply_to, Some(original.message_id.clone()));
        assert_eq!(draft.references, [original.message_id]);
        let part = mailparse::parse_mail(draft.attachments[0].as_bytes()).unwrap();
        assert_eq!(
            part.ctype.params.get("method").map(String::as_str),
            Some("REPLY")
        );
        let parsed = calendar::parse_ical(&part.get_body().unwrap());
        assert_eq!(parsed[0].attendees[0].status, "ACCEPTED");
    }
}
