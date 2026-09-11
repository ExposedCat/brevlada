use crate::models::Message;
use gtk::prelude::*;

pub fn insert(buffer: &gtk::TextBuffer, message: &Message) {
    buffer.set_text(&format!("\n\n{}", quoted(&message.body_text)));
    buffer.place_cursor(&buffer.start_iter());
}

fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
