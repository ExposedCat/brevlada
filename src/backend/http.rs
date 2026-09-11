use anyhow::Result;
use std::{sync::LazyLock, time::Duration};

const LIMIT: u64 = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(true)
        .user_agent(concat!("Brevlada/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(TIMEOUT))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
});

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }
}

/// Performs a bounded HTTPS GET, optionally with an OAuth2 bearer token.
/// Unlike `ureq`'s default, a non-2xx status is returned rather than raised so
/// callers can distinguish "no avatar here" from "the network is unavailable".
pub fn get(url: &str, token: Option<&str>) -> Result<Response> {
    let mut request = AGENT.get(url);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let mut response = request.call()?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()?;
    Ok(Response { status, body })
}

/// Percent-encodes a value for use inside a query string.
pub fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_reserved_character_in_an_address() {
        assert_eq!(encode("a.b-c_d~e"), "a.b-c_d~e");
        assert_eq!(encode("first+tag@example.com"), "first%2Btag%40example.com");
        assert_eq!(encode("a b&c=d?e/f"), "a%20b%26c%3Dd%3Fe%2Ff");
        assert_eq!(encode("é"), "%C3%A9");
    }
}
