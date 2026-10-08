use crate::models::{Message, calendar};
use scraper::{ElementRef, Html, Node};

pub fn message(message: &Message) -> String {
    if let Some(preview) = &message.list_preview {
        return preview.clone();
    }
    if calendar::is_reply(message) {
        return String::new();
    }
    let text = if message.body_text.trim().is_empty() {
        let document = Html::parse_document(&message.body_html);
        let mut text = String::new();
        append_text(document.root_element(), &mut text);
        text
    } else {
        message.body_text.chars().take(4096).collect()
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = text.chars();
    let mut preview: String = characters.by_ref().take(160).collect();
    if characters.next().is_some() {
        preview.truncate(preview.trim_end().len());
        preview.push('…');
    }
    preview
}

pub fn thread(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|message| !calendar::is_reply(message))
        .map(message)
        .unwrap_or_default()
}

fn append_text(element: ElementRef<'_>, text: &mut String) {
    if matches!(
        element.value().name(),
        "head" | "script" | "style" | "template"
    ) {
        return;
    }
    let block = matches!(
        element.value().name(),
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "br"
            | "div"
            | "dl"
            | "dt"
            | "dd"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "ul"
    );
    if block {
        text.push(' ');
    }
    for child in element.children() {
        match child.value() {
            Node::Text(value) => text.push_str(value),
            Node::Element(_) => append_text(ElementRef::wrap(child).unwrap(), text),
            _ => {}
        }
    }
    if block {
        text.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_rsvp_bodies_before_the_invitation_or_body_loads() {
        let mut reply = Message {
            body_text: "Accepted: Lunch".into(),
            body_html: "<p>Accepted: Lunch</p>".into(),
            calendar_events: vec![calendar::Event {
                method: Some("REPLY".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        for loaded in [false, true] {
            reply.body_loaded = loaded;
            assert_eq!(message(&reply), "");
            assert_eq!(thread(std::slice::from_ref(&reply)), "");
            assert_eq!(
                thread(&[
                    reply.clone(),
                    Message {
                        body_text: "Join us for lunch".into(),
                        ..Default::default()
                    }
                ]),
                "Join us for lunch"
            );
        }
        reply.is_draft = true;
        assert_eq!(message(&reply), "Accepted: Lunch");
        reply.is_draft = false;
        reply.calendar_events.push(calendar::Event {
            method: Some("REQUEST".into()),
            name: "Another event".into(),
            ..Default::default()
        });
        assert_eq!(message(&reply), "Accepted: Lunch");
    }

    #[test]
    fn prefers_plain_text_and_collapses_whitespace() {
        assert_eq!(
            message(&Message {
                body_text: "  Hello\n\tworld <3  ".into(),
                body_html: "<p>Different text</p>".into(),
                ..Default::default()
            }),
            "Hello world <3"
        );
    }

    #[test]
    fn extracts_html_text_without_metadata_or_tags() {
        assert_eq!(message(&Message {
            body_text: " \n ".into(),
            body_html: "<head><title>Title</title><style>p { color: red }</style></head><body><!-- comment --><script>ignored()</script><template>hidden</template><p data-value='>'>Hello <b>world</b>! &amp; &#x1F600;</p><div>Next<br>line&nbsp;here</div></body>".into(),
            ..Default::default()
        }), "Hello world! & 😀 Next line here");
    }

    #[test]
    fn handles_empty_and_truncates_unicode() {
        assert_eq!(message(&Message::default()), "");
        assert_eq!(
            message(&Message {
                body_text: "é".repeat(161),
                ..Default::default()
            }),
            format!("{}…", "é".repeat(160))
        );
    }
}
