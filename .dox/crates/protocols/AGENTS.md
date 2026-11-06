# crates/protocols/ — Mail server clients

## Purpose

Real implementations of `mail_core`'s blocking contracts: IMAP
(`ImapRemote: MailRemote`), SMTP submission (`SmtpSender: MailSender`), the
account connector (`ImapSmtpConnector: MailConnector`), OAuth 2.0 sign-in
(PKCE + loopback redirect, refresh, XOAUTH2), and server discovery (provider
presets, Thunderbird autoconfig, guess).

## Ownership

- Owns: `crates/protocols/**` (sources, `tests/greenmail.rs`,
  `tests/greenmail.sh`, `tests/fixtures/`).
- Does not own: the remote traits and value types (`mail_core::remote`,
  `mail_core::account`), sync orchestration (`mail_sync`), persistence and
  keychain storage of secrets (`storage` / `bgmail`), UI text (`bgmail`
  locale).

## Local Contracts

- **Certificates:** always validated, except on a literal loopback address
  (`ConnectOptions::for_host`, user decision), for Proton Bridge's self-signed one.
  Applied per host in `connect_tls` and `SmtpSender::new`.
- **Blocking.** Callers run every call on a background thread. The only async
  runtime is the single-threaded Tokio one `bimi::BrandChecker` owns for its
  resolver, never exposed. No GPUI. `#![forbid(unsafe_code)]`.
- **Dependencies (decided stack):** `imap = "3.0.0-alpha.15"` with
  `default-features = false` over our own rustls stream; `imap-proto 0.16`
  (parses raw command responses); `rustls 0.23` with the **ring** provider
  only; `rustls-platform-verifier 0.7` (OS trust store on Windows/macOS/Linux);
  `lettre 0.11` with `default-features = false` and `smtp-transport`,
  `rustls`, `ring`, `rustls-platform-verifier` (no `hostname`, so `EHLO` is `[127.0.0.1]`: the machine's
  name would reach `Received:`) — transport only
  (`send_raw`); `ureq 3` (`rustls`, `platform-verifier`, `json`) for OAuth and
  autoconfig; `sha2`, `base64 0.22`, `getrandom 0.3`, `roxmltree 0.21`. For BIMI:
  `mail-auth 0.7.5` (`default-features = false`, `ring`: DKIM/DMARC and its hickory
  resolver, on the same patched `mail-parser 0.11`), `tokio` (`rt`), `psl 2`,
  `rustls-webpki 0.103` (`ring`) + `rustls-pki-types`, `flate2`.
  **Never** add native-tls/OpenSSL, aws-lc, an `oauth2` crate or a second
  rustls; check with `cargo tree -p protocols -d` and
  `cargo tree --workspace -i rustls`.
- **Timeouts:** TCP connect 20 s per address, every read/write 60 s
  (`tls::CONNECT_TIMEOUT` / `tls::IO_TIMEOUT`); IDLE temporarily replaces the
  read timeout and `ImapStream::set_read_timeout(None)` restores 60 s. HTTP
  requests: 30 s (OAuth) or the caller's timeout (autoconfig).
- **Cancellation:** `ImapRemote::connect` registers a hook on the connector's
  `CancelToken` that shuts down a cloned socket; `idle` registers the idle
  token too. After a cancel the session is dead (calls → `Cancelled`; `idle`
  → `IdleOutcome::Stopped`); the engine reconnects. lettre owns its sockets,
  so SMTP only checks the token before starting.
- **Error kinds:** LOGIN/AUTHENTICATE `NO`/`BAD` → `Auth`; I/O → `Network`
  (rustls inside → `Tls`); `NO [TRYCREATE]` or "no such / doesn't exist"
  text → `NotFound`; other `NO`/`BAD`/parse → `Protocol`; `BYE` → `Network`.
  SMTP 530/534/535 (and 454 mentioning auth) → `Auth`, other replies →
  `Protocol`. OAuth `invalid_grant`/`invalid_client`/`access_denied` → `Auth`;
  sign-in timeout/cancel → `Cancelled`.
- **IMAP specifics:** `SELECT` (never EXAMINE; we store flags), cached per
  folder; `SELECT … (CONDSTORE)` when announced (QRESYNC implies it). Fetch
  items are added only when announced (`X-GM-MSGID X-GM-THRID` with
  `X-GM-EXT-1`, `MODSEQ`/`CHANGEDSINCE` with CONDSTORE). Folder names stay raw
  modified UTF-7. Commands are chunked (100 UIDs per header fetch, 500
  otherwise) with compressed sets (`1:3,5`). `move_messages` returns the new
  UIDs from `COPYUID` (MOVE's untagged OK or COPY's tagged OK); `None` when
  the server sends none. Without MOVE: `UID COPY` + `\Deleted` + `UID EXPUNGE`
  (without UIDPLUS, a plain `EXPUNGE` guarded so other clients' `\Deleted`
  messages survive). When COPY worked but the expunge failed, a replay copies
  again: accepted, because skipping the copy when the source is already
  `\Deleted` could lose mail another client flagged.
- **Secrets:** `Secret`/`Credential` `Debug` never prints values. Google's
  client id/secret come from `BGMAIL_GOOGLE_CLIENT_ID` /
  `BGMAIL_GOOGLE_CLIENT_SECRET` (runtime, then build-time `option_env!`). At
  build time `build.rs` also reads them from the workspace `.env` (gitignored,
  created empty when missing so edits always trigger a rebuild; real environment
  variables win). Never commit them. An installed-app client
  secret ships inside the binary, so it can be extracted: PKCE and the redirect
  rules protect the flow, not its secrecy. `ConnectOptions::accept_invalid_certs` is for local test
  servers only and is never set by the app.
- **User-facing text:** only the OAuth loopback page (English). Preset notes
  are `PresetNote` keys; the UI localizes them.

## Work Guidance

- Every function gets a test. Unit tests never touch the real network: IMAP
  runs against the scripted server (`imap_remote::test_server`), SMTP against
  `smtp::test_server`, HTTP/OAuth/autoconfig against `http::test_server`, TLS
  against `tls::test_support` with `tests/fixtures/test-{cert,key}.pem`
  (self-signed `localhost`/`127.0.0.1`, 100 years).
- Autoconfig parsing is checked on real ISPDB documents saved under
  `tests/fixtures/autoconfig-*.xml`; add a fixture when fixing a parser case.
- New presets go in `presets::ENTRIES` (domains must stay unique; a test
  enforces it).
- Integration tests (`tests/greenmail.rs`) are `#[ignore]` and gated by env:
  GreenMail (`BGMAIL_GREENMAIL=1`, SMTP+IMAP, IDLE/MOVE/UIDPLUS, no CONDSTORE)
  and Dovecot (`BGMAIL_DOVECOT=1`, STARTTLS/IMAPS, CONDSTORE, MOVE).
  `tests/greenmail.sh start|dovecot|stop` manages the containers; stop them
  when done.

## Verification

- `cargo test -p protocols`
- `cargo clippy -p protocols --all-targets -- -D warnings`
- `cargo fmt`
- Integration: `crates/protocols/tests/greenmail.sh start && crates/protocols/tests/greenmail.sh dovecot`,
  then `BGMAIL_GREENMAIL=1 BGMAIL_DOVECOT=1 cargo test -p protocols --test greenmail -- --ignored`,
  then `crates/protocols/tests/greenmail.sh stop`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/protocols/src/` | [`.dox/crates/protocols/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
