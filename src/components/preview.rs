use crate::models::Message;
use scraper::{ElementRef, Html, Node};

pub fn message(message: &Message) -> String {
    let text = if message.body_text.trim().is_empty() {
        let document = Html::parse_document(&message.body_html);
        let mut text = String::new();
        append_text(document.root_element(), &mut text);
        text
    } else {
        message.body_text.clone()
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
