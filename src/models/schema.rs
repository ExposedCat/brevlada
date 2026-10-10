use super::dom::{Document, ElementRef, NodeRef};
use serde_json::{Map, Value};

const JSON_LD: &str = "application/ld+json";

/// Every JSON-LD script that holds valid JSON, in document order.
pub fn json_ld<'d>(document: &'d Document<'_>) -> impl Iterator<Item = Value> + 'd {
    document
        .elements()
        .filter(|element| {
            element.name() == "script"
                && element.attr("type").is_some_and(|kind| {
                    // Like a CSS attribute selector, HTML elements compare `type` ignoring case.
                    if element.is_html() {
                        kind.eq_ignore_ascii_case(JSON_LD)
                    } else {
                        kind == JSON_LD
                    }
                })
        })
        .filter_map(|script| serde_json::from_str(&script.text().collect::<String>()).ok())
}

/// Elements that start a typed microdata item.
pub fn scopes<'d>(document: &'d Document<'_>) -> impl Iterator<Item = ElementRef<'d>> {
    document
        .elements()
        .filter(|element| element.attr("itemscope").is_some() && element.attr("itemtype").is_some())
}

/// JSON-LD scripts, then microdata items not inside an item with a type `nested` accepts.
pub fn items<'d>(
    document: &'d Document<'_>,
    nested: impl Fn(&str) -> bool + 'd,
) -> impl Iterator<Item = Value> + 'd {
    let outer = move |element: &ElementRef<'_>| {
        !element
            .node()
            .ancestors()
            .filter_map(NodeRef::as_element)
            .any(|parent| {
                parent
                    .attr("itemtype")
                    .is_some_and(|types| types.split_whitespace().any(&nested))
            })
    };
    json_ld(document).chain(scopes(document).filter(outer).map(microdata))
}

pub fn microdata(element: ElementRef<'_>) -> Value {
    let mut map = Map::new();
    if let Some(kind) = element.attr("itemtype") {
        map.insert(
            "@type".into(),
            Value::String(kind.rsplit('/').next().unwrap_or(kind).into()),
        );
    }
    for child in element
        .node()
        .descendants()
        .skip(1)
        .filter_map(NodeRef::as_element)
    {
        let Some(property) = child.attr("itemprop") else {
            continue;
        };
        let nearest_scope = child
            .node()
            .ancestors()
            .filter_map(NodeRef::as_element)
            .find(|ancestor| ancestor.attr("itemscope").is_some());
        if nearest_scope != Some(element) {
            continue;
        }
        let value = if child.attr("itemscope").is_some() {
            microdata(child)
        } else {
            let text = ["content", "href", "src", "datetime"]
                .iter()
                .find_map(|name| child.attr(name))
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
