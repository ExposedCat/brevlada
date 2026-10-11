use crate::{
    models::{
        Message,
        html::{self, HTML_NAMESPACE},
    },
    theme,
};
use ego_tree::{NodeRef, iter::Edge};
use lol_html::{Settings, element, html_content::Element};
use scraper::{Html, Node, Selector};
use std::sync::LazyLock;

pub fn has_remote_media(message: &Message) -> bool {
    if let Some(remote) = message.remote_media {
        return remote;
    }
    if !message.body_loaded || message.body_html.is_empty() {
        return false;
    }
    let source = &message.body_html;
    if !html::streamable(source) {
        let document = Html::parse_fragment(source);
        return document
            .select(&Selector::parse(MEDIA).unwrap())
            .any(|element| {
                MEDIA_ATTRIBUTES
                    .into_iter()
                    .any(|attribute| element.value().attr(attribute).is_some_and(remote))
            });
    }
    let check = |element: &mut Element<'_, '_>| {
        let remote = MEDIA_ATTRIBUTES.into_iter().any(|attribute| {
            html::attribute(source, element, attribute).is_some_and(|value| remote(&value))
        });
        // Failing stops the scan at the first remote resource.
        if remote {
            Err("remote media".into())
        } else {
            Ok(())
        }
    };
    html::scan(
        source,
        Settings::new()
            .append_element_content_handler(element!(MEDIA, &check))
            // html5ever reads an HTML `<image>` as `<img>`.
            .append_element_content_handler(element!("image[src], image[srcset]", |element| {
                if element.namespace_uri() == HTML_NAMESPACE {
                    check(element)
                } else {
                    Ok(())
                }
            })),
    )
    .is_err()
}

const MEDIA: &str = "img[src], img[srcset], source[srcset], video[poster]";
const MEDIA_ATTRIBUTES: [&str; 3] = ["src", "srcset", "poster"];

fn remote(value: &str) -> bool {
    value.split(',').any(|candidate| {
        let url = candidate.trim_start();
        url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("//")
            || url.starts_with("cid:")
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
    let mut result = String::new();
    for style in document.select(&styles) {
        serialize(&mut result, *style, true);
    }
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
        serialize(&mut result, *body, false);
        result.push_str("</div>");
    }
    result
}

/// Writes what html5ever's serializer writes for `root`, without its
/// character-at-a-time output.
fn serialize(html: &mut String, root: NodeRef<'_, Node>, include_root: bool) {
    // Per open element: whether its text is written raw, and whether its children are left out.
    let mut open: Vec<(bool, bool)> = Vec::new();
    for edge in root.traverse() {
        match edge {
            Edge::Open(node) if include_root || node != root => match node.value() {
                Node::Doctype(doctype) => {
                    html.push_str("<!DOCTYPE ");
                    html.push_str(doctype.name());
                    html.push('>');
                }
                Node::Comment(comment) => {
                    html.push_str("<!--");
                    html.push_str(comment);
                    html.push_str("-->");
                }
                Node::Text(text) => {
                    if open.last().is_some_and(|&(raw, _)| raw) {
                        html.push_str(text);
                    } else {
                        escape(html, text, false);
                    }
                }
                Node::Element(element) => {
                    let ignored = open.last().is_some_and(|&(_, ignored)| ignored);
                    let name = element.name();
                    if !ignored {
                        html.push('<');
                        html.push_str(name);
                        for (attribute, value) in &element.attrs {
                            html.push(' ');
                            // html5ever gives `xmlns` itself an empty prefix.
                            if let Some(prefix) = &attribute.prefix
                                && !prefix.is_empty()
                            {
                                html.push_str(prefix);
                                html.push(':');
                            }
                            html.push_str(&attribute.local);
                            html.push_str("=\"");
                            escape(html, value, true);
                            html.push('"');
                        }
                        html.push('>');
                    }
                    let is_html = *element.name.ns == *HTML_NAMESPACE;
                    open.push((
                        is_html
                            && matches!(
                                name,
                                "style"
                                    | "script"
                                    | "xmp"
                                    | "iframe"
                                    | "noembed"
                                    | "noframes"
                                    | "plaintext"
                            ),
                        ignored
                            || is_html
                                && matches!(
                                    name,
                                    "area"
                                        | "base"
                                        | "basefont"
                                        | "bgsound"
                                        | "br"
                                        | "col"
                                        | "embed"
                                        | "frame"
                                        | "hr"
                                        | "img"
                                        | "input"
                                        | "keygen"
                                        | "link"
                                        | "meta"
                                        | "param"
                                        | "source"
                                        | "track"
                                        | "wbr"
                                ),
                    ));
                }
                _ => {}
            },
            Edge::Close(node) if include_root || node != root => {
                if let Node::Element(element) = node.value()
                    && open.pop().is_some_and(|(_, ignored)| !ignored)
                {
                    html.push_str("</");
                    html.push_str(element.name());
                    html.push('>');
                }
            }
            _ => {}
        }
    }
}

fn escape(html: &mut String, text: &str, attribute: bool) {
    let mut written = 0;
    for (index, special) in text.match_indices(|character| {
        matches!(character, '&' | '\u{a0}' | '<' | '>') || attribute && character == '"'
    }) {
        html.push_str(&text[written..index]);
        html.push_str(match special {
            "&" => "&amp;",
            "\u{a0}" => "&nbsp;",
            "<" => "&lt;",
            ">" => "&gt;",
            _ => "&quot;",
        });
        written = index + special.len();
    }
    html.push_str(&text[written..]);
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
