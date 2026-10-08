use crate::{models::Message, theme};
use scraper::{Html, Selector};
use std::sync::LazyLock;

pub fn has_remote_media(message: &Message) -> bool {
    if let Some(remote) = message.remote_media {
        return remote;
    }
    if !message.body_loaded || message.body_html.is_empty() {
        return false;
    }
    let document = Html::parse_fragment(&message.body_html);
    let selector = Selector::parse("img[src], img[srcset], source[srcset], video[poster]").unwrap();
    document.select(&selector).any(|element| {
        ["src", "srcset", "poster"].into_iter().any(|attribute| {
            element.value().attr(attribute).is_some_and(|value| {
                value.split(',').any(|candidate| {
                    let url = candidate.trim_start();
                    url.starts_with("https://")
                        || url.starts_with("http://")
                        || url.starts_with("//")
                        || url.starts_with("cid:")
                })
            })
        })
    })
}

pub fn document(message: &Message, hide_quotes: bool, media: bool) -> String {
    let plain_text_body = message.body_html.is_empty();
    let body = if plain_text_body {
        plain_text(&message.body_text, hide_quotes)
    } else {
        normalized_html(&message.body_html)
    };
    static CID: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?i)cid:([^\s"'<>)]+)"#).unwrap());
    let body = if media && !message.inline_media.is_empty() {
        CID.replace_all(&body, |captures: &regex::Captures<'_>| {
            let id = captures[1].to_ascii_lowercase();
            message
                .inline_media
                .iter()
                .find(|part| part.content_id == id)
                .map(|part| format!("data:{};base64,{}", part.mime, part.data))
                .unwrap_or_else(|| captures[0].to_owned())
        })
        .into_owned()
    } else {
        body
    };
    format!(
        "<!doctype html><html><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data: https: http:;\"></head><body><div id='brevlada-content' data-plain-text='{plain_text_body}' data-hide-quotes='{hide_quotes}'>{body}</div><style>{}</style></body></html>",
        theme::HTML_CSS,
    )
}

fn normalized_html(source: &str) -> String {
    let document = Html::parse_document(source);
    let styles = Selector::parse("head style").unwrap();
    let body = Selector::parse("body").unwrap();
    let mut result = document
        .select(&styles)
        .map(|style| style.html())
        .collect::<String>();
    if let Some(body) = document.select(&body).next() {
        result.push_str("<div");
        for (name, value) in body.value().attrs() {
            if matches!(name, "style" | "class" | "id" | "dir" | "lang") {
                result.push_str(&format!(
                    " {name}=\"{}\"",
                    gtk::glib::markup_escape_text(value)
                ));
            }
        }
        result.push('>');
        result.push_str(&body.inner_html());
        result.push_str("</div>");
    }
    result
}

