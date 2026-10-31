//! OAuth 2.0 for desktop apps (RFC 8252): authorization code + PKCE (S256)
//! through a loopback redirect, token refresh, and the SASL XOAUTH2 payload.
//!
//! Hand-written over ureq (no `oauth2` crate). The Google client id/secret
//! are not in the source: [`google_client_from_env`] reads them at runtime or
//! from the build environment. For installed apps Google's "secret" is not
//! confidential; PKCE is what protects the code exchange.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use mail_core::{CancelToken, RemoteError, RemoteErrorKind, RemoteResult};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::http;

/// Timeout of each token-endpoint request.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// One OAuth authorization server + registered client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthProvider {
    /// Stable id stored in `AuthKind::OAuth2 { provider }` (e.g. `"google"`).
    pub id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub client_id: String,
    pub client_secret: Option<String>,
    /// Extra authorization-request parameters (Google: offline access).
    pub extra_params: Vec<(String, String)>,
}

/// Result of a completed sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds when `access_token` expires.
    pub expires_at: i64,
    /// From the OpenID `id_token`, when the provider returned one.
    pub email: Option<String>,
}

/// A refreshed access token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessToken {
    pub token: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// Set when the provider rotated the refresh token.
    pub refresh_token: Option<String>,
}

/// Google's endpoints with the full-mail scope plus `openid email` (to learn
/// the address), requesting a refresh token every time (`prompt=consent`).
pub fn google_provider(client_id: String, client_secret: Option<String>) -> OAuthProvider {
    OAuthProvider {
        id: "google".into(),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        scopes: ["https://mail.google.com/", "openid", "email"]
            .map(String::from)
            .to_vec(),
        client_id,
        client_secret,
        extra_params: vec![
            ("access_type".into(), "offline".into()),
            ("prompt".into(), "consent".into()),
        ],
    }
}

/// Google provider from `BGMAIL_GOOGLE_CLIENT_ID` / `BGMAIL_GOOGLE_CLIENT_SECRET`
/// (runtime first, then the values baked in at build time); `None` when no
/// client id is configured, so the UI can explain why Google sign-in is off.
pub fn google_client_from_env() -> Option<OAuthProvider> {
    google_client_from(
        std::env::var("BGMAIL_GOOGLE_CLIENT_ID").ok(),
        std::env::var("BGMAIL_GOOGLE_CLIENT_SECRET").ok(),
        option_env!("BGMAIL_GOOGLE_CLIENT_ID"),
        option_env!("BGMAIL_GOOGLE_CLIENT_SECRET"),
    )
}

/// Pure part of [`google_client_from_env`]: runtime values win over build
/// values; blank strings count as absent.
fn google_client_from(
    runtime_id: Option<String>,
    runtime_secret: Option<String>,
    build_id: Option<&str>,
    build_secret: Option<&str>,
) -> Option<OAuthProvider> {
    let present = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    let id = present(runtime_id).or_else(|| present(build_id.map(str::to_string)))?;
    let secret = present(runtime_secret).or_else(|| present(build_secret.map(str::to_string)));
    Some(google_provider(id, secret))
}

/// Provider for an `AuthKind::OAuth2 { provider }` id, if configured.
pub fn oauth_provider(id: &str) -> Option<OAuthProvider> {
    match id {
        "google" => google_client_from_env(),
        _ => None,
    }
}

