# crates/protocols/src/ — Source API

## Purpose

Source of the `protocols` crate: TLS plumbing, IMAP client, SMTP sender,
OAuth, account connector, presets and autoconfig, plus a private HTTP helper.

## Ownership

- Owns: all modules under `crates/protocols/src/`.
- Does not own: dependency policy, error-kind policy and test strategy
  (parent `.dox/crates/protocols/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with
  purpose and behavior. Update the matching entry when changing signatures or
  semantics. Visibility (`pub` / `pub(crate)` / `private`) reflects the source.
- Test-only helpers (`#[cfg(test)]` modules `tls::test_support`,
  `http::test_server`, `imap_remote::test_server`, `smtp::test_server`) are
  listed because other modules' tests depend on their behavior.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Shared helpers: `tls::io_error` maps every transport error (also used by
  `http` and `oauth`); `http::{encode_component, parse_query}` serve OAuth and
  autoconfig; `presets::{domain, local_part}` serve autoconfig.

## Verification

- `cargo test -p protocols`
- `cargo clippy -p protocols --all-targets -- -D warnings`

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

### `src/lib.rs`

- `#![forbid(unsafe_code)]`; declares `autoconfig`, `connector`, `http`
  (private), `imap_remote`, `oauth`, `presets`, `smtp`, `tls`, and re-exports
  the public entry points (`ImapRemote`, `SmtpSender`, `ImapSmtpConnector`,
  `Credential`, `Secret`, `test_account`, `ConnectOptions`, OAuth functions and
  types, presets, autoconfig functions). No functions.

### `src/tls.rs`

#### Types / constants

- **const `CONNECT_TIMEOUT`** (pub): 20 s per resolved address.
- **const `IO_TIMEOUT`** (pub): 60 s per blocking read/write.
- **`ConnectOptions::for_host`** (pub): the options for a host: a literal loopback address skips validation.
- **struct `ConnectOptions { accept_invalid_certs: bool }`** (pub): Copy,
  Default (verifying). `accept_invalid_certs` = local test servers only.
- **type `TlsStream`** (pub): `rustls::StreamOwned<ClientConnection, TcpStream>`.
- **struct `AcceptAnyCert { provider }`** (private): `ServerCertVerifier`
  that accepts any chain/name but still verifies handshake signatures.

#### Functions / methods

##### Context: module

- **`crypto_provider`** (pub) — `pub fn crypto_provider() -> Arc<CryptoProvider>`
  - Purpose: the one ring provider used everywhere.
  - Behavior: `OnceLock`-cached `rustls::crypto::ring::default_provider()`; never touches rustls' process default.
- **`client_config`** (pub) — `pub fn client_config(options: ConnectOptions) -> RemoteResult<Arc<ClientConfig>>`
  - Purpose: cached client config.
  - Behavior: two `OnceLock`s: insecure (`AcceptAnyCert`) when `accept_invalid_certs`, else `rustls_platform_verifier::Verifier` (OS trust store); safe default protocol versions; init failure cached and returned as `Tls`.
- **`connect_tcp`** (pub) — `pub fn connect_tcp(host: &str, port: u16) -> RemoteResult<TcpStream>`
  - Purpose: resolve + connect with timeouts.
  - Behavior: tries each resolved address with `CONNECT_TIMEOUT`; sets read/write timeouts to `IO_TIMEOUT` and `TCP_NODELAY`; resolve/connect failures → `Network` (last error reported).
- **`connect_tls`** (pub) — `pub fn connect_tls(tcp: TcpStream, host: &str, options: ConnectOptions) -> RemoteResult<TlsStream>`
  - Purpose: wrap a socket in TLS.
  - Behavior: SNI/verification name from `host` (IP literals allowed); drives `complete_io` until the handshake finishes so certificate errors surface here (`Tls` via `io_error`).
- **`read_line_unbuffered`** (pub) — `pub fn read_line_unbuffered(stream: &mut impl Read) -> io::Result<String>`
  - Purpose: read pre-TLS lines without over-reading.
  - Behavior: one byte at a time until CRLF (included); EOF → `UnexpectedEof`; > 8192 bytes → `InvalidData`; lossy UTF-8.
