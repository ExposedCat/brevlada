use crate::{models::Message, theme};

pub fn document(message: &Message) -> String {
    let body = if message.body_html.is_empty() {
        plain_text(&message.body_text)
    } else {
        message.body_html.clone()
    };
    format!(
        "<!doctype html><html><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:;\"><style>{}</style></head><body><div id='brevlada-content'>{body}</div></body></html>",
        theme::HTML_CSS
    )
}

fn plain_text(text: &str) -> String {
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
    let mut result = String::from("<pre>");
    for (index, line) in lines.iter().enumerate() {
        if start == Some(index) {
            result.push_str("</pre><details><summary>Quoted reply</summary><pre>");
        }
        result.push_str(&gtk::glib::markup_escape_text(line));
        result.push('\n');
    }
    result.push_str("</pre>");
    if start.is_some() {
        result.push_str("</details>");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_only_trailing_plain_text_quotes() {
        let html = plain_text("Intro\n> inline quote\nMy reply\n> older reply\n> another line\n\n");
        assert_eq!(html.matches("<details>").count(), 1);
        assert!(html.starts_with("<pre>Intro\n&gt; inline quote\nMy reply\n</pre><details>"));
        assert!(!plain_text("> quotation\nMy reply").contains("<details>"));
        assert!(!plain_text("Only my reply\n\n").contains("<details>"));
    }

    #[test]
    fn escapes_plain_text_and_collapses_quoted_replies_without_scripts() {
        let html = document(&Message {
            body_text: "<script>alert(1)</script>\n> old reply\nnew reply".into(),
            ..Default::default()
        });
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<details>"));
        assert!(html.contains("default-src 'none'"));
    }
}
