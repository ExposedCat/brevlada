use super::{Data, Namespace, NodeRef, Prefix};

/// Serializes like html5ever with scripting disabled, as scraper's `html()` does.
pub(super) fn html(root: NodeRef<'_>, include_root: bool) -> String {
    let mut output = String::new();
    let mut node = if include_root {
        Some(root)
    } else {
        root.children().next()
    };
    while let Some(current) = node {
        let mut descend = false;
        match &current.node().data {
            Data::Element(element) => {
                output.push('<');
                output.push_str(&element.name);
                for attribute in &element.attributes {
                    output.push(' ');
                    match attribute.prefix {
                        Prefix::None | Prefix::Xmlns => {}
                        Prefix::Xlink => output.push_str("xlink:"),
                        Prefix::Xml => output.push_str("xml:"),
                        Prefix::XmlnsColon => output.push_str("xmlns:"),
                    }
                    output.push_str(&attribute.name);
                    output.push_str("=\"");
                    escape(&mut output, &attribute.value, true);
                    output.push('"');
                }
                output.push('>');
                descend = !void(current);
            }
            Data::Text(text) if raw_text_parent(root, current, include_root) => {
                output.push_str(text);
            }
            Data::Text(text) => escape(&mut output, text, false),
            Data::Comment(text) => {
                output.push_str("<!--");
                output.push_str(text);
                output.push_str("-->");
            }
            Data::Root | Data::Contents => descend = true,
        }
        if descend && let Some(child) = current.children().next() {
            node = Some(child);
            continue;
        }
        if descend {
            close(&mut output, current);
        }
        node = next(root, current, include_root, &mut output);
    }
    output
}

/// Moves past `current` to the next node of the subtree, closing the elements it leaves.
fn next<'d>(
    root: NodeRef<'d>,
    mut current: NodeRef<'d>,
    include_root: bool,
    output: &mut String,
) -> Option<NodeRef<'d>> {
    loop {
        if current.id == root.id {
            return None;
        }
        if let Some(sibling) = current.link(current.node().next_sibling) {
            return Some(sibling);
        }
        current = current.parent()?;
        if current.id != root.id || include_root {
            close(output, current);
        }
    }
}

fn close(output: &mut String, node: NodeRef<'_>) {
    if let Data::Element(element) = &node.node().data {
        output.push_str("</");
        output.push_str(&element.name);
        output.push('>');
    }
}

fn html_element(node: NodeRef<'_>) -> Option<&str> {
    match &node.node().data {
        Data::Element(element) if element.namespace == Namespace::Html => Some(&element.name),
        _ => None,
    }
}

/// HTML void elements, whose end tags and children html5ever skips.
fn void(node: NodeRef<'_>) -> bool {
    html_element(node).is_some_and(|name| {
        matches!(
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
        )
    })
}

/// Text in these elements is written unescaped; html5ever does not know the
/// parent of the children it serializes without their root.
fn raw_text_parent(root: NodeRef<'_>, text: NodeRef<'_>, include_root: bool) -> bool {
    let parent = text
        .parent()
        .filter(|parent| include_root || parent.id != root.id);
    parent.and_then(html_element).is_some_and(|name| {
        matches!(
            name,
            "style" | "script" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext"
        )
    })
}

fn escape(output: &mut String, text: &str, attribute: bool) {
    let mut start = 0;
    for (index, character) in text.char_indices() {
        let escaped = match character {
            '&' => "&amp;",
            '\u{a0}' => "&nbsp;",
            '"' if attribute => "&quot;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => continue,
        };
        output.push_str(&text[start..index]);
        output.push_str(escaped);
        start = index + character.len_utf8();
    }
    output.push_str(&text[start..]);
}