- **`imap_starttls`** (pub) — `pub fn imap_starttls(stream: &mut TcpStream) -> RemoteResult<()>`
  - Purpose: IMAP STARTTLS negotiation before `connect_tls`.
  - Behavior: greeting must start with `* OK` (else `Protocol`); sends `s0 STARTTLS`; skips untagged lines; tagged `OK` → Ok, anything else → `Tls` "STARTTLS refused".
- **`io_error`** (pub) — `pub fn io_error(err: io::Error) -> RemoteError`
  - Purpose: transport error classification.
  - Behavior: inner `rustls::Error` → `Tls`; otherwise `Network`, prefixed "timed out:" for `TimedOut`/`WouldBlock`.
- **`network`** (private) — `fn network(message: String) -> RemoteError`: `Network` error constructor.
- **`shutdown`** (pub) — `pub fn shutdown(socket: &TcpStream)`
  - Purpose: cancellation hook body.
  - Behavior: `shutdown(Both)`, errors ignored; unblocks readers of any clone.

##### Context: `AcceptAnyCert` (impl `ServerCertVerifier`)

- **`verify_server_cert`** — always `ServerCertVerified::assertion()`.
- **`verify_tls12_signature`** / **`verify_tls13_signature`** — delegate to `rustls::crypto::verify_tls1{2,3}_signature` with the provider's algorithms.
- **`supported_verify_schemes`** — the provider's supported schemes.

##### Context: `test_support` (`#[cfg(test)]`, pub(crate))

- **`server_config`** — rustls `ServerConfig` with `tests/fixtures/test-cert.pem` / `test-key.pem`.
- **`accept_tls`** — server-side handshake on an accepted socket; returns the server stream.

### `src/http.rs` (private module)

#### Types

- **struct `HttpResponse { status: u16, body: String }`** (pub(crate)).

#### Functions

- **`agent`** (pub(crate)) — `fn agent(timeout: Duration) -> ureq::Agent`
  - Behavior: global timeout, non-2xx returned as data (`http_status_as_error(false)`), `BGMail/<version>` user agent, `RootCerts::PlatformVerifier`.
- **`get`** (pub(crate)) — `fn get(url: &str, timeout: Duration) -> Result<HttpResponse, RemoteError>`: GET and read the body as text.
- **`post_form`** (pub(crate)) — `fn post_form(url: &str, form: &[(&str, &str)], timeout: Duration) -> Result<HttpResponse, RemoteError>`: urlencoded POST, body as text.
- **`read`** (private) — status + `read_to_string` of a ureq response.
- **`map_error`** (pub(crate)) — `fn map_error(err: ureq::Error) -> RemoteError`
  - Behavior: `Io` → `tls::io_error`; `Tls`/`Rustls`/`TlsRequired` → `Tls`; `BadUri`/`Http` → `Other`; rest → `Network`.
- **`encode_component`** (pub(crate)) — percent-encodes everything except RFC 3986 unreserved characters (space → `%20`).
- **`decode_component`** (pub(crate)) — decodes `%XX` and `+`→space; invalid/truncated escapes kept literally; lossy UTF-8.
- **`parse_query`** (pub(crate)) — `a=1&b` → decoded pairs (missing `=` → empty value; empty segments skipped).

##### Context: `test_server` (`#[cfg(test)]`, pub(crate))

- **struct `Request { method, target, body }`**.
- **`serve`** — `fn serve(handler: impl Fn(&Request) -> (u16, String) + Send + 'static) -> (String, Arc<Mutex<Vec<Request>>>)`: one-thread HTTP/1.1 server on `127.0.0.1:0`; reads request line, headers, `Content-Length` body; answers `handler`'s status/body with `Connection: close`; returns base URL and request log.

### `build.rs` + `src/dotenv.rs`

