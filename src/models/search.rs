use super::Message;
use chrono::{Local, NaiveDate};

pub const SOURCE_LABELS: [&str; 6] = [
    "Subject",
    "Sender (From)",
    "Reply-To",
    "Recipients (To)",
    "Cc",
    "Body",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filters {
    pub sources: [bool; 6],
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            sources: [true; 6],
            from: None,
            to: None,
        }
    }
}

impl Filters {
    pub fn includes_date(&self, timestamp: i64) -> bool {
        if self.from.is_none() && self.to.is_none() {
            return true;
        }
        let Some(date) = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|date| date.with_timezone(&Local).date_naive())
        else {
            return false;
        };
        self.from.is_none_or(|from| date >= from) && self.to.is_none_or(|to| date <= to)
    }

    pub fn matches_fields(&self, fields: &[String], query: &str) -> bool {
        query.is_empty()
            || fields
                .iter()
                .enumerate()
                .any(|(index, field)| self.sources[index.min(5)] && field.contains(query))
    }

    pub fn dates(
        from: &str,
        to: &str,
    ) -> Result<(Option<NaiveDate>, Option<NaiveDate>), &'static str> {
        fn parse(value: &str) -> Result<Option<NaiveDate>, &'static str> {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| "Use dates in YYYY-MM-DD format")?;
            if date.format("%Y-%m-%d").to_string() != value {
                return Err("Use dates in YYYY-MM-DD format");
            }
            Ok(Some(date))
        }
        let (from, to) = (parse(from)?, parse(to)?);
        if from.zip(to).is_some_and(|(from, to)| from > to) {
            return Err("From must be on or before To");
        }
        Ok((from, to))
    }
}

/// Search the visible HTML text too, including older HTML-only cache entries.
pub fn html_text(html: &str) -> String {
    let document = scraper::Html::parse_document(html);
    let mut text = String::new();
    // Iterative traversal also handles deeply nested mail without using the stack.
    let mut pending = vec![Some(*document.root_element())];
    while let Some(node) = pending.pop() {
        let Some(node) = node else {
            text.push(' ');
            continue;
        };
        if let Some(value) = node.value().as_text() {
            text.push_str(value);
        } else if let Some(element) = scraper::ElementRef::wrap(node) {
            let name = element.value().name();
            if matches!(name, "script" | "style" | "head") {
                continue;
            }
            let block = matches!(
                name,
                "p" | "div"
                    | "br"
                    | "hr"
                    | "li"
                    | "tr"
                    | "td"
                    | "th"
                    | "section"
                    | "article"
                    | "blockquote"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "pre"
            );
            if block {
                text.push(' ');
                pending.push(None);
            }
            pending.extend(element.children().rev().map(Some));
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn fields(message: &Message) -> [std::borrow::Cow<'_, str>; 7] {
    [
        (&message.subject).into(),
        (&message.sender).into(),
        (&message.reply_to).into(),
        (&message.recipients).into(),
        (&message.cc).into(),
        (&message.body_text).into(),
        if message.body_html.is_empty() {
            "".into()
        } else {
            html_text(&message.body_html).into()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_control_headers_and_both_body_formats() {
        let fields = fields(&Message {
            subject: "Subject needle".into(),
            body_text: "Plain needle".into(),
            body_html: "<p>HTML needle</p>".into(),
            ..Default::default()
        })
        .iter()
        .map(|field| field.to_lowercase())
        .collect::<Vec<_>>();
        let mut filters = Filters {
            sources: [false; 6],
            ..Default::default()
        };
        assert!(!filters.matches_fields(&fields, "needle"));
        filters.sources[0] = true;
        assert!(filters.matches_fields(&fields, "subject needle"));
        assert!(!filters.matches_fields(&fields, "html needle"));
        filters.sources[5] = true;
        assert!(filters.matches_fields(&fields, "plain needle"));
        assert!(filters.matches_fields(&fields, "html needle"));
    }

    #[test]
    fn date_ranges_are_optional_inclusive_and_validated() {
        use chrono::TimeZone;
        let timestamp = Local
            .with_ymd_and_hms(2026, 10, 2, 23, 59, 59)
            .single()
            .unwrap()
            .timestamp();
        let date = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        assert!(Filters::default().includes_date(timestamp));
        let mut filters = Filters {
            from: Some(date),
            to: Some(date),
            ..Default::default()
        };
        assert!(filters.includes_date(timestamp));
        filters.from = None;
        assert!(filters.includes_date(timestamp));
        filters.to = date.pred_opt();
        assert!(!filters.includes_date(timestamp));
        filters.to = None;
        filters.from = date.succ_opt();
        assert!(!filters.includes_date(timestamp));
        assert_eq!(Filters::dates("", ""), Ok((None, None)));
        assert!(Filters::dates("2026-02-30", "").is_err());
        assert!(Filters::dates("2026-10-03", "2026-10-02").is_err());
        assert!(Filters::dates("2026-10-2", "").is_err());
    }

    #[test]
    fn searches_all_fields_and_visible_html_with_unicode_and_literal_substrings() {
        let message = Message {
            sender: "Alice <alice@example.com>".into(),
            recipients: "BÖB <bob@example.com>".into(),
            cc: "Carol <carol@example.com>".into(),
            subject: "Résumé 100%".into(),
            body_text: "Plain message".into(),
            body_html: "<style>hidden-style</style><p>HTML &amp; delivery</p>".into(),
            ..Default::default()
        };
        for query in [
            "alice",
            "böb",
            "carol",
            "résumé",
            "100%",
            "plain",
            "html & delivery",
        ] {
            assert!(message.matches(query), "{query}");
        }
        assert!(!message.matches("hidden-style"));
        assert!(!message.matches("<p>"));
        assert_eq!(
            html_text("<p>De<span>liv</span>ery</p><p>tomorrow</p>"),
            "Delivery tomorrow"
        );
    }
}