/// SASL XOAUTH2 initial response (before base64): `user=…^Aauth=Bearer …^A^A`.
pub fn xoauth2_payload(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// Runs the interactive sign-in: binds `127.0.0.1:<random>`, calls
/// `open_browser` with the authorization URL, waits (up to `timeout`, or
/// until `cancel`) for the redirect, checks `state`, answers the browser with
/// a small "you can close this window" page, and exchanges the code (with the
/// PKCE verifier) for tokens. A denied consent is `Auth`; timeout/cancel are
/// `Cancelled`.
pub fn authorize(
    provider: &OAuthProvider,
    open_browser: &dyn Fn(&str),
    cancel: &CancelToken,
    timeout: Duration,
) -> RemoteResult<OAuthTokens> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(crate::tls::io_error)?;
    let port = listener.local_addr().map_err(crate::tls::io_error)?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = random_token(32)?;
    let state = random_token(16)?;
    let url = authorization_url(provider, &redirect_uri, &state, &pkce_challenge(&verifier));
    open_browser(&url);

    let code = wait_for_code(&listener, &state, cancel, timeout)?;
    let response = http::post_form(
        &provider.token_url,
        &token_form(
            provider,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", &redirect_uri),
                ("code_verifier", &verifier),
            ],
        ),
        HTTP_TIMEOUT,
    )?;
    let token = parse_token_response(&response)?;
    let refresh_token = token.refresh_token.ok_or_else(|| {
        RemoteError::new(
            RemoteErrorKind::Auth,
            "the provider did not return a refresh token",
        )
    })?;
    Ok(OAuthTokens {
        email: token.id_token.as_deref().and_then(email_from_id_token),
        access_token: token.access_token,
        refresh_token,
        expires_at: now() + token.expires_in.unwrap_or(3600),
    })
}

/// Exchanges a refresh token for a new access token. `invalid_grant`
/// (revoked/expired consent) is `Auth`: the user must sign in again.
pub fn refresh(provider: &OAuthProvider, refresh_token: &str) -> RemoteResult<AccessToken> {
    let response = http::post_form(
        &provider.token_url,
        &token_form(
            provider,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ],
        ),
        HTTP_TIMEOUT,
    )?;
    let token = parse_token_response(&response)?;
    Ok(AccessToken {
        token: token.access_token,
        expires_at: now() + token.expires_in.unwrap_or(3600),
        refresh_token: token.refresh_token,
    })
}

/// Current Unix time in seconds.
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `bytes` of OS randomness, base64url without padding.
fn random_token(bytes: usize) -> RemoteResult<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|err| RemoteError::new(RemoteErrorKind::Other, format!("no randomness: {err}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// PKCE S256 challenge: base64url(SHA-256(verifier)).
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The browser URL for the authorization request.
fn authorization_url(
    provider: &OAuthProvider,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let scope = provider.scopes.join(" ");
    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &provider.client_id),
        ("redirect_uri", redirect_uri),
        ("scope", &scope),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    params.extend(
        provider
            .extra_params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str())),
    );
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                http::encode_component(k),
                http::encode_component(v)
            )
        })
        .collect();
    let separator = if provider.auth_url.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{}{separator}{}", provider.auth_url, query.join("&"))
}

/// Token request form: `fields` plus the client credentials.
fn token_form<'a>(
    provider: &'a OAuthProvider,
    fields: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    let mut form = fields.to_vec();
    form.push(("client_id", &provider.client_id));
    if let Some(secret) = &provider.client_secret {
        form.push(("client_secret", secret));
    }
    form
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Token endpoint response → tokens, or a classified error (`invalid_grant`
/// / `invalid_client` / `unauthorized_client` → Auth, other 4xx → Protocol,
/// 5xx → Network).
fn parse_token_response(response: &http::HttpResponse) -> RemoteResult<TokenResponse> {
    if (200..300).contains(&response.status) {
        return serde_json::from_str(&response.body).map_err(|err| {
            RemoteError::new(
                RemoteErrorKind::Protocol,
                format!("bad token response: {err}"),
            )
        });
    }
    let detail = serde_json::from_str::<TokenError>(&response.body).ok();
    let kind = match detail.as_ref().map(|d| d.error.as_str()) {
        Some("invalid_grant" | "invalid_client" | "unauthorized_client" | "access_denied") => {
            RemoteErrorKind::Auth
        }
        _ if response.status >= 500 => RemoteErrorKind::Network,
        _ => RemoteErrorKind::Protocol,
    };
    let message = match detail {
        Some(TokenError {
            error,
            error_description: Some(description),
        }) => format!("{error}: {description}"),
        Some(TokenError { error, .. }) => error,
        None => format!("token endpoint answered HTTP {}", response.status),
    };
    Err(RemoteError::new(kind, message))
}

/// The `email` claim of an OpenID id_token. The signature is not checked:
/// the token came straight from the token endpoint over TLS.
fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("email")?.as_str().map(str::to_string)
}

