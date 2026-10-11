use super::html::{self, HTML_NAMESPACE, Html};
use lol_html::{Settings, element, text};
use regex::Regex;
use scraper::{ElementRef, Selector};
use serde_json::{Map, Value};
use std::{cell::RefCell, sync::LazyLock};

const JSON_LD: &str = "application/ld+json";

/// Every JSON-LD script that holds valid JSON, in document order.
pub fn json_ld(html: &Html<'_>) -> Vec<Value> {
    streamed_json_ld(html.source).unwrap_or_else(|| {
        static SCRIPTS: LazyLock<Selector> =
            LazyLock::new(|| Selector::parse("script[type='application/ld+json']").unwrap());
        html.fragment()
            .select(&SCRIPTS)
            .filter_map(|script| serde_json::from_str(&script.text().collect::<String>()).ok())
            .collect()
    })
}

/// [`json_ld`] read without a tree, unless only html5ever's tree reads a script right.
fn streamed_json_ld(source: &str) -> Option<Vec<Value>> {
    if !html::streamable(source) {
        return None;
    }
    // The text read so far of the JSON-LD script being scanned.
    let script = RefCell::new(None::<String>);
    let mut values = Vec::new();
    let read_type = element!("script", |element| {
        let json_ld = html::attribute(source, element, "type")
            .is_some_and(|kind| kind.eq_ignore_ascii_case(JSON_LD));
        // html5ever reads SVG and MathML scripts as markup.
        if json_ld && element.namespace_uri() != HTML_NAMESPACE {
            return Err("foreign JSON-LD script".into());
        }
        *script.borrow_mut() = json_ld.then(String::new);
        Ok(())
    });
    let read_text = text!("script", |chunk| {
        let mut script = script.borrow_mut();
        if let Some(text) = script.as_mut() {
            text.push_str(chunk.as_str());
            if chunk.last_in_text_node() {
                // html5ever reads NUL in scripts as U+FFFD.
                if let Ok(value) = serde_json::from_str(&text.replace('\0', "\u{FFFD}")) {
                    values.push(value);
                }
                *script = None;
            }
        }
        Ok(())
    });
    html::scan(
        source,
        Settings::new()
            .append_element_content_handler(read_type)
            .append_element_content_handler(read_text),
    )
    .ok()?;
    Some(values)
}

/// Elements that start a typed microdata item, parsing only sources that can have any.
pub fn scopes<'h>(html: &'h Html<'_>) -> impl Iterator<Item = ElementRef<'h>> {
    static ITEMSCOPE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)itemscope").unwrap());
    static SCOPES: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("[itemscope][itemtype]").unwrap());
    ITEMSCOPE
        .is_match(html.source)
        .then(|| html.fragment().select(&SCOPES))
        .into_iter()
        .flatten()
}

/// JSON-LD scripts, then microdata items not inside an item with a type `nested` accepts.
pub fn items<'h>(
    html: &'h Html<'_>,
    nested: impl Fn(&str) -> bool + 'h,
) -> impl Iterator<Item = Value> + 'h {
    let outer = move |element: &ElementRef<'_>| {
        !element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|parent| {
                parent
                    .value()
                    .attr("itemtype")
                    .is_some_and(|types| types.split_whitespace().any(&nested))
            })
    };
    json_ld(html).into_iter().chain(
        scopes(html)
            .filter(outer)
            .map(|element| microdata(&element)),
    )
}

pub fn microdata(element: &ElementRef<'_>) -> Value {
    let mut map = Map::new();
    if let Some(kind) = element.value().attr("itemtype") {
        map.insert(
            "@type".into(),
            Value::String(kind.rsplit('/').next().unwrap_or(kind).into()),
        );
    }
    for child in element.descendants().skip(1).filter_map(ElementRef::wrap) {
        let Some(property) = child.value().attr("itemprop") else {
            continue;
        };
        let nearest_scope = child
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|ancestor| ancestor.value().attr("itemscope").is_some());
        if nearest_scope.is_none_or(|scope| scope.id() != element.id()) {
            continue;
        }
        let value = if child.value().attr("itemscope").is_some() {
            microdata(&child)
        } else {
            let text = ["content", "href", "src", "datetime"]
                .iter()
                .find_map(|name| child.value().attr(name))
                .map(str::to_owned)
                .unwrap_or_else(|| child.text().collect::<String>().trim().to_owned());
            Value::String(text)
        };
        if let Some(previous) = map.get_mut(property) {
            if let Value::Array(items) = previous {
                items.push(value);
            } else {
                let old = std::mem::replace(previous, Value::Null);
                *previous = Value::Array(vec![old, value]);
            }
        } else {
            map.insert(property.to_owned(), value);
        }
    }
    Value::Object(map)
}