fn plain_text(text: &str, hide_quotes: bool) -> String {
    let lines: Vec<_> = text.lines().collect();
    let end = lines.iter().rposition(|line| !line.trim().is_empty());
    let start = end.and_then(|end| {
        if !lines[end].trim_start().starts_with('>') {
            return None;
        }
        let mut start = end;
        while start > 0 {
            let previous = lines[start - 1].trim_start();
            if !previous.is_empty() && !previous.starts_with('>') {
                break;
            }
            start -= 1;
        }
        Some(start)
    });
    let hidden_start = if hide_quotes {
        start.map(|mut index| {
            while index > 0 && lines[index - 1].trim().is_empty() {
                index -= 1;
            }
            if index > 0 && lines[index - 1].trim_end().ends_with("wrote:") {
                index -= 1;
            }
            index
        })
    } else {
        None
    };
    let mut result = String::from("<pre>");
    for (index, line) in lines.iter().take(end.map_or(0, |end| end + 1)).enumerate() {
        if hidden_start == Some(index) {
            break;
        }
        if start == Some(index) && !hide_quotes {
            result.push_str("</pre><details><summary>Quoted reply</summary><pre>");
        } else if index > 0 {
            result.push('\n');
        }
        result.push_str(&gtk::glib::markup_escape_text(line));
    }
    result.push_str("</pre>");
    if start.is_some() && !hide_quotes {
        result.push_str("</details>");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_documents_and_unbalanced_markup_stay_inside_the_measured_root() {
        let html = document(&Message {
            body_html: "<!doctype html><html><head><style>body{padding:0}</style></head><body class='newsletter' style='background:blue'><table><tr><td>Top</td></tr></table></div><p>Bottom</p></body></html>".into(),
            ..Default::default()
        }, false, false);
        let parsed = Html::parse_document(&html);
        let root = parsed
            .select(&Selector::parse("#brevlada-content").unwrap())
            .next()
            .unwrap();
        assert!(root.text().collect::<String>().contains("Bottom"));
        assert!(root.inner_html().contains("newsletter"));
        assert!(root.inner_html().contains("background:blue"));
        assert_eq!(parsed.select(&Selector::parse("html").unwrap()).count(), 1);
        assert!(html.find("body{padding:0}").unwrap() < html.find(theme::HTML_CSS).unwrap());
    }

    #[test]
    fn collapses_only_trailing_plain_text_quotes() {
        let html = plain_text(
            "Intro\n> inline quote\nMy reply\n> older reply\n> another line\n\n",
            false,
        );
        assert_eq!(html.matches("<details>").count(), 1);
        assert!(html.starts_with("<pre>Intro\n&gt; inline quote\nMy reply</pre><details>"));
        assert!(!plain_text("> quotation\nMy reply", false).contains("<details>"));
        assert!(!plain_text("Only my reply\n\n", false).contains("<details>"));
        let without_quote = plain_text(
            "My reply\nOn Tuesday, Alice wrote:\n\n> older message\n",
            true,
        );
        assert!(!without_quote.contains("older message"));
        assert!(!without_quote.contains("wrote:"));
        assert!(without_quote.contains("My reply"));
    }

    #[test]
    fn escapes_plain_text_and_collapses_quoted_replies_without_scripts() {
        let html = document(
            &Message {
                body_text: "<script>alert(1)</script>\n> old reply\nnew reply".into(),
                ..Default::default()
            },
            false,
            false,
        );
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<details>"));
        assert!(html.contains("default-src 'none'"));
    }

    #[test]
    fn remote_images_are_permitted_by_policy_for_webkit_to_load_on_request() {
        let message = Message {
            body_html: "<img src='https://example.com/image.png'>".into(),
            ..Default::default()
        };
        assert!(document(&message, false, false).contains("img-src data: https: http:;"));
    }

    #[test]
    fn short_plain_text_does_not_keep_trailing_blank_lines() {
        assert_eq!(plain_text("Hello\n\n \n", false), "<pre>Hello</pre>");
        assert_eq!(plain_text("Hello", false), "<pre>Hello</pre>");
    }

    #[test]
    fn remote_media_requires_a_loaded_html_image() {
        let mut message = Message {
            body_html: "<img src='https://example.com/image.png'>".into(),
            ..Default::default()
        };
        assert!(!has_remote_media(&message));
        message.body_loaded = true;
        assert!(has_remote_media(&message));
        message.body_html = "<p>Just text</p><img src='cid:logo'>".into();
        assert!(has_remote_media(&message));
    }

    #[test]
    fn replaces_cid_image_only_when_media_is_enabled() {
        let message = Message {
            body_html: "<img src='cid:logo@example.com'>".into(),
            inline_media: vec![crate::models::InlineMedia {
                content_id: "logo@example.com".into(),
                mime: "image/png".into(),
                data: "aGVsbG8=".into(),
            }],
            ..Default::default()
        };
        assert!(document(&message, false, false).contains("cid:logo@example.com"));
        assert!(document(&message, false, true).contains("data:image/png;base64,aGVsbG8="));
    }
}
