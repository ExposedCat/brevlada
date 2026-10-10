use mailparse::{
    DispositionType, MailHeader, MailHeaderMap, ParsedContentDisposition, ParsedContentType,
    ParsedMail,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Details {
    pub mime: String,
    pub size: u64,
    #[serde(default)]
    pub calendar_rendered: Option<bool>,
}

pub fn name(mail: &ParsedMail<'_>) -> Option<String> {
    part_name(&mail.get_content_disposition(), &mail.ctype, &mail.headers)
}

pub fn part_name(
    disposition: &ParsedContentDisposition,
    ctype: &ParsedContentType,
    headers: &[MailHeader<'_>],
) -> Option<String> {
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| ctype.params.get("name"))
        .cloned();
    if disposition.disposition != DispositionType::Attachment
        && headers.get_first_value("Content-ID").is_some()
    {
        return None;
    }
    filename.or_else(|| {
        (disposition.disposition == DispositionType::Attachment)
            .then(|| "Unnamed attachment".into())
    })
}

pub fn safe_name(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    if name.trim().is_empty() || matches!(name.as_str(), "." | "..") {
        "attachment".into()
    } else {
        name
    }
}

pub fn rendered_calendar(message: &super::Message, index: usize) -> bool {
    if let Some(rendered) = message
        .attachment_details
        .get(index)
        .and_then(|details| details.calendar_rendered)
    {
        return rendered;
    }
    !message.calendar_events.is_empty()
        && (message
            .attachments
            .get(index)
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".ics"))
            || message
                .attachment_details
                .get(index)
                .is_some_and(|details| {
                    matches!(details.mime.as_str(), "text/calendar" | "application/ics")
                }))
}
