use crate::models::{
    Message, calendar,
    html::{Html, text_chunks},
};

pub fn message(message: &Message, html: &Html) -> String {
    if let Some(preview) = &message.list_preview {
        return preview.clone();
    }
    if calendar::is_reply_in(message, html) {
        return String::new();
    }
    if message.body_html.is_empty() {
        short_text(&message.body_text)
    } else {
        short_characters(
            text_chunks(
                html.document().root_element(),
                |name| matches!(name, "head" | "script" | "style" | "template"),
                |name| {
                    matches!(
                        name,
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
                    )
                },
            )
            .flat_map(str::chars),
        )
    }
}

pub(crate) fn short_text(text: &str) -> String {
    short_characters(text.chars())
}

fn short_characters(characters: impl Iterator<Item = char>) -> String {
    let mut preview = String::new();
    let mut length = 0;
    let mut space = false;
    for character in characters {
        if character.is_whitespace() {
            space = length > 0;
            continue;
        }
        if length + usize::from(space) >= 160 {
            preview.push('…');
            break;
        }
        if space {
            preview.push(' ');
            length += 1;
            space = false;
        }
        preview.push(character);
        length += 1;
    }
    preview
}

pub fn thread(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|message| !calendar::is_reply(message))
        .map(|found| message(found, &Html::new(&found.body_html)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(message: &Message) -> String {
        super::message(message, &Html::new(&message.body_html))
    }

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
            assert_eq!(preview(&reply), "");
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
        assert_eq!(preview(&reply), "Accepted: Lunch");
        reply.is_draft = false;
        reply.calendar_events.push(calendar::Event {
            method: Some("REQUEST".into()),
            name: "Another event".into(),
            ..Default::default()
        });
        assert_eq!(preview(&reply), "Accepted: Lunch");
    }

    #[test]
    fn prefers_cleaned_html_over_the_plain_text_alternative() {
        assert_eq!(
            preview(&Message {
                body_text: "p { margin:10px 0; padding:0; } table { border-collapse:collapse; }".into(),
                body_html: "<head><style>p { margin:10px 0; padding:0; }</style></head><body><p>Hello <b>world</b> &amp; friends</p></body>".into(),
                ..Default::default()
            }),
            "Hello world & friends"
        );
    }

    #[test]
    fn falls_back_to_plain_text_only_without_html() {
        assert_eq!(
            preview(&Message {
                body_text: "  Hello\n\tworld <3  ".into(),
                ..Default::default()
            }),
            "Hello world <3"
        );
        assert_eq!(
            preview(&Message {
                body_text: "p { color: red }".into(),
                body_html: "<style>p { color: red }</style><img src='cid:photo'>".into(),
                ..Default::default()
            }),
            ""
        );
    }

    #[test]
    fn extracts_html_text_without_metadata_or_tags() {
        assert_eq!(preview(&Message {
            body_text: " \n ".into(),
            body_html: "<head><title>Title</title><style>p { color: red }</style></head><body><!-- comment --><script>ignored()</script><template>hidden</template><p data-value='>'>Hello <b>world</b>! &amp; &#x1F600;</p><div>Next<br>line&nbsp;here</div></body>".into(),
            ..Default::default()
        }), "Hello world! & 😀 Next line here");
    }

    #[test]
    fn handles_empty_and_truncates_unicode() {
        assert_eq!(preview(&Message::default()), "");
        assert_eq!(
            preview(&Message {
                body_text: "é".repeat(161),
                ..Default::default()
            }),
            format!("{}…", "é".repeat(160))
        );
    }

    #[test]
    fn shortening_stops_after_the_first_extra_non_whitespace_character() {
        let mut read = 0;
        assert_eq!(
            short_characters("é".repeat(100_000).chars().inspect(|_| read += 1)),
            format!("{}…", "é".repeat(160))
        );
        assert_eq!(read, 161);
        for text in [
            "",
            " \n\t ",
            "Hello\nworld\u{a0}!",
            &"é".repeat(160),
            &format!("{}  \n ", "é".repeat(160)),
            &format!("{}  z", "é".repeat(159)),
            &format!("{}  zz", "é".repeat(158)),
        ] {
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut expected: String = normalized.chars().take(160).collect();
            if normalized.chars().count() > 160 {
                expected.truncate(expected.trim_end().len());
                expected.push('…');
            }
            assert_eq!(short_text(text), expected);
        }
    }
}