/// Accepts loopback requests until one carries the redirect (`code` or
/// `error`); other paths (e.g. `/favicon.ico`) get a 404 and are ignored.
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &CancelToken,
    timeout: Duration,
) -> RemoteResult<String> {
    listener
        .set_nonblocking(true)
        .map_err(crate::tls::io_error)?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.is_cancelled() {
            return Err(RemoteError::new(
                RemoteErrorKind::Cancelled,
                "sign-in cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(RemoteError::new(
                RemoteErrorKind::Cancelled,
                "sign-in timed out",
            ));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(result) = handle_redirect(stream, state) {
                    return result;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(crate::tls::io_error(err)),
        }
    }
}

/// Reads one HTTP request; `None` when it isn't the redirect.
fn handle_redirect(stream: TcpStream, state: &str) -> Option<RemoteResult<String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    // Drain headers so the browser sees a clean response.
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header.trim().is_empty() => break,
            Ok(_) => {}
        }
    }
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let outcome = parse_redirect(target, state);
    let mut stream = stream;
    match &outcome {
        None => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
        Some(result) => {
            let page = page(result.is_ok());
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    }
    let _ = stream.flush();
    outcome
}

/// Interprets the redirect target (`/?code=…&state=…` or `/?error=…`).
fn parse_redirect(target: &str, state: &str) -> Option<RemoteResult<String>> {
    let (_, query) = target.split_once('?')?;
    let params = http::parse_query(query);
    let get = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    if let Some(error) = get("error") {
        let message = match error {
            "access_denied" => "access denied by the user".to_string(),
            other => format!("authorization failed: {other}"),
        };
        return Some(Err(RemoteError::new(RemoteErrorKind::Auth, message)));
    }
    let code = get("code")?;
    if get("state") != Some(state) {
        return Some(Err(RemoteError::new(
            RemoteErrorKind::Auth,
            "state mismatch in OAuth redirect",
        )));
    }
    Some(Ok(code.to_string()))
}

