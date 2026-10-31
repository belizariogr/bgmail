//! Minimal blocking HTTP shared by OAuth and autoconfig (ureq over the same
//! rustls + OS trust store as IMAP/SMTP), plus URL form encoding helpers.

use std::time::Duration;

use mail_core::{RemoteError, RemoteErrorKind};
use ureq::tls::{RootCerts, TlsConfig};

/// A response we read fully: status and body text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Agent with a global `timeout`, OS certificate verification, and non-2xx
/// statuses returned as data (OAuth errors carry a JSON body).
pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(concat!("BGMail/", env!("CARGO_PKG_VERSION")))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build();
    ureq::Agent::new_with_config(config)
}

/// GET `url`, reading at most a few MB of text.
pub(crate) fn get(url: &str, timeout: Duration) -> Result<HttpResponse, RemoteError> {
    let response = agent(timeout).get(url).call().map_err(map_error)?;
    read(response)
}

/// POST an `application/x-www-form-urlencoded` body.
pub(crate) fn post_form(
    url: &str,
    form: &[(&str, &str)],
    timeout: Duration,
) -> Result<HttpResponse, RemoteError> {
    let response = agent(timeout)
        .post(url)
        .send_form(form.iter().copied())
        .map_err(map_error)?;
    read(response)
}

fn read(mut response: ureq::http::Response<ureq::Body>) -> Result<HttpResponse, RemoteError> {
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(map_error)?;
    Ok(HttpResponse { status, body })
}

/// ureq errors → `Tls` for certificate/handshake problems, `Network` for the
/// rest of the transport, `Other` for malformed URLs.
pub(crate) fn map_error(err: ureq::Error) -> RemoteError {
    let message = err.to_string();
    let kind = match err {
        ureq::Error::Io(io) => return crate::tls::io_error(io),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => {
            RemoteErrorKind::Tls
        }
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => RemoteErrorKind::Other,
        _ => RemoteErrorKind::Network,
    };
    RemoteError::new(kind, message)
}

/// `application/x-www-form-urlencoded` component encoding (RFC 3986
/// unreserved characters pass through, everything else is `%XX`).
pub(crate) fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Decodes `%XX` and `+` (space); invalid escapes are kept literally.
pub(crate) fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `a=1&b=x%20y` into decoded pairs (empty segments skipped).
pub(crate) fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (decode_component(k), decode_component(v)),
            None => (decode_component(part), String::new()),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod test_server {
    //! One-thread HTTP/1.1 server answering each request through a closure.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// A request as seen by the fake server.
    #[derive(Clone, Debug)]
    pub struct Request {
        pub method: String,
        pub target: String,
        pub body: String,
    }

    /// Starts the server; returns its base URL and the log of requests.
    pub fn serve(
        handler: impl Fn(&Request) -> (u16, String) + Send + 'static,
    ) -> (String, Arc<Mutex<Vec<Request>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let mut length = 0usize;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let request = Request {
                    method,
                    target,
                    body: String::from_utf8_lossy(&body).into_owned(),
                };
                let (status, text) = handler(&request);
                seen.lock().unwrap().push(request);
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (base, log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_decode_round_trip() {
        let raw = "a b&c=d/é~";
        let encoded = encode_component(raw);
        assert_eq!(encoded, "a%20b%26c%3Dd%2F%C3%A9~");
        assert_eq!(decode_component(&encoded), raw);
        assert_eq!(decode_component("a+b"), "a b");
        assert_eq!(decode_component("bad%zz%4"), "bad%zz%4");
    }

    #[test]
    fn parse_query_decodes_pairs() {
        assert_eq!(
            parse_query("code=4%2Fabc&state=x+y&flag&"),
            vec![
                ("code".to_string(), "4/abc".to_string()),
                ("state".to_string(), "x y".to_string()),
                ("flag".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn get_and_post_against_a_local_server() {
        let (base, log) = test_server::serve(|req| {
            if req.method == "POST" {
                (400, format!("echo:{}", req.body))
            } else {
                (200, format!("path:{}", req.target))
            }
        });
        let timeout = Duration::from_secs(5);
        let got = get(&format!("{base}/x?y=1"), timeout).unwrap();
        assert_eq!(got.status, 200);
        assert_eq!(got.body, "path:/x?y=1");
        let posted = post_form(&format!("{base}/t"), &[("a", "1 2"), ("b", "&")], timeout).unwrap();
        assert_eq!(posted.status, 400);
        assert_eq!(posted.body, "echo:a=1+2&b=%26");
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[test]
    fn transport_errors_are_mapped() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = get(&format!("http://127.0.0.1:{port}/"), Duration::from_secs(2)).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Network, "{err}");
        let err = get("not a url", Duration::from_secs(2)).unwrap_err();
        assert_ne!(err.kind, RemoteErrorKind::Tls);
        assert_eq!(
            map_error(ureq::Error::Tls("bad cert")).kind,
            RemoteErrorKind::Tls
        );
        assert_eq!(
            map_error(ureq::Error::BadUri("x".into())).kind,
            RemoteErrorKind::Other
        );
    }
}
