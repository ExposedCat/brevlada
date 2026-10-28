//! Email HTML: html5ever trees for readers that need structure, and a
//! streaming lol_html scan for checks that only look at tags.
#[cfg(test)]
mod parity;

use lol_html::{HtmlRewriter, Settings, errors::RewritingError, html_content::Element};
use regex::Regex;
use scraper::ElementRef;
use std::{cell::OnceCell, sync::LazyLock};

pub const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// An HTML source parsed on first use, at most once per parse mode, so every
/// reader of one message body shares the same trees.
pub struct Html<'a> {
    pub source: &'a str,
    fragment: OnceCell<scraper::Html>,
    document: OnceCell<scraper::Html>,
}

impl<'a> Html<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            fragment: OnceCell::new(),
            document: OnceCell::new(),
        }
    }

    pub fn fragment(&self) -> &scraper::Html {
        self.fragment
            .get_or_init(|| scraper::Html::parse_fragment(self.source))
    }

    pub fn document(&self) -> &scraper::Html {
        self.document
            .get_or_init(|| scraper::Html::parse_document(self.source))
    }
}

/// The element's text without `skip` elements, with `block` elements set
/// apart by spaces. Template contents are left out.
pub fn layout_text(
    element: ElementRef<'_>,
    skip: impl Fn(&str) -> bool,
    block: impl Fn(&str) -> bool,
) -> String {
    text_chunks(element, skip, block).collect()
}

pub fn text_chunks<'a>(
    element: ElementRef<'a>,
    skip: impl Fn(&str) -> bool,
    block: impl Fn(&str) -> bool,
) -> impl Iterator<Item = &'a str> {
    // `None` closes a block element; walking without recursion handles any nesting depth.
    let mut pending = vec![Some(*element)];
    std::iter::from_fn(move || {
        while let Some(node) = pending.pop() {
            let Some(node) = node else {
                return Some(" ");
            };
            if let Some(value) = node.value().as_text() {
                return Some(value.as_ref());
            } else if let Some(element) = ElementRef::wrap(node)
                && !skip(element.value().name())
            {
                let separated = block(element.value().name());
                if separated {
                    pending.push(None);
                }
                pending.extend(node.children().rev().map(Some));
                if separated {
                    return Some(" ");
                }
            }
        }
        None
    })
}

/// Whether lol_html puts every element of `source` in the namespace html5ever
/// does for scraper, which never makes `<annotation-xml>` an HTML integration
/// point as the standard says.
pub fn streamable(source: &str) -> bool {
    static ANNOTATION_XML: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("(?i)annotation-xml").unwrap());
    !ANNOTATION_XML.is_match(source)
}

/// Streams `source` through the handlers in `settings` without building a tree.
pub fn scan(source: &str, settings: Settings<'_, '_>) -> Result<(), RewritingError> {
    // Strict mode refuses tag soup whose parsing depends on the tree, like scripts in `<select>`.
    let mut rewriter = HtmlRewriter::new(settings.with_strict(false), |_: &[u8]| {});
    rewriter.write(source.as_bytes())?;
    rewriter.end()
}

/// An attribute of a scanned element as html5ever reads it: lol_html leaves
/// character references in values, so values with any are read from the
/// start tag by html5ever itself.
pub fn attribute(source: &str, element: &Element<'_, '_>, name: &str) -> Option<String> {
    let value = element.get_attribute(name)?;
    if !value.contains('&') {
        return Some(value);
    }
    let start_tag = scraper::Html::parse_fragment(&source[element.source_location().bytes()]);
    start_tag
        .root_element()
        .first_child()
        .and_then(ElementRef::wrap)
        .and_then(|element| element.value().attr(name))
        .map(str::to_owned)
}