/// The page shown in the browser after the redirect: plain HTML, no external
/// resources. English only (it is served before the app regains focus).
fn page(success: bool) -> String {
    let (title, body) = if success {
        (
            "BGMail",
            "Signed in. You can close this window and return to BGMail.",
        )
    } else {
        (
            "BGMail",
            "Sign-in did not complete. You can close this window and try again in BGMail.",
        )
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head>\
         <body style=\"font-family:sans-serif;margin:3em\"><p>{body}</p></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server;
    use std::sync::{Arc, Mutex};

    fn provider(base: &str) -> OAuthProvider {
        OAuthProvider {
            id: "test".into(),
            auth_url: format!("{base}/auth"),
            token_url: format!("{base}/token"),
            scopes: vec!["mail".into(), "email".into()],
            client_id: "cid".into(),
            client_secret: Some("csecret".into()),
            extra_params: vec![("prompt".into(), "consent".into())],
        }
    }

    fn id_token(email: &str) -> String {
        let payload = URL_SAFE_NO_PAD.encode(format!("{{\"email\":\"{email}\"}}"));
        format!("e30.{payload}.sig")
    }

    /// Acts as the browser: follows the redirect with `query_for(state)`.
    fn browser(
        query_for: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> (impl Fn(&str), Arc<Mutex<Option<String>>>) {
        let page = Arc::new(Mutex::new(None));
        let seen = page.clone();
        let query_for = Arc::new(query_for);
        let open = move |url: &str| {
            let params = http::parse_query(url.split_once('?').unwrap().1);
            let get = |k: &str| params.iter().find(|(key, _)| key == k).unwrap().1.clone();
            let redirect = get("redirect_uri");
            let target = format!("{redirect}/?{}", query_for(&get("state")));
            let seen = seen.clone();
            thread::spawn(move || {
                // A stray favicon request first must not end the flow.
                let _ = http::get(&format!("{redirect}/favicon.ico"), Duration::from_secs(5));
                let response = http::get(&target, Duration::from_secs(5)).unwrap();
                *seen.lock().unwrap() = Some(response.body);
            });
        };
        (open, page)
    }

    #[test]
    fn authorize_runs_the_whole_pkce_flow() {
        let (base, log) = test_server::serve(|req| {
            assert_eq!(req.target, "/token");
            (
                200,
                format!(
                    "{{\"access_token\":\"at\",\"expires_in\":3599,\"refresh_token\":\"rt\",\"id_token\":\"{}\"}}",
                    id_token("me@gmail.com")
                ),
            )
        });
        let (open, page) = browser(|state| format!("code=4%2Fxyz&state={state}"));
        let tokens = authorize(
            &provider(&base),
            &open,
            &CancelToken::new(),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token, "rt");
        assert_eq!(tokens.email.as_deref(), Some("me@gmail.com"));
        assert!((tokens.expires_at - now() - 3599).abs() <= 2);
        let body = log.lock().unwrap()[0].body.clone();
        let form = http::parse_query(&body);
        let field = |k: &str| {
            form.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(field("grant_type").as_deref(), Some("authorization_code"));
        assert_eq!(field("code").as_deref(), Some("4/xyz"));
        assert_eq!(field("client_secret").as_deref(), Some("csecret"));
        assert_eq!(field("code_verifier").unwrap().len(), 43);
        assert!(field("redirect_uri")
            .unwrap()
            .starts_with("http://127.0.0.1:"));
        // Give the browser thread a moment to record the page.
        for _ in 0..50 {
            if page.lock().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(page
            .lock()
            .unwrap()
            .as_deref()
            .unwrap()
            .contains("close this window"));
    }

    #[test]
    fn authorize_reports_denied_consent_and_bad_state() {
        let (base, _) = test_server::serve(|_| (500, String::new()));
        let (open, _) = browser(|_| "error=access_denied".into());
        let err = authorize(
            &provider(&base),
            &open,
            &CancelToken::new(),
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
        assert!(err.message.contains("denied"));

        let (open, _) = browser(|_| "code=x&state=forged".into());
        let err = authorize(
            &provider(&base),
            &open,
            &CancelToken::new(),
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
        assert!(err.message.contains("state"));
    }

    #[test]
    fn authorize_honours_cancel_and_timeout() {
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let open = move |_: &str| {
            let trigger = trigger.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                trigger.cancel();
            });
        };
        let started = Instant::now();
        let err = authorize(
            &provider("http://127.0.0.1:1"),
            &open,
            &cancel,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));

        let err = authorize(
            &provider("http://127.0.0.1:1"),
            &|_: &str| {},
            &CancelToken::new(),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Cancelled);
        assert!(err.message.contains("timed out"));
    }

    #[test]
    fn authorize_requires_a_refresh_token() {
        let (base, _) = test_server::serve(|_| (200, "{\"access_token\":\"at\"}".into()));
        let (open, _) = browser(|state| format!("code=c&state={state}"));
        let err = authorize(
            &provider(&base),
            &open,
            &CancelToken::new(),
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
    }

    #[test]
    fn refresh_returns_new_token_or_auth_error() {
        let (base, log) = test_server::serve(|req| {
            if req.body.contains("refresh_token=good") {
                (200, "{\"access_token\":\"new\",\"expires_in\":60}".into())
            } else {
                (
                    400,
                    "{\"error\":\"invalid_grant\",\"error_description\":\"Token has been expired or revoked.\"}"
                        .into(),
                )
            }
        });
        let token = refresh(&provider(&base), "good").unwrap();
        assert_eq!(token.token, "new");
        assert_eq!(token.refresh_token, None);
        assert!((token.expires_at - now() - 60).abs() <= 2);
        assert!(log.lock().unwrap()[0]
            .body
            .contains("grant_type=refresh_token"));
        let err = refresh(&provider(&base), "bad").unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
        assert!(err.message.contains("revoked"));
    }

    #[test]
    fn token_errors_are_classified() {
        let response = |status, body: &str| http::HttpResponse {
            status,
            body: body.into(),
        };
        let kind = |status, body| {
            parse_token_response(&response(status, body))
                .unwrap_err()
                .kind
        };
        assert_eq!(kind(503, ""), RemoteErrorKind::Network);
        assert_eq!(
            kind(400, "{\"error\":\"invalid_request\"}"),
            RemoteErrorKind::Protocol
        );
        assert_eq!(
            kind(401, "{\"error\":\"invalid_client\"}"),
            RemoteErrorKind::Auth
        );
        assert_eq!(kind(200, "not json"), RemoteErrorKind::Protocol);
        assert!(parse_token_response(&response(200, "{\"access_token\":\"a\"}")).is_ok());
    }

    #[test]
    fn google_provider_shape() {
        let google = google_provider("id".into(), None);
        assert_eq!(google.id, "google");
        assert!(google
            .scopes
            .contains(&"https://mail.google.com/".to_string()));
        assert!(google
            .extra_params
            .contains(&("access_type".into(), "offline".into())));
        let url = authorization_url(&google, "http://127.0.0.1:5", "st", "ch");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?response_type=code"));
        assert!(url.contains("scope=https%3A%2F%2Fmail.google.com%2F%20openid%20email"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("prompt=consent"));
        let mut odd = google.clone();
        odd.auth_url = "https://x/auth?tenant=1".into();
        assert!(authorization_url(&odd, "r", "s", "c").starts_with("https://x/auth?tenant=1&"));
    }

    #[test]
    fn google_client_env_resolution() {
        assert_eq!(google_client_from(None, None, None, None), None);
        assert_eq!(google_client_from(Some(" ".into()), None, None, None), None);
        let p = google_client_from(Some("rt".into()), None, Some("bt"), Some("bs")).unwrap();
        assert_eq!(
            (p.client_id.as_str(), p.client_secret.as_deref()),
            ("rt", Some("bs"))
        );
        let p = google_client_from(None, Some("rs".into()), Some("bt"), Some("bs")).unwrap();
        assert_eq!(
            (p.client_id.as_str(), p.client_secret.as_deref()),
            ("bt", Some("rs"))
        );
        // Real env: whatever it is, it must agree with the pure resolver.
        assert_eq!(
            google_client_from_env(),
            google_client_from(
                std::env::var("BGMAIL_GOOGLE_CLIENT_ID").ok(),
                std::env::var("BGMAIL_GOOGLE_CLIENT_SECRET").ok(),
                option_env!("BGMAIL_GOOGLE_CLIENT_ID"),
                option_env!("BGMAIL_GOOGLE_CLIENT_SECRET"),
            )
        );
        assert_eq!(oauth_provider("google"), google_client_from_env());
        assert_eq!(oauth_provider("microsoft"), None);
    }

    #[test]
    fn xoauth2_payload_format() {
        assert_eq!(
            xoauth2_payload("me@x.com", "tok"),
            "user=me@x.com\x01auth=Bearer tok\x01\x01"
        );
    }

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636 Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let a = random_token(32).unwrap();
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_token(32).unwrap());
    }

    #[test]
    fn id_token_email_extraction() {
        assert_eq!(
            email_from_id_token(&id_token("a@b.c")).as_deref(),
            Some("a@b.c")
        );
        assert_eq!(email_from_id_token("nodots"), None);
        assert_eq!(email_from_id_token("a.!!!.c"), None);
        let no_email = format!("x.{}.y", URL_SAFE_NO_PAD.encode("{\"sub\":\"1\"}"));
        assert_eq!(email_from_id_token(&no_email), None);
    }

    #[test]
    fn redirect_parsing() {
        assert!(parse_redirect("/favicon.ico", "s").is_none());
        assert!(parse_redirect("/?foo=bar", "s").is_none());
        assert_eq!(
            parse_redirect("/?code=a%2Fb&state=s", "s"),
            Some(Ok("a/b".into()))
        );
        assert_eq!(
            parse_redirect("/?error=server_error", "s")
                .unwrap()
                .unwrap_err()
                .kind,
            RemoteErrorKind::Auth
        );
        assert!(page(true).contains("close this window"));
        assert!(page(false).contains("try again"));
        assert!(!page(true).contains("http"));
    }

    #[test]
    fn token_form_adds_client_credentials() {
        let mut p = provider("http://x");
        let form = token_form(&p, &[("grant_type", "refresh_token")]);
        assert!(form.contains(&("client_id", "cid")));
        assert!(form.contains(&("client_secret", "csecret")));
        p.client_secret = None;
        assert_eq!(token_form(&p, &[]).len(), 1);
        assert!(now() > 1_700_000_000);
    }
}
