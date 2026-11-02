//! Resolves sub-resources (images, stylesheets) a mail body references.
//!
//! Mail is rendered offline: `data:` URLs and `cid:` attachments are served
//! from memory, `file:` only when explicitly allowed (tests/tools), and
//! remote URLs (also protocol-relative `//host/…` ones) are fetched only when
//! the user allowed remote content ([`crate::remote`]); otherwise they are
//! blocked and recorded.

use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// What happened to a requested resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Served(Vec<u8>),
    Blocked,
    Missing,
    /// Fetched from the network (remote content allowed).
    Remote,
}

/// In-memory resource source for one document.
#[derive(Default)]
pub struct Resources {
    /// Attachments addressed as `cid:<content-id>` (without angle brackets).
    attachments: HashMap<String, Arc<[u8]>>,
    allow_file_urls: bool,
    allow_remote: bool,
    blocked: Mutex<Vec<String>>,
    /// Remote loads of the document using these resources.
    loads: Arc<crate::remote::RemoteLoads>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an inline attachment by its Content-ID.
    pub fn with_attachment(
        mut self,
        content_id: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Self {
        let id = content_id.into();
        let id = id.trim_start_matches('<').trim_end_matches('>').to_string();
        self.attachments.insert(id, bytes.into());
        self
    }

    /// Lets `file:` URLs load from disk. Off by default: a mail body must never
    /// read local files.
    pub fn allow_file_urls(mut self, allow: bool) -> Self {
        self.allow_file_urls = allow;
        self
    }

    /// Fetches `http:`/`https:` resources from the network instead of
    /// blocking them (the user chose to show remote content).
    pub fn allow_remote(mut self, allow: bool) -> Self {
        self.allow_remote = allow;
        self
    }

    /// The document's remote loads (in flight, arrived).
    pub fn remote_loads(&self) -> &Arc<crate::remote::RemoteLoads> {
        &self.loads
    }

    /// Remote URLs that were requested and blocked, in request order.
    pub fn blocked_urls(&self) -> Vec<String> {
        self.blocked
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    /// Whether [`Self::resolve`] would serve `url` (without recording it as
    /// blocked): decodable `data:`, known `cid:` attachments, and `file:`
    /// when allowed.
    pub fn is_available(&self, url: &url::Url) -> bool {
        if let Some(web) = protocol_relative_web_url(url) {
            return self.is_available(&web);
        }
        match url.scheme() {
            "data" => data_url::DataUrl::process(url.as_str())
                .is_ok_and(|data| data.decode_to_vec().is_ok()),
            "cid" => self.attachments.contains_key(&percent_decode(url.path())),
            "file" => self.allow_file_urls,
            "http" | "https" => self.allow_remote,
            _ => false,
        }
    }

    /// Resolves a URL according to the offline policy.
    pub fn resolve(&self, url: &url::Url) -> Resolution {
        if let Some(web) = protocol_relative_web_url(url) {
            return self.resolve(&web);
        }
        match url.scheme() {
            "data" => match data_url::DataUrl::process(url.as_str()) {
                Ok(data) => match data.decode_to_vec() {
                    Ok((bytes, _)) => Resolution::Served(bytes),
                    Err(_) => Resolution::Missing,
                },
                Err(_) => Resolution::Missing,
            },
            "cid" => {
                // `cid:` URLs may be percent-encoded (RFC 2392).
                let id = percent_decode(url.path());
                match self.attachments.get(&id) {
                    Some(bytes) => Resolution::Served(bytes.to_vec()),
                    None => Resolution::Missing,
                }
            }
            "file" if self.allow_file_urls => {
                match url.to_file_path().ok().and_then(|p| std::fs::read(p).ok()) {
                    Some(bytes) => Resolution::Served(bytes),
                    None => Resolution::Missing,
                }
            }
            "file" => Resolution::Blocked,
            // Relative references inside the message (see `MESSAGE_BASE_URL`):
            // nothing to fetch, and not remote content either.
            "bgmail" => Resolution::Missing,
            "http" | "https" if self.allow_remote => Resolution::Remote,
            _ => {
                if let Ok(mut blocked) = self.blocked.lock() {
                    blocked.push(url.to_string());
                }
                Resolution::Blocked
            }
        }
    }
}

/// The web URL a protocol-relative reference (`//host/path`, common in mail)
/// stands for. Resolved against the message base URL it comes out as
/// `bgmail://host/path`; mail clients load it over HTTPS. `None` for any
/// other URL, including real message-relative ones (`bgmail://message/…`).
pub(crate) fn protocol_relative_web_url(url: &url::Url) -> Option<url::Url> {
    // `message` is the host of `MESSAGE_BASE_URL` (checked by the tests).
    if url.scheme() != "bgmail" || url.host_str()? == "message" {
        return None;
    }
    url::Url::parse(&format!(
        "https://{}",
        &url[url::Position::BeforeUsername..]
    ))
    .ok()
}

/// Minimal `%XX` decoding for Content-IDs.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Adapter that serves [`Resources`] to Blitz synchronously.
pub(crate) struct ResourceProvider(pub Arc<Resources>);

impl NetProvider for ResourceProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let target = protocol_relative_web_url(&request.url).unwrap_or(request.url);
        let url = target.to_string();
        match self.0.resolve(&target) {
            Resolution::Served(bytes) => handler.bytes(url, Bytes::from(bytes)),
            Resolution::Blocked => handler.error(url, "blocked by offline policy".into()),
            Resolution::Missing => handler.error(url, "not found".into()),
            Resolution::Remote => crate::remote::fetch(target, handler, self.0.loads.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_are_fetched_only_when_allowed() {
        let blocked = Resources::new();
        assert_eq!(
            blocked.resolve(&url("https://cdn.test/a.png")),
            Resolution::Blocked
        );
        assert!(!blocked.is_available(&url("https://cdn.test/a.png")));
        let allowed = Resources::new().allow_remote(true);
        assert_eq!(
            allowed.resolve(&url("https://cdn.test/a.png")),
            Resolution::Remote
        );
        assert_eq!(
            allowed.resolve(&url("http://cdn.test/a.png")),
            Resolution::Remote
        );
        assert!(allowed.is_available(&url("https://cdn.test/a.png")));
        // Other schemes keep their policy; nothing is recorded as blocked.
        assert_eq!(
            allowed.resolve(&url("ftp://cdn.test/a")),
            Resolution::Blocked
        );
        assert_eq!(allowed.blocked_urls(), ["ftp://cdn.test/a"]);
        assert!(!allowed.remote_loads().pending());
    }

    fn url(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn serves_data_urls() {
        let r = Resources::new();
        assert_eq!(
            r.resolve(&url("data:text/plain;base64,aGk=")),
            Resolution::Served(b"hi".to_vec())
        );
    }

    #[test]
    fn serves_cid_attachments_with_or_without_brackets() {
        let r = Resources::new().with_attachment("<logo@x>", b"png".to_vec());
        assert_eq!(
            r.resolve(&url("cid:logo@x")),
            Resolution::Served(b"png".to_vec())
        );
        assert_eq!(r.resolve(&url("cid:other")), Resolution::Missing);
    }

    #[test]
    fn decodes_percent_encoded_cid() {
        let r = Resources::new().with_attachment("a b", b"x".to_vec());
        assert_eq!(
            r.resolve(&url("cid:a%20b")),
            Resolution::Served(b"x".to_vec())
        );
    }

    #[test]
    fn blocks_and_records_remote_urls() {
        let r = Resources::new();
        assert_eq!(
            r.resolve(&url("https://tracker.example/p.gif")),
            Resolution::Blocked
        );
        assert_eq!(
            r.blocked_urls(),
            vec!["https://tracker.example/p.gif".to_string()]
        );
    }

    #[test]
    fn file_urls_need_opt_in() {
        let path = std::env::temp_dir().join("html_view_resources_test.txt");
        std::fs::write(&path, b"ok").unwrap();
        let file_url = url::Url::from_file_path(&path).unwrap();
        assert_eq!(Resources::new().resolve(&file_url), Resolution::Blocked);
        assert_eq!(
            Resources::new().allow_file_urls(true).resolve(&file_url),
            Resolution::Served(b"ok".to_vec())
        );
        // Blocked file URLs are not "remote content" for the UI.
        assert!(Resources::new().blocked_urls().is_empty());
    }

    #[test]
    fn message_relative_urls_are_missing_not_blocked() {
        let r = Resources::new();
        assert_eq!(
            r.resolve(&url("bgmail://message/assets/x.png")),
            Resolution::Missing
        );
        assert!(r.blocked_urls().is_empty());
    }

    #[test]
    fn availability_matches_the_policy_without_recording() {
        let r = Resources::new().with_attachment("logo", b"png".to_vec());
        assert!(r.is_available(&url("data:image/png;base64,aGk=")));
        assert!(r.is_available(&url("cid:logo")));
        assert!(!r.is_available(&url("cid:other")));
        assert!(!r.is_available(&url("https://tracker.example/p.gif")));
        assert!(!r.is_available(&url("file:///tmp/x.png")));
        assert!(r.blocked_urls().is_empty());
    }

    #[test]
    fn protocol_relative_urls_are_web_urls() {
        let base = url(crate::document::MESSAGE_BASE_URL);
        let joined = base.join("//cdn.test/a.png?x=1").unwrap();
        assert_eq!(
            protocol_relative_web_url(&joined).map(|u| u.to_string()),
            Some("https://cdn.test/a.png?x=1".to_string())
        );
        // Message-relative and other URLs are left alone.
        assert_eq!(
            protocol_relative_web_url(&base.join("a.png").unwrap()),
            None
        );
        assert_eq!(protocol_relative_web_url(&url("https://cdn.test/a")), None);
        // They follow the remote policy.
        let allowed = Resources::new().allow_remote(true);
        assert_eq!(allowed.resolve(&joined), Resolution::Remote);
        assert!(allowed.is_available(&joined));
        let blocked = Resources::new();
        assert!(!blocked.is_available(&joined));
        assert_eq!(blocked.resolve(&joined), Resolution::Blocked);
        assert_eq!(blocked.blocked_urls(), ["https://cdn.test/a.png?x=1"]);
    }

    #[test]
    fn percent_decode_keeps_invalid_sequences() {
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%41b"), "Ab");
    }
}