- **`main`** (`build.rs`): for each Google key without a real environment variable, emits `cargo:rustc-env` from `../../.env`, which it creates empty when missing so cargo can always watch it; an uncreatable file (read-only checkout) skips both the values and the watch.
- **`read_or_create`** (`dotenv.rs`) — `pub fn read_or_create(path: &Path) -> io::Result<String>`: contents of `path`, creating an empty file if missing; an existing file keeps its contents and mtime.
- **`lookup`** (`dotenv.rs`, shared via `#[path]`, a test-only module in the lib) — `pub fn lookup(text: &str, key: &str) -> Option<String>`: `KEY=value`, `export`, quotes; comments and blanks skipped; last assignment wins.
- **`unquote`** (private) — strips one pair of matching `"` or `'`.
- Tests: `read_or_create_creates_missing_and_keeps_existing`, `lookup_reads_plain_exported_and_quoted_values`, `lookup_skips_missing_and_blank_values`.

### `src/imap_remote.rs`

- Windows: `tls::shutdown` doesn't wake a blocked local read; it returns when the peer closes in response, or at the read timeout. Cancelling is bounded there, not instant; `Engine::stop_account` still returns after `STOP_TIMEOUT`, detaching the worker.

- **`Debug::fmt`** — hides the session.

#### Types / constants

- **const `HEADER_CHUNK`** (private): 100 UIDs per header fetch.
- **const `COMMAND_CHUNK`** (private): 500 UIDs per flag fetch/STORE/COPY/MOVE/EXPUNGE.
- **enum `ImapStream { Plain(TcpStream), Tls(Box<TlsStream>) }`** (pub): session transport; implements `Read`, `Write`, `imap::extensions::idle::SetReadTimeout`.
- **struct `XOAuth2 { payload }`** (private): `imap::Authenticator` for XOAUTH2.
- **struct `ImapRemote`** (pub): `session`, `capabilities`, `selected` (cached SELECT), `socket` (clone for shutdown), `cancel` (connector token), `_cancel_hook` (guard). `Debug` shows capabilities and selection only.

#### Functions / methods

##### Context: `ImapStream`

- **`tcp`** (private) — the underlying `TcpStream` of either variant.
- **`read` / `write` / `flush`** — delegate to the plain socket or the TLS stream.
- **`set_read_timeout`** (`SetReadTimeout`) — `Some(t)` → `max(t, 10 ms)`; `None` → `IO_TIMEOUT` (IDLE resets with `None`; we never leave reads unbounded).

##### Context: `XOAuth2`

