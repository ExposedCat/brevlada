use super::{html::Html, schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub number: String,
    pub merchant: Option<String>,
    pub amount: String,
    pub date: Option<String>,
    pub invoice: bool,
    pub url: Option<String>,
    pub items: Vec<String>,
}

#[cfg(test)]
pub fn parse(html: &str) -> Vec<Receipt> {
    let html = Html::new(html);
    parse_in(&html)
}

pub fn parse_in(html: &Html<'_>) -> Vec<Receipt> {
    if !html.source.contains("Order") && !html.source.contains("Invoice") {
        return Vec::new();
    }
    let mut receipts = Vec::new();
    for value in schema::items(html, |kind| {
        matches!(kind.rsplit('/').next(), Some("Order" | "Invoice"))
    }) {
        collect(&value, &mut receipts);
    }
    receipts
}

fn field(value: &Value, key: &str) -> Option<String> {
    match &value[key] {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().into()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn collect(value: &Value, receipts: &mut Vec<Receipt>) {
    match value {
        Value::Array(values) => values.iter().for_each(|value| collect(value, receipts)),
        Value::Object(values) => {
            let kinds = match &value["@type"] {
                Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
                Value::String(kind) => vec![kind.as_str()],
                _ => Vec::new(),
            };
            let invoice = kinds
                .iter()
                .any(|kind| kind.rsplit('/').next() == Some("Invoice"));
            let order = kinds
                .iter()
                .any(|kind| kind.rsplit('/').next() == Some("Order"));
            if invoice || order {
                if let Some(receipt) = from_value(value, invoice) {
                    receipts.push(receipt);
                }
            } else {
                for child in values.values() {
                    collect(child, receipts);
                }
            }
        }
        _ => {}
    }
}

fn from_value(value: &Value, invoice: bool) -> Option<Receipt> {
    let number = field(
        value,
        if invoice {
            "confirmationNumber"
        } else {
            "orderNumber"
        },
    )
    .or_else(|| field(value, "identifier"))?;
    let total = &value["totalPaymentDue"];
    let price = field(value, "price")
        .or_else(|| field(total, "price"))
        .or_else(|| field(total, "value"))?;
    let numeric = price.parse::<f64>().ok()?;
    if !numeric.is_finite() || numeric < 0.0 {
        return None;
    }
    let currency = field(value, "priceCurrency")
        .or_else(|| field(total, "priceCurrency"))
        .or_else(|| field(total, "currency"))?;
    if currency.len() != 3
        || !currency
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    let merchant = &value[if invoice { "provider" } else { "seller" }];
    let action = &value["potentialAction"];
    let url = field(value, "url")
        .or_else(|| field(action, "url"))
        .or_else(|| field(action, "target"))
        .or_else(|| field(&action["target"], "urlTemplate"))
        .filter(|url| super::ticket::web_url(url));
    let offers = &value["acceptedOffer"];
    let offers: Vec<_> = offers
        .as_array()
        .map(|values| values.iter().collect())
        .unwrap_or_else(|| vec![offers]);
    let items = offers
        .into_iter()
        .filter_map(|offer| field(&offer["itemOffered"], "name").or_else(|| field(offer, "name")))
        .collect();
    Some(Receipt {
        number,
        merchant: merchant
            .as_str()
            .map(str::to_owned)
            .or_else(|| field(merchant, "name")),
        amount: format!("{} {price}", currency.to_ascii_uppercase()),
        date: field(value, "orderDate")
            .or_else(|| field(value, "dateIssued"))
            .or_else(|| field(value, "dateCreated")),
        invoice,
        url,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_explicit_number_amount_and_currency_in_structured_orders() {
        assert!(parse("Your order total is EUR 42.00. Order #123").is_empty());
        for fields in [
            r#""price":42"#,
            r#""price":"NaN","priceCurrency":"EUR""#,
            r#""price":-42,"priceCurrency":"EUR""#,
        ] {
            let html = format!(
                r#"<script type="application/ld+json">{{"@type":"Order","orderNumber":"123",{fields}}}</script>"#
            );
            assert!(parse(&html).is_empty());
        }
        let html = r#"<script type="application/ld+json">{"@graph":[{"@type":"Order","orderNumber":"123","price":42,"priceCurrency":"EUR","seller":{"name":"Shop"}}]}</script>"#;
        let receipts = parse(html);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].amount, "EUR 42");
        assert_eq!(receipts[0].merchant.as_deref(), Some("Shop"));
    }

    #[test]
    fn reads_invoice_microdata_with_a_nested_total() {
        let html = r#"<div itemscope itemtype="https://schema.org/Invoice"><meta itemprop="confirmationNumber" content="INV-1"><div itemprop="totalPaymentDue" itemscope itemtype="https://schema.org/PriceSpecification"><meta itemprop="price" content="19.50"><meta itemprop="priceCurrency" content="EUR"></div></div>"#;
        let receipts = parse(html);
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].invoice);
        assert_eq!(receipts[0].number, "INV-1");
        assert_eq!(receipts[0].amount, "EUR 19.50");
    }
}
