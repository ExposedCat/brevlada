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

#[derive(Debug, PartialEq, Eq)]
pub enum UnsubscribeResponse {
    Done,
    UseBrowser,
}

pub fn unsubscribe(url: &str) -> Result<UnsubscribeResponse> {
    anyhow::ensure!(
        url.starts_with("https://"),
        "One-click unsubscribe requires HTTPS"
    );
    unsubscribe_with(&AGENT, url)
}

fn unsubscribe_with(agent: &ureq::Agent, url: &str) -> Result<UnsubscribeResponse> {
    let response = agent
        .post(url)
        .config()
        .max_redirects(0)
        .build()
        .send_form([("List-Unsubscribe", "One-Click")])?;
    // Some senders advertise one-click support but redirect to a web flow.
    // Keep the POST unredirected (RFC 8058); let the user complete the manual
    // flow at the original List-Unsubscribe URL rather than assume success.
    if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        return Ok(UnsubscribeResponse::UseBrowser);
    }
    anyhow::ensure!(
        response.status().is_success(),
        "Unsubscribe server returned HTTP {}",
        response.status().as_u16()
    );
    Ok(UnsubscribeResponse::Done)
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
    use std::io::{Read, Write};

    fn unsubscribe_server(
        status: &str,
        headers: &str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/unsubscribe?token=opaque",
            listener.local_addr().unwrap()
        );
        let response =
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n{headers}\r\n");
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "Connection closed before request body");
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("Content-Length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, thread)
    }

    fn unsubscribe_agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .new_agent()
    }

    #[test]
    fn sends_one_click_post_with_form_body_and_no_browser_credentials() {
        let (url, server) = unsubscribe_server("204 No Content", "");
        assert_eq!(
            unsubscribe_with(&unsubscribe_agent(), &url).unwrap(),
            UnsubscribeResponse::Done
        );
        let request = server.join().unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /unsubscribe?token=opaque HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("content-type: application/x-www-form-urlencoded")
        );
        assert_eq!(body, "List-Unsubscribe=One-Click");
        for forbidden in ["cookie:", "authorization:", "referer:"] {
            assert!(!headers.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn reports_http_errors() {
        for status in ["405 Method Not Allowed", "500 Internal Server Error"] {
            let (url, server) = unsubscribe_server(status, "");
            let error = unsubscribe_with(&unsubscribe_agent(), &url).unwrap_err();
            assert!(error.to_string().contains(&status[..3]), "{error}");
            server.join().unwrap();
        }
    }

    #[test]
    fn redirects_require_browser_completion_without_following_the_post() {
        for status in [
            "301 Moved Permanently",
            "302 Found",
            "303 See Other",
            "307 Temporary Redirect",
            "308 Permanent Redirect",
        ] {
            let (url, server) = unsubscribe_server(
                status,
                "Location: http://127.0.0.1:1/should-not-be-contacted\r\n",
            );
            assert_eq!(
                unsubscribe_with(&unsubscribe_agent(), &url).unwrap(),
                UnsubscribeResponse::UseBrowser,
                "{status}"
            );
            assert!(server.join().unwrap().starts_with("POST "));
        }
    }

    #[test]
    fn refuses_non_https_one_click_endpoints() {
        let error = unsubscribe("http://127.0.0.1:1/unsubscribe").unwrap_err();
        assert!(error.to_string().contains("requires HTTPS"));
    }

    #[test]
    fn encodes_every_reserved_character_in_an_address() {
        assert_eq!(encode("a.b-c_d~e"), "a.b-c_d~e");
        assert_eq!(encode("first+tag@example.com"), "first%2Btag%40example.com");
        assert_eq!(encode("a b&c=d?e/f"), "a%20b%26c%3Dd%3Fe%2Ff");
        assert_eq!(encode("é"), "%C3%A9");
    }
}