- **`process`** — empty challenge → bearer payload; non-empty (server's JSON error) → empty response so the server sends the tagged `NO`.

##### Context: `ImapRemote`

- **`connect`** (pub) — `pub fn connect(server: &ServerConfig, username: &str, secret: &Secret, options: ConnectOptions, cancel: &CancelToken) -> RemoteResult<Self>`
  - Purpose: open an authenticated session.
  - Behavior: already-cancelled token → `Cancelled`; `connect_tcp`; registers `cancel` hook shutting down a socket clone; TLS / STARTTLS (`imap_starttls` then `connect_tls`, greeting already consumed) / plain; greeting must be `* OK`/`* PREAUTH` (else `Network` with server text); `Password` → `LOGIN`, `AccessToken` → `AUTHENTICATE XOAUTH2`; login failures via `map_login_error`; reads `CAPABILITY` into `Capabilities`; any failure after a cancel → `Cancelled`.
- **`fail`** (private) — `map_error`, but `Cancelled` once the connector token fired.
- **`run`** (private) — `fn run(&mut self, command: &str) -> RemoteResult<Vec<u8>>`: checks cancel; `Session::run`; returns all response bytes including the tagged completion.
- **`ensure_selected`** (private) — SELECTs `folder` unless it is the cached selection.
- **`fetch_header_chunk`** (private) — one `UID FETCH <set> (<header_items>)`; parses `RemoteHeader`s from FETCH responses (skips those without UID + header).
- **`store`** (private) — `fn store(&mut self, uids: &[u32], op: char, flags: &[&str]) -> RemoteResult<()>`: no-op for empty UIDs/flags; `UID STORE <set> {+|-}FLAGS.SILENT (<flags>)` per chunk.
- **`expunge`** (private) — removes only `uids`. UIDPLUS → `UID EXPUNGE <set>` per chunk. Without it, other `\Deleted` messages are found (`UID SEARCH DELETED`), unflagged around a plain `EXPUNGE` and re-flagged even when it fails. Failures only lose their `\Deleted`; a message another client flags between SEARCH and EXPUNGE is still removed (IMAP has no atomic alternative).
- **`search_uids`** (private) — runs a `UID SEARCH …`; matching UIDs ascending, deduplicated.

##### Context: `impl MailRemote for ImapRemote`

- **`capabilities`** — capabilities read at connect.
- **`list_folders`** — `LIST "" "*"` → `RemoteFolder::new(raw name, delimiter, attributes as atoms)`.
- **`select`** — clears the cache; `SELECT "<quoted>"` (+ ` (CONDSTORE)` when announced); caches the folder; `FolderStatus` from `folder_status`. `NO` → `NotFound`/`Protocol` per `no_kind`.
- **`uids`** — ensure selected; `UID SEARCH ALL`; sorted, deduplicated.
- **`find_uid_by_message_id`** — blank → `None`; selects; `UID SEARCH HEADER Message-ID "<id>"` with angle brackets (HEADER is a substring match); the highest UID.
- **`fetch_headers`** — empty input → empty; ensure selected; chunks of `HEADER_CHUNK`; keeps only requested UIDs (unknown/expunged skipped); sorted by UID, deduplicated.
- **`fetch_flags`** — empty → empty; `UID FETCH <set> (UID FLAGS[ MODSEQ])` with ` (CHANGEDSINCE n)` only under CONDSTORE (otherwise `changed_since` is ignored and all requested UIDs return); requested UIDs only; sorted, deduplicated.
- **`fetch_body`** — `UID FETCH <uid> (UID BODY.PEEK[])` (never sets `\Seen`); missing → `NotFound`.
- **`store_flags`** — empty UIDs → Ok; `+FLAGS.SILENT` for `add.to_imap()`, then `-FLAGS.SILENT` for `remove.to_imap()`; empty sides skipped.
- **`move_messages`** — empty → `Some([])`; MOVE → `UID MOVE <set> "<to>"`; else `UID COPY` then `\Deleted` + `expunge`. Returns new UIDs in input order from `COPYUID` codes; `None` when any chunk lacked COPYUID or a UID is unmapped. Missing target → `NotFound` (`[TRYCREATE]`).
- **`delete_messages`** — empty → Ok; `+FLAGS.SILENT (\Deleted)` + `expunge`.
- **`append`** — cancel check; name pre-escaped with `escape_quoted` (the `imap` crate quotes verbatim); flags from `MessageFlags::to_imap`; crate `append(..).flags(..).finish()`.
- **`create_folder`** — `CREATE "<quoted>"`; existing folder → `Protocol`.
- **`noop`** — `NOOP`.
- **`logout`** — `LOGOUT` unless cancelled (errors ignored); clears selection; shuts the socket down.
- **`idle`** — no IDLE capability → `Unsupported`; already cancelled (idle or session token) → `Stopped`; ensure selected; registers a shutdown hook on the idle `cancel`; IDLE with `timeout`, keepalive off, stop on any untagged response; dropping the handle sends `DONE`. After a cancel → `Stopped` (session dead, selection cleared); `MailboxChanged` → `Changed`; `TimedOut` → `Timeout`; errors → `fail`.

##### Context: module

- **`check_cancel`** (private) — `Cancelled` error when the token fired.
- **`parse_capabilities`** (pub) — `pub fn parse_capabilities<'a>(atoms: impl IntoIterator<Item = &'a str>) -> Capabilities`: case-insensitive IDLE, MOVE, UIDPLUS, CONDSTORE (or QRESYNC), X-GM-EXT-1, SPECIAL-USE.
- **`uid_set`** (pub) — `pub fn uid_set(uids: &[u32]) -> String`: sorts/dedups and compresses runs (`1:3,5,9:10`); empty → `""`; safe at `u32::MAX`.
- **`uid_chunks`** (pub) — `pub fn uid_chunks(uids: &[u32], size: usize) -> Vec<String>`: sorted/deduped UIDs split into `size` (min 1) per set.
- **`escape_quoted`** (private) — escapes `\` and `"`; CR/LF/NUL → `Other`.
- **`quote`** (pub) — `pub fn quote(value: &str) -> RemoteResult<String>`: `"` + `escape_quoted` + `"`.
- **`name_attribute`** (private) — `NameAttribute` → atom (`\Noselect`, `\Sent`, …; extensions verbatim, e.g. `\Important`).
- **`parse_responses`** (private) — parses all responses in the bytes with `imap_proto::parser::parse_response`; unparseable line skipped (to next CRLF).
- **`folder_status`** (private) — `EXISTS`, `[UIDVALIDITY]`, `[UIDNEXT]`, `[HIGHESTMODSEQ]` from untagged or tagged codes.
- **`header_items`** (private) — `UID FLAGS RFC822.SIZE BODY.PEEK[HEADER] BODYSTRUCTURE` + ` X-GM-MSGID X-GM-THRID` (gmail) + ` MODSEQ` (condstore).
- **`fetch_flags`** (private) — `FLAGS` attribute → `MessageFlags`.
- **`fetch_uid`** (private) — `UID` attribute.
- **`header_from_attributes`** (private) — `RemoteHeader` only when UID and `BODY[HEADER]`/`RFC822.HEADER` are present; fills flags, size, Gmail ids, modseq, `has_attachments` from BODYSTRUCTURE.
- **`flags_from_attributes`** (private) — `RemoteFlags` when UID and FLAGS are present (modseq optional).
- **`body_from_attributes`** (private) — `BODY[]`/`RFC822` bytes when the UID matches.
- **`has_attachments`** (pub) — `pub fn has_attachments(structure: &BodyStructure<'_>) -> bool`: disposition `attachment` anywhere, any `message/rfc822`, or a non-text leaf that is neither `inline` nor has a Content-ID. Text parts count only with `attachment`.
- **`expand_uid_set`** (private) — `UidSetMember`s → UIDs (reversed ranges normalized).
- **`copy_uids`** (private) — all `COPYUID` source→dest pairs from untagged/tagged codes; `None` when none present; mismatched lengths ignored.
- **`transaction`** (private) — MAIL FROM (+ SMTPUTF8 / 8BITMIME when needed), RCPT TO each, DATA, the message; on failure reports the phase it failed in.
- **`classify`** (pub) — `pub fn classify(err, phase: SendPhase) -> SendError`: `Connect` phase and auth replies → transient; `4xx` → transient; other codes → permanent; local client errors → permanent; no code in `Envelope` → transient; no code in `Message` → **uncertain** (the terminator may have reached the server).
- **`map_error`** (pub) — `pub fn map_error(err: imap::Error) -> RemoteError`: `Io` → `tls::io_error`; `No` → `no_kind`; `Bad` → `Protocol`; `Bye` / `ConnectionLost` → `Network`; `Validate` → `Other`; rest (parse, append, unexpected) → `Protocol`. (The crate reports an unparseable line as a `Bye` "no explanation given".)
- **`no_kind`** (private) — `[TRYCREATE]` or text containing nonexist / not exist / doesn't exist / no such / unknown mailbox / not found → `NotFound`, else `Protocol`.
- **`map_login_error`** (pub) — `NO`/`BAD` → `Auth`, except a `NO` with an RFC 5530 passing code (`login_refusal_kind`: `[UNAVAILABLE]`, `[SERVERBUG]`, `[INUSE]`, `[LIMIT]` in the text) → `Network`, which retries; others as `map_error`.

##### Context: `test_server` (`#[cfg(test)]`, pub(crate))

- **`start`** — `start_n(greeting, steps, 1)`.
- **`start_n`** — scripted IMAP server for `connections` sequential connections; returns port + join handle (`Err` with the first mismatch).
- **`serve`** (private) — writes the greeting; per step reads a line: `=line` matches an exact untagged line (`=*` any) and reuses the previous tag, otherwise the command after the tag must start with the expectation (mismatch → `BAD` + `Err`); reply has `{tag}` substituted; then drains until hang-up.

### `src/smtp.rs`

#### Types

- **struct `SmtpSender { transport: SmtpTransport }`** (pub): `Debug` hides internals.

#### Functions / methods

##### Context: `SmtpSender`

- **`new`** (pub) — `pub fn new(server: &ServerConfig, username: &str, secret: &Secret, options: ConnectOptions) -> RemoteResult<Self>`
  - Behavior: no network. `Tls` → `Tls::Wrapper`, `StartTls` → `Tls::Required`, `None` → no TLS; rustls params with `accept_invalid_certs` applied to certificates **and** host names; port from config; `IO_TIMEOUT`; `Password` → PLAIN/LOGIN, `AccessToken` → XOAUTH2.
- **`verify`** (pub) — `pub fn verify(&self) -> RemoteResult<()>`: connect + TLS + AUTH + QUIT via `test_connection`; closed connection → `Network`; errors via `map_error`.
- **`send`** (`MailSender`) — builds `envelope(from, recipients)` and runs `transaction` on a fresh connection (no pool), bytes unchanged; failures go through `classify`.
- **`Debug::fmt`** — hides internals.

##### Context: module

- **`envelope`** (pub) — `pub fn envelope(from: &str, recipients: &[String]) -> RemoteResult<Envelope>`: trims + parses addresses; invalid address or no recipients → `Other`.
- **`map_error`** (pub) — `pub fn map_error(err: lettre::transport::smtp::Error) -> RemoteError`: reply 530/534/535 → `Auth`, 454 with "auth" text → `Auth`, other replies → `Protocol`; TLS (lettre flag or rustls in the source chain) → `Tls`; timeouts/connection/client errors → `Network`.
- **`caused_by_tls`** (private) — walks `source()` looking for `rustls::Error` (directly or inside `io::Error`).
- **`is_auth_reply`** (private) — message mentions "auth" (case-insensitive).

##### Context: `test_server` (`#[cfg(test)]`, pub(crate))

- **struct `Received { auth, from, to, data }`**.
- **`start`** — multi-connection fake SMTP: EHLO advertises `AUTH PLAIN LOGIN XOAUTH2`; AUTH payload (decoded) must contain `secret` (else 535); `RCPT` containing "reject" → 550; logs each DATA (and auth-only sessions on QUIT).
- **`session`** (private) — one connection of the above.

### `src/oauth.rs`

#### Types / constants

- **const `HTTP_TIMEOUT`** (private): 30 s per token request.
- **struct `OAuthProvider { id, auth_url, token_url, scopes, client_id, client_secret: Option<String>, extra_params: Vec<(String, String)> }`** (pub).
- **struct `OAuthTokens { access_token, refresh_token, expires_at: i64, email: Option<String> }`** (pub).
- **struct `AccessToken { token, expires_at: i64, refresh_token: Option<String> }`** (pub): `refresh_token` set when the provider rotated it.
- **structs `TokenResponse` / `TokenError`** (private): token endpoint JSON.

#### Functions

- **`google_provider`** (pub) — `pub fn google_provider(client_id: String, client_secret: Option<String>) -> OAuthProvider`: Google endpoints, scopes `https://mail.google.com/ openid email`, extra `access_type=offline`, `prompt=consent`, id `"google"`.
- **`google_client_from_env`** (pub) — `pub fn google_client_from_env() -> Option<OAuthProvider>`: `google_client_from` over runtime env vars and build-time `option_env!` (`BGMAIL_GOOGLE_CLIENT_ID` / `_SECRET`).
- **`google_client_from`** (private) — runtime value wins over build value per field; blank = absent; no id → `None`.
- **`oauth_provider`** (pub) — `pub fn oauth_provider(id: &str) -> Option<OAuthProvider>`: `"google"` → `google_client_from_env()`; others `None`.
- **`xoauth2_payload`** (pub) — `pub fn xoauth2_payload(user: &str, access_token: &str) -> String`: `user=<u>\x01auth=Bearer <t>\x01\x01` (callers base64 it).
- **`authorize`** (pub) — `pub fn authorize(provider: &OAuthProvider, open_browser: &dyn Fn(&str), cancel: &CancelToken, timeout: Duration) -> RemoteResult<OAuthTokens>`
  - Behavior: binds `127.0.0.1:0`; redirect `http://127.0.0.1:<port>`; 32-byte verifier + 16-byte state (base64url); calls `open_browser(url)`; `wait_for_code`; POSTs `authorization_code` + `code_verifier` (+ client creds) to `token_url`; requires a refresh token (else `Auth`); `expires_at = now + expires_in` (default 3600); `email` from the id_token.
- **`refresh`** (pub) — `pub fn refresh(provider: &OAuthProvider, refresh_token: &str) -> RemoteResult<AccessToken>`: `grant_type=refresh_token`; errors per `parse_token_response`.
- **`now`** (pub(crate)) — Unix seconds (0 if the clock is before 1970).
- **`random_token`** (private) — `getrandom` bytes → base64url (no padding); failure → `Other`.
- **`pkce_challenge`** (private) — base64url(SHA-256(verifier)) (RFC 7636 S256).
- **`authorization_url`** (private) — `auth_url` + `?`/`&` + encoded `response_type, client_id, redirect_uri, scope (space-joined), state, code_challenge, code_challenge_method=S256` + `extra_params`.
- **`token_form`** (private) — fields + `client_id` (+ `client_secret` when set).
- **`parse_token_response`** (private) — 2xx JSON → `TokenResponse` (bad JSON → `Protocol`); errors: `invalid_grant`/`invalid_client`/`unauthorized_client`/`access_denied` → `Auth`, 5xx → `Network`, else `Protocol`; message `error: description`.
- **`email_from_id_token`** (private) — base64url-decodes the JWT payload (no signature check) and returns the `email` claim.
- **`wait_for_code`** (private) — non-blocking accept loop (50 ms sleeps) until a redirect arrives; cancel → `Cancelled` "sign-in cancelled"; deadline → `Cancelled` "sign-in timed out".
- **`handle_redirect`** (private) — reads one HTTP request (5 s timeout), answers 404 for non-redirect paths (returns `None`), otherwise the `page` (success or failure) and the parsed result.
- **`parse_redirect`** (private) — no query or neither `code` nor `error` → `None`; `error=access_denied` → `Auth` "access denied by the user"; other `error` → `Auth`; `state` mismatch → `Auth`; else `Ok(code)`.
- **`page`** (private) — minimal self-contained HTML (no external resources), English success/failure text.

### `src/connector.rs`

#### Types / constants

- **const `REFRESH_MARGIN`** (private): 60 s early refresh.
- **enum `Secret { Password(String), AccessToken(String) }`** (pub): what one login presents; `Debug` redacts.
- **enum `Credential { Password(String), OAuth { provider: OAuthProvider, refresh_token: String } }`** (pub): keychain credential; `Debug` redacts (shows provider id).
- **struct `ImapSmtpConnector`** (pub): `config`, `credential`, `options`, `token: Mutex<Option<AccessToken>>`, `gmail_seen: AtomicBool`; `Debug` shows email + redacted credential.

#### Functions / methods

##### Context: `ImapSmtpConnector`

- **`new`** (pub) — `pub fn new(config: AccountConfig, credential: Credential, options: ConnectOptions) -> Self`: empty token cache.
- **`config`** (pub) — the account config.
- **`current_refresh_token`** (pub) — `None` for passwords; else the rotated refresh token if one was received, otherwise the stored one (callers persist changes).
- **`secret`** (pub) — `pub fn secret(&self) -> RemoteResult<Secret>`: password as-is; OAuth → cached access token while `expires_at - 60 > now`, else `oauth::refresh` with the latest refresh token (keeps remembering a rotated one) and caches the result.
- **`invalidate_token`** (private) — marks the cached token expired.
- **`lock_token`** (private) — poison-tolerant lock of the cache.
- **`with_secret`** (private) — runs a login; for OAuth an `Auth` failure invalidates the cache and retries once with a refreshed token.
- **`connect_imap`** (private) — `ImapRemote::connect` via `with_secret`; records `X-GM-EXT-1` in `gmail_seen`.
- **`sender`** (private) — `SmtpSender::new` with the current secret.

##### Context: `impl MailConnector for ImapSmtpConnector`

- **`connect`** — boxed `connect_imap(cancel)`.
- **`connect_sender`** — cancelled token → `Cancelled`; else boxed `sender()`.
- **`saves_sent_copy`** — `is_gmail_host(imap.host)` or a session saw `X-GM-EXT-1`.

##### Context: module

- **`is_gmail_host`** (pub) — `pub fn is_gmail_host(host: &str) -> bool`: `imap.`/`smtp.` + `gmail.com`/`googlemail.com`, case-insensitive, trailing dot ignored.
- **`test_account`** (pub) — `pub fn test_account(config: AccountConfig, credential: Credential, options: ConnectOptions) -> RemoteResult<()>`: IMAP login + logout, then `SmtpSender::verify`; first failure returned.

### `src/presets.rs`

#### Types / constants

- **enum `LoginName { EmailAddress (default), LocalPart }`** (pub).
- **enum `PresetNote { AppPasswordRequired, OAuthRequired, ProtonBridge, EnableImapFirst }`** (pub): stable keys the UI localizes.
- **struct `ProviderPreset { name, imap, smtp, login, oauth: Option<&'static str>, note: Option<PresetNote> }`** (pub).
- **struct `Entry`** / **const `ENTRIES`** (private): static table — Gmail/Googlemail (OAuth `google`, 993 / 465), Outlook family (993 / 587 STARTTLS, `OAuthRequired`), Yahoo (+`.com.br`), iCloud (993 / 587 STARTTLS), Fastmail, Zoho, AOL, GMX (`.com` and `.net`/`.de`/`.at`/`.ch`), UOL/BOL (`imap.uol.com.br` / `smtps.uol.com.br:587`), Terra, Yandex, mail.com, Proton Bridge (`127.0.0.1:1143`/`1025` STARTTLS, `ProtonBridge`).
- **consts `TLS` / `STARTTLS`** (private): shorthands.

#### Functions / methods

- **`ProviderPreset::account_config`** (pub) — `pub fn account_config(&self, email: &str) -> AccountConfig`: trimmed email; username per `login`; `AuthKind::OAuth2 { provider }` when `oauth` is set else `Password`; `DEFAULT_SYNC_WINDOW`.
- **`provider_for_email`** (pub) — `pub fn provider_for_email(email: &str) -> Option<ProviderPreset>`: case-insensitive domain lookup in `ENTRIES`.
- **`domain`** (pub) — lowercased, trimmed domain after the last `@` (trailing dot removed); `None` if missing/empty.
- **`local_part`** (pub) — text before the last `@` (whole trimmed input without `@`).

### `src/autoconfig.rs`

#### Types

- **struct `Server { config: ServerConfig, username: String }`** (private).

#### Functions

- **`autoconfig_urls`** (pub) — `pub fn autoconfig_urls(email: &str) -> Vec<String>`: ISPDB `https://autoconfig.thunderbird.net/v1.1/<domain>`, `https://autoconfig.<domain>/mail/config-v1.1.xml?emailaddress=<encoded>`, `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml`; empty without a domain.
- **`autoconfig`** (pub) — `pub fn autoconfig(email: &str, timeout: Duration) -> Option<AccountConfig>`: `autoconfig_from(autoconfig_urls(email))`.
- **`autoconfig_from`** (private) — first URL answering 2xx whose body `parse_autoconfig` accepts; transport errors/non-2xx/unusable documents fall through.
- **`guess_config`** (pub) — `pub fn guess_config(email: &str) -> Option<AccountConfig>`: `imap.<domain>:993` TLS, `smtp.<domain>:587` STARTTLS, username = email, password auth.
- **`discover_config`** (pub) — `pub fn discover_config(email: &str, timeout: Duration) -> Option<AccountConfig>`: preset → autoconfig → guess (blocking; network only past the preset).
- **`parse_autoconfig`** (pub) — `pub fn parse_autoconfig(xml: &str, email: &str) -> Option<AccountConfig>`: first usable `incomingServer type="imap"` and `outgoingServer type="smtp"` under `emailProvider`; username from the IMAP server; password auth; `None` unless both exist.
- **`parse_server`** (private) — `socketType` SSL/TLS → `Tls`, STARTTLS → `StartTls`, anything else (cleartext too, user decision) → skip; hostname required (placeholders substituted); numeric port required; username substituted, default = email.
- **`substitute`** (private) — expands `%EMAILADDRESS%`, `%EMAILLOCALPART%`, `%EMAILDOMAIN%` (lowercased domain).
