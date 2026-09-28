use crate::{models::Message, theme};
use scraper::{Html, Selector};

pub fn has_remote_media(message: &Message) -> bool {
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
                })
            })
        })
    })
}

pub fn document(message: &Message, hide_quotes: bool) -> String {
    let body = if message.body_html.is_empty() {
        plain_text(&message.body_text, hide_quotes)
    } else {
        message.body_html.clone()
    };
    format!(
        "<!doctype html><html><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data: https: http:;\"><style>{}</style></head><body><div id='brevlada-content' data-hide-quotes='{hide_quotes}'>{body}</div></body></html>",
        theme::HTML_CSS,
    )
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
        assert!(document(&message, false).contains("img-src data: https: http:;"));
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
        assert!(!has_remote_media(&message));
    }
}
