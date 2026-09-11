use crate::models::Message;

pub fn sender(message: &Message) -> (String, String) {
    crate::models::senders::identity(message)
}

pub fn sender_name(message: &Message) -> String {
    let (name, email) = sender(message);
    if !name.is_empty() {
        name
    } else if !email.is_empty() {
        email
    } else {
        "Unknown Sender".into()
    }
}

pub fn date(message: &Message, with_time: bool) -> String {
    let Some(date) = chrono::DateTime::from_timestamp(message.timestamp, 0) else {
        return String::new();
    };
    let date = date.with_timezone(&chrono::Local);
    let days = chrono::Local::now().signed_duration_since(date).num_days();
    let format = if with_time {
        match days {
            0 => "Today %H:%M",
            1 => "Yesterday %H:%M",
            2..7 => "%a %H:%M",
            7..365 => "%b %d %H:%M",
            _ => "%m/%d/%y %H:%M",
        }
    } else {
        match days {
            0 => "%H:%M",
            1 => "Yesterday",
            2..7 => "%A",
            7..365 => "%b %d",
            _ => "%b %d, %Y",
        }
    };
    date.format(format).to_string()
}

pub fn subject(message: &Message) -> String {
    static PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(?:(?:re|fw|fwd|aw|antw|回复|转发):\s*)+").unwrap()
    });
    let subject = PREFIX.replace_all(&message.subject, "").trim().to_owned();
    if subject.is_empty() {
        "(No Subject)".into()
    } else {
        subject
    }
}
