use super::Parcel;

fn alza_order(url: &str) -> Option<&str> {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = url.split_once('/')?;
    if !matches!(host, "www.alza.cz" | "alza.cz") {
        return None;
    }
    let number = path
        .strip_prefix("my-account/objednavka-")?
        .split_once(".htm")?
        .0;
    (!number.is_empty() && number.bytes().all(|ch| ch.is_ascii_digit())).then_some(number)
}

impl Parcel {
    pub fn order_id(&self) -> Option<&str> {
        self.order_number
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .or_else(|| self.order_link().and_then(alza_order))
    }

    pub fn order_link(&self) -> Option<&str> {
        self.order_url.as_deref().or_else(|| {
            self.details["provider"]["url"]
                .as_str()
                .filter(|url| alza_order(url).is_some())
        })
    }

    pub(super) fn hydrate_identity(&mut self) {
        if self.order_number.is_none() {
            self.order_number = self.order_id().map(str::to_owned);
        }
        if self.order_url.is_none() {
            self.order_url = self.order_link().map(str::to_owned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_alza_order_links_identify_orders_without_confusing_provider_homepages() {
        assert_eq!(
            alza_order("https://www.alza.cz/my-account/objednavka-123.htm?x=first"),
            Some("123")
        );
        assert_eq!(
            alza_order("https://www.alza.cz/my-account/objednavka-123.htm?x=second"),
            Some("123")
        );
        assert_eq!(alza_order("https://www.alza.cz/"), None);
        assert_eq!(
            alza_order("https://other.example/my-account/objednavka-123.htm"),
            None
        );
    }
}
