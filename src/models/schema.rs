use scraper::ElementRef;
use serde_json::{Map, Value};

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
