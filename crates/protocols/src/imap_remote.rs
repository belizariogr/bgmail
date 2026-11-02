//! IMAP4rev1 client implementing [`mail_core::MailRemote`].
//!
//! The `imap` crate drives the connection (login, AUTHENTICATE, APPEND, IDLE,
//! literal handling); most other commands are sent raw and their responses
//! parsed here with `imap-proto`, because the crate's typed results hide what
//! we need (`X-GM-MSGID`/`X-GM-THRID`, `COPYUID`, `HIGHESTMODSEQ` via
//! `SELECT … (CONDSTORE)`).
//!
//! Cancellation: at connect a cloned socket is registered on the connector's
//! [`CancelToken`]; cancelling shuts it down, which unblocks any read (IDLE
//! included). After that the session is dead and calls fail with
//! [`RemoteErrorKind::Cancelled`].

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use imap::extensions::idle::{SetReadTimeout, WaitOutcome};
use imap::types::Flag;
use imap_proto::types::{
    AttributeValue, BodyStructure, MailboxDatum, MessageSection, NameAttribute, Response,
    ResponseCode, SectionPath, UidSetMember,
};
use mail_core::{
    CancelGuard, CancelToken, Capabilities, FolderStatus, IdleOutcome, MailRemote, MessageFlags,
    RemoteError, RemoteErrorKind, RemoteFlags, RemoteFolder, RemoteHeader, RemoteResult, Security,
    ServerConfig,
};

use crate::connector::Secret;
use crate::tls::{self, ConnectOptions, TlsStream, IO_TIMEOUT};

/// UIDs per `UID FETCH` of headers (keeps each response a few hundred KB).
const HEADER_CHUNK: usize = 100;
/// UIDs per flag fetch / STORE / COPY / MOVE command.
const COMMAND_CHUNK: usize = 500;

/// The socket under an IMAP session: plain (local test servers) or TLS.
#[derive(Debug)]
pub enum ImapStream {
    Plain(TcpStream),
    Tls(Box<TlsStream>),
}

impl ImapStream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Self::Plain(tcp) => tcp,
            Self::Tls(tls) => &tls.sock,
        }
    }
}

impl Read for ImapStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(tcp) => tcp.read(buf),
            Self::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for ImapStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(tcp) => tcp.write(buf),
            Self::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(tcp) => tcp.flush(),
            Self::Tls(tls) => tls.flush(),
        }
    }
}

impl SetReadTimeout for ImapStream {
    /// IDLE sets its wait as the read timeout and resets it with `None`
    /// afterwards; `None` restores [`IO_TIMEOUT`] so a stalled server can
    /// never block a command forever.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::Result<()> {
        let timeout = timeout
            .map(|t| t.max(Duration::from_millis(10)))
            .unwrap_or(IO_TIMEOUT);
        self.tcp()
            .set_read_timeout(Some(timeout))
            .map_err(imap::Error::Io)
    }
}

/// SASL XOAUTH2: answers the initial (empty) challenge with the bearer
/// payload; a non-empty challenge is the server's JSON error, which must be
/// acknowledged with an empty response so it sends the tagged `NO`.
struct XOAuth2 {
    payload: String,
}

impl imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&self, challenge: &[u8]) -> String {
        if challenge.is_empty() {
            self.payload.clone()
        } else {
            String::new()
        }
    }
}

/// An authenticated IMAP session.
pub struct ImapRemote {
    session: imap::Session<ImapStream>,
    capabilities: Capabilities,
    /// Folder currently SELECTed (raw name), to skip redundant SELECTs.
    selected: Option<String>,
    /// Clone of the session socket, shut down to interrupt blocking reads.
    socket: TcpStream,
    cancel: CancelToken,
    _cancel_hook: CancelGuard,
}

impl std::fmt::Debug for ImapRemote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapRemote")
            .field("capabilities", &self.capabilities)
            .field("selected", &self.selected)
            .finish()
    }
}

impl ImapRemote {
    /// Connects (TLS / STARTTLS / plain per `server.security`), logs in with
    /// `LOGIN` or `AUTHENTICATE XOAUTH2`, and reads the capabilities.
    /// `cancel` can interrupt the session for its whole lifetime.
    pub fn connect(
        server: &ServerConfig,
        username: &str,
        secret: &Secret,
        options: ConnectOptions,
        cancel: &CancelToken,
    ) -> RemoteResult<Self> {
        check_cancel(cancel)?;
        let mut tcp = tls::connect_tcp(&server.host, server.port)?;
        let socket = tcp.try_clone().map_err(tls::io_error)?;
        let hook_socket = tcp.try_clone().map_err(tls::io_error)?;
        let hook = cancel.on_cancel(move || tls::shutdown(&hook_socket));
        let cancelled = |err: RemoteError| {
            if cancel.is_cancelled() {
                RemoteError::new(RemoteErrorKind::Cancelled, "cancelled")
            } else {
                err
            }
        };

        let (stream, greeting_read) = match server.security {
            Security::Tls => (
                ImapStream::Tls(Box::new(
                    tls::connect_tls(tcp, &server.host, options).map_err(cancelled)?,
                )),
                false,
            ),
            Security::StartTls => {
                tls::imap_starttls(&mut tcp).map_err(cancelled)?;
                (
                    ImapStream::Tls(Box::new(
                        tls::connect_tls(tcp, &server.host, options).map_err(cancelled)?,
                    )),
                    true,
                )
            }
            Security::None => (ImapStream::Plain(tcp), false),
        };

        let mut client = imap::Client::new(stream);
        if !greeting_read {
            let greeting = client
                .read_greeting()
                .map_err(|err| cancelled(map_error(err)))?;
            if !greeting.starts_with(b"* OK") && !greeting.starts_with(b"* PREAUTH") {
                return Err(RemoteError::new(
                    RemoteErrorKind::Network,
                    format!(
                        "server refused the connection: {}",
                        String::from_utf8_lossy(&greeting).trim_end()
                    ),
                ));
            }
        }

        let session = match secret {
            Secret::Password(password) => client.login(username, password),
            Secret::AccessToken(token) => client.authenticate(
                "XOAUTH2",
                &XOAuth2 {
                    payload: crate::oauth::xoauth2_payload(username, token),
                },
            ),
        };
        let mut session = session.map_err(|(err, _)| cancelled(map_login_error(err)))?;
        let raw_caps = session
            .capabilities()
            .map_err(|err| cancelled(map_error(err)))?;
        let capabilities = parse_capabilities(raw_caps.iter().filter_map(|cap| match cap {
            imap_proto::types::Capability::Atom(atom) => Some(atom.as_ref()),
            _ => None,
        }));

        Ok(Self {
            session,
            capabilities,
            selected: None,
            socket,
            cancel: cancel.clone(),
            _cancel_hook: hook,
        })
    }

    /// Maps a crate error, reporting `Cancelled` once the token fired.
    fn fail(&self, err: imap::Error) -> RemoteError {
        if self.cancel.is_cancelled() {
            return RemoteError::new(RemoteErrorKind::Cancelled, "cancelled");
        }
        map_error(err)
    }

    /// Sends a raw command; returns every response line including the tagged
    /// completion (which may carry `COPYUID` and friends).
    fn run(&mut self, command: &str) -> RemoteResult<Vec<u8>> {
        check_cancel(&self.cancel)?;
        match self.session.run(command) {
            Ok((data, _)) => Ok(data),
            Err(err) => Err(self.fail(err)),
        }
    }

    fn ensure_selected(&mut self, folder: &str) -> RemoteResult<()> {
        if self.selected.as_deref() == Some(folder) {
            return Ok(());
        }
        self.select(folder).map(|_| ())
    }

    fn fetch_header_chunk(&mut self, set: &str) -> RemoteResult<Vec<RemoteHeader>> {
        let command = format!("UID FETCH {set} ({})", header_items(&self.capabilities));
        let data = self.run(&command)?;
        Ok(parse_responses(&data)
            .into_iter()
            .filter_map(|response| match response {
                Response::Fetch(_, attrs) => header_from_attributes(&attrs),
                _ => None,
            })
            .collect())
    }

    /// Flags `uids` with `+FLAGS.SILENT` / `-FLAGS.SILENT` (`op` is `+` or `-`).
    fn store(&mut self, uids: &[u32], op: char, flags: &[&str]) -> RemoteResult<()> {
        if flags.is_empty() || uids.is_empty() {
            return Ok(());
        }
        for set in uid_chunks(uids, COMMAND_CHUNK) {
            self.run(&format!(
                "UID STORE {set} {op}FLAGS.SILENT ({})",
                flags.join(" ")
            ))?;
        }
        Ok(())
    }

    /// Permanently removes `uids` (already flagged `\Deleted`) from the
    /// selected folder, never anything else.
    ///
    /// With UIDPLUS: `UID EXPUNGE`. Without it a plain `EXPUNGE` would also
    /// destroy messages another client flagged `\Deleted` (and may still
    /// want to undelete), so those are found with `UID SEARCH DELETED`,
    /// unflagged for the `EXPUNGE` and re-flagged right after. Failure modes
    /// stay non-destructive: if the connection drops before the restore,
    /// those messages merely lose `\Deleted`. Residual race: a message
    /// another client flags between our SEARCH and EXPUNGE is still
    /// expunged (IMAP offers no atomic alternative without UIDPLUS).
    fn expunge(&mut self, uids: &[u32]) -> RemoteResult<()> {
        if self.capabilities.uidplus {
            for set in uid_chunks(uids, COMMAND_CHUNK) {
                self.run(&format!("UID EXPUNGE {set}"))?;
            }
            return Ok(());
        }
        let ours: HashSet<u32> = uids.iter().copied().collect();
        let others: Vec<u32> = self
            .search_uids("UID SEARCH DELETED")?
            .into_iter()
            .filter(|uid| !ours.contains(uid))
            .collect();
        self.store(&others, '-', &["\\Deleted"])?;
        let expunged = self.run("EXPUNGE");
        let restored = self.store(&others, '+', &["\\Deleted"]);
        expunged?;
        restored
    }

    /// Runs a `UID SEARCH …` command; the matching UIDs, ascending.
    fn search_uids(&mut self, command: &str) -> RemoteResult<Vec<u32>> {
        let data = self.run(command)?;
        let mut uids: Vec<u32> = parse_responses(&data)
            .into_iter()
            .flat_map(|response| match response {
                Response::MailboxData(MailboxDatum::Search(uids)) => uids,
                _ => Vec::new(),
            })
            .collect();
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }
}

impl MailRemote for ImapRemote {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn list_folders(&mut self) -> RemoteResult<Vec<RemoteFolder>> {
        let data = self.run("LIST \"\" \"*\"")?;
        let responses = parse_responses(&data);
        Ok(responses
            .into_iter()
            .filter_map(|response| match response {
                Response::MailboxData(MailboxDatum::List {
                    name_attributes,
                    delimiter,
                    name,
                }) => Some(RemoteFolder::new(
                    name.as_ref(),
                    delimiter.as_deref(),
                    name_attributes.iter().map(name_attribute).collect(),
                )),
                _ => None,
            })
            .collect())
    }

    fn select(&mut self, folder: &str) -> RemoteResult<FolderStatus> {
        // A failed SELECT leaves nothing selected (RFC 3501 §6.3.1).
        self.selected = None;
        let mut command = format!("SELECT {}", quote(folder)?);
        if self.capabilities.condstore {
            command.push_str(" (CONDSTORE)");
        }
        let data = self.run(&command)?;
        let responses = parse_responses(&data);
        self.selected = Some(folder.to_string());
        Ok(folder_status(&responses))
    }

    fn uids(&mut self, folder: &str) -> RemoteResult<Vec<u32>> {
        self.ensure_selected(folder)?;
        self.search_uids("UID SEARCH ALL")
    }

    /// `UID SEARCH HEADER Message-ID "<id>"`; the highest matching UID (the
    /// newest copy when a folder holds duplicates).
    fn find_uid_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> RemoteResult<Option<u32>> {
        let message_id = message_id.trim();
        if message_id.is_empty() {
            return Ok(None);
        }
        // HEADER search is a substring match: the angle brackets keep
        // `a@x` from matching `<ba@x>`.
        let bracketed = if message_id.starts_with('<') {
            message_id.to_string()
        } else {
            format!("<{message_id}>")
        };
        self.ensure_selected(folder)?;
        let command = format!("UID SEARCH HEADER Message-ID {}", quote(&bracketed)?);
        Ok(self.search_uids(&command)?.last().copied())
    }

    fn fetch_headers(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<Vec<RemoteHeader>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_selected(folder)?;
        let wanted: HashSet<u32> = uids.iter().copied().collect();
        let mut out = Vec::new();
        for set in uid_chunks(uids, HEADER_CHUNK) {
            out.extend(
                self.fetch_header_chunk(&set)?
                    .into_iter()
                    .filter(|header| wanted.contains(&header.uid)),
            );
        }
        out.sort_by_key(|header| header.uid);
        out.dedup_by_key(|header| header.uid);
        Ok(out)
    }

    fn fetch_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        changed_since: Option<u64>,
    ) -> RemoteResult<Vec<RemoteFlags>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_selected(folder)?;
        let wanted: HashSet<u32> = uids.iter().copied().collect();
        let condstore = self.capabilities.condstore;
        let mut out = Vec::new();
        for set in uid_chunks(uids, COMMAND_CHUNK) {
            let mut command = format!(
                "UID FETCH {set} (UID FLAGS{})",
                if condstore { " MODSEQ" } else { "" }
            );
            if let (true, Some(modseq)) = (condstore, changed_since) {
                command.push_str(&format!(" (CHANGEDSINCE {modseq})"));
            }
            let data = self.run(&command)?;
            let responses = parse_responses(&data);
            out.extend(responses.into_iter().filter_map(|response| match response {
                Response::Fetch(_, attrs) => {
                    flags_from_attributes(&attrs).filter(|f| wanted.contains(&f.uid))
                }
                _ => None,
            }));
        }
        out.sort_by_key(|flags| flags.uid);
        out.dedup_by_key(|flags| flags.uid);
        Ok(out)
    }

    fn fetch_body(&mut self, folder: &str, uid: u32) -> RemoteResult<Vec<u8>> {
        self.ensure_selected(folder)?;
        let data = self.run(&format!("UID FETCH {uid} (UID BODY.PEEK[])"))?;
        let responses = parse_responses(&data);
        responses
            .into_iter()
            .find_map(|response| match response {
                Response::Fetch(_, attrs) => body_from_attributes(&attrs, uid),
                _ => None,
            })
            .ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::NotFound,
                    format!("message {uid} not found in {folder}"),
                )
            })
    }

    fn store_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        add: MessageFlags,
        remove: MessageFlags,
    ) -> RemoteResult<()> {
        if uids.is_empty() {
            return Ok(());
        }
        self.ensure_selected(folder)?;
        self.store(uids, '+', &add.to_imap())?;
        self.store(uids, '-', &remove.to_imap())
    }

    fn move_messages(
        &mut self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> RemoteResult<Option<Vec<u32>>> {
        if uids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        self.ensure_selected(from)?;
        let target = quote(to)?;
        let mut mapping = HashMap::new();
        let mut complete = true;
        for set in uid_chunks(uids, COMMAND_CHUNK) {
            let data = if self.capabilities.r#move {
                self.run(&format!("UID MOVE {set} {target}"))?
            } else {
                self.run(&format!("UID COPY {set} {target}"))?
            };
            let responses = parse_responses(&data);
            match copy_uids(&responses) {
                Some(pairs) => mapping.extend(pairs),
                None => complete = false,
            }
        }
        if !self.capabilities.r#move {
            self.store(uids, '+', &["\\Deleted"])?;
            self.expunge(uids)?;
        }
        if !complete {
            return Ok(None);
        }
        Ok(uids
            .iter()
            .map(|uid| mapping.get(uid).copied())
            .collect::<Option<Vec<u32>>>())
    }

    fn delete_messages(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<()> {
        if uids.is_empty() {
            return Ok(());
        }
        self.ensure_selected(folder)?;
        self.store(uids, '+', &["\\Deleted"])?;
        self.expunge(uids)
    }

    fn append(&mut self, folder: &str, flags: MessageFlags, raw: &[u8]) -> RemoteResult<()> {
        check_cancel(&self.cancel)?;
        // The crate wraps the name in quotes verbatim; pre-escape it.
        let escaped = escape_quoted(folder)?;
        let atoms = flags.to_imap();
        let result = self
            .session
            .append(&escaped, raw)
            .flags(atoms.into_iter().map(Flag::from))
            .finish();
        result.map(|_| ()).map_err(|err| self.fail(err))
    }

    fn create_folder(&mut self, name: &str) -> RemoteResult<()> {
        self.run(&format!("CREATE {}", quote(name)?)).map(|_| ())
    }

    fn noop(&mut self) -> RemoteResult<()> {
        self.run("NOOP").map(|_| ())
    }

    fn logout(&mut self) {
        if !self.cancel.is_cancelled() {
            let _ = self.session.logout();
        }
        self.selected = None;
        tls::shutdown(&self.socket);
    }

    fn idle(
        &mut self,
        folder: &str,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> RemoteResult<IdleOutcome> {
        if !self.capabilities.idle {
            return Err(RemoteError::new(
                RemoteErrorKind::Unsupported,
                "server does not support IDLE",
            ));
        }
        if cancel.is_cancelled() || self.cancel.is_cancelled() {
            return Ok(IdleOutcome::Stopped);
        }
        self.ensure_selected(folder)?;
        let socket = self.socket.try_clone().map_err(tls::io_error)?;
        let _hook = cancel.on_cancel(move || tls::shutdown(&socket));
        let result = {
            let mut handle = self.session.idle();
            handle.timeout(timeout).keepalive(false);
            handle.wait_while(imap::extensions::idle::stop_on_any)
            // Dropping the handle sends DONE and reads the completion.
        };
        if cancel.is_cancelled() || self.cancel.is_cancelled() {
            // The socket is shut down; this session can't be used again.
            self.selected = None;
            return Ok(IdleOutcome::Stopped);
        }
        match result {
            Ok(WaitOutcome::MailboxChanged) => Ok(IdleOutcome::Changed),
            Ok(WaitOutcome::TimedOut) => Ok(IdleOutcome::Timeout),
            Err(err) => Err(self.fail(err)),
        }
    }
}

fn check_cancel(cancel: &CancelToken) -> RemoteResult<()> {
    if cancel.is_cancelled() {
        Err(RemoteError::new(RemoteErrorKind::Cancelled, "cancelled"))
    } else {
        Ok(())
    }
}

/// Reads the extensions we adapt to from CAPABILITY atoms (case-insensitive;
/// QRESYNC implies CONDSTORE).
pub fn parse_capabilities<'a>(atoms: impl IntoIterator<Item = &'a str>) -> Capabilities {
    let mut caps = Capabilities::default();
    for atom in atoms {
        match atom.to_ascii_uppercase().as_str() {
            "IDLE" => caps.idle = true,
            "MOVE" => caps.r#move = true,
            "UIDPLUS" => caps.uidplus = true,
            "CONDSTORE" | "QRESYNC" => caps.condstore = true,
            "X-GM-EXT-1" => caps.gmail = true,
            "SPECIAL-USE" => caps.special_use = true,
            _ => {}
        }
    }
    caps
}

/// Compresses UIDs into an IMAP sequence set: `[1,2,3,5,9,10]` → `1:3,5,9:10`
/// (input order and duplicates don't matter).
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut iter = sorted.into_iter().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end.wrapping_add(1))) && end != u32::MAX {
            end = iter.next().unwrap_or(end);
        }
        if start == end {
            parts.push(start.to_string());
        } else {
            parts.push(format!("{start}:{end}"));
        }
    }
    parts.join(",")
}

/// Splits `uids` (sorted, deduplicated) into sets of at most `size` UIDs.
pub fn uid_chunks(uids: &[u32], size: usize) -> Vec<String> {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.chunks(size.max(1)).map(uid_set).collect()
}

/// Escapes an IMAP quoted-string body (`\` and `"`); CR/LF can't be quoted.
fn escape_quoted(value: &str) -> RemoteResult<String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(RemoteError::new(
            RemoteErrorKind::Other,
            "mailbox names cannot contain line breaks",
        ));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `value` as an IMAP quoted string.
pub fn quote(value: &str) -> RemoteResult<String> {
    Ok(format!("\"{}\"", escape_quoted(value)?))
}

/// LIST attribute as the conventional IMAP atom (`\Sent`, `\Noselect`, …).
fn name_attribute(attribute: &NameAttribute<'_>) -> String {
    match attribute {
        NameAttribute::NoInferiors => "\\Noinferiors".into(),
        NameAttribute::NoSelect => "\\Noselect".into(),
        NameAttribute::Marked => "\\Marked".into(),
        NameAttribute::Unmarked => "\\Unmarked".into(),
        NameAttribute::All => "\\All".into(),
        NameAttribute::Archive => "\\Archive".into(),
        NameAttribute::Drafts => "\\Drafts".into(),
        NameAttribute::Flagged => "\\Flagged".into(),
        NameAttribute::Junk => "\\Junk".into(),
        NameAttribute::Sent => "\\Sent".into(),
        NameAttribute::Trash => "\\Trash".into(),
        NameAttribute::Extension(other) => other.to_string(),
        other => format!("{other:?}"),
    }
}

/// Parses every response in `data` (already validated line by line by the
/// `imap` crate, which fails the command on anything unparseable); a line we
/// still can't parse is skipped rather than failing the command.
fn parse_responses(data: &[u8]) -> Vec<Response<'_>> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        match imap_proto::parser::parse_response(rest) {
            Ok((next, response)) => {
                out.push(response);
                rest = next;
            }
            Err(_) => {
                rest = match rest.windows(2).position(|w| w == b"\r\n") {
                    Some(end) => &rest[end + 2..],
                    None => &[],
                };
            }
        }
    }
    out
}

/// `FolderStatus` from SELECT's untagged data (`EXISTS`, `OK [UIDVALIDITY]`,
/// `OK [UIDNEXT]`, `OK [HIGHESTMODSEQ]`).
fn folder_status(responses: &[Response<'_>]) -> FolderStatus {
    let mut status = FolderStatus::default();
    for response in responses {
        match response {
            Response::MailboxData(MailboxDatum::Exists(n)) => status.exists = *n,
            Response::Data {
                code: Some(code), ..
            }
            | Response::Done {
                code: Some(code), ..
            } => match code {
                ResponseCode::UidValidity(v) => status.uid_validity = *v,
                ResponseCode::UidNext(v) => status.uid_next = Some(*v),
                ResponseCode::HighestModSeq(v) => status.highest_modseq = Some(*v),
                _ => {}
            },
            _ => {}
        }
    }
    status
}

/// Fetch items for a header fetch, adding Gmail/CONDSTORE items only when the
/// server announced them (others reply BAD to unknown items).
fn header_items(caps: &Capabilities) -> String {
    let mut items = String::from("UID FLAGS RFC822.SIZE BODY.PEEK[HEADER] BODYSTRUCTURE");
    if caps.gmail {
        items.push_str(" X-GM-MSGID X-GM-THRID");
    }
    if caps.condstore {
        items.push_str(" MODSEQ");
    }
    items
}

fn fetch_flags(attrs: &[AttributeValue<'_>]) -> Option<MessageFlags> {
    attrs.iter().find_map(|attr| match attr {
        AttributeValue::Flags(flags) => Some(MessageFlags::from_imap(flags.iter())),
        _ => None,
    })
}

fn fetch_uid(attrs: &[AttributeValue<'_>]) -> Option<u32> {
    attrs.iter().find_map(|attr| match attr {
        AttributeValue::Uid(uid) => Some(*uid),
        _ => None,
    })
}

/// A [`RemoteHeader`] from one FETCH response; `None` unless it carries both
/// a UID and the header section (unsolicited flag updates don't).
fn header_from_attributes(attrs: &[AttributeValue<'_>]) -> Option<RemoteHeader> {
    let uid = fetch_uid(attrs)?;
    let mut header = RemoteHeader {
        uid,
        flags: fetch_flags(attrs).unwrap_or_default(),
        ..Default::default()
    };
    let mut has_header = false;
    for attr in attrs {
        match attr {
            AttributeValue::Rfc822Size(size) => header.size = *size,
            AttributeValue::GmailMsgId(id) => header.gm_msgid = Some(*id),
            AttributeValue::GmailThrId(id) => header.gm_thrid = Some(*id),
            AttributeValue::ModSeq(modseq) => header.modseq = Some(*modseq),
            AttributeValue::BodyStructure(structure) => {
                header.has_attachments = Some(has_attachments(structure))
            }
            AttributeValue::BodySection {
                section: Some(SectionPath::Full(MessageSection::Header)),
                data,
                ..
            }
            | AttributeValue::Rfc822Header(data) => {
                header.header = data.as_deref().unwrap_or_default().to_vec();
                has_header = true;
            }
            _ => {}
        }
    }
    has_header.then_some(header)
}

/// [`RemoteFlags`] from one FETCH response (needs UID and FLAGS).
fn flags_from_attributes(attrs: &[AttributeValue<'_>]) -> Option<RemoteFlags> {
    Some(RemoteFlags {
        uid: fetch_uid(attrs)?,
        flags: fetch_flags(attrs)?,
        modseq: attrs.iter().find_map(|attr| match attr {
            AttributeValue::ModSeq(modseq) => Some(*modseq),
            _ => None,
        }),
    })
}

/// The full message (`BODY[]`) of `uid` from one FETCH response.
fn body_from_attributes(attrs: &[AttributeValue<'_>], uid: u32) -> Option<Vec<u8>> {
    if fetch_uid(attrs) != Some(uid) {
        return None;
    }
    attrs.iter().find_map(|attr| match attr {
        AttributeValue::BodySection {
            section: None,
            data: Some(data),
            ..
        }
        | AttributeValue::Rfc822(Some(data)) => Some(data.to_vec()),
        _ => None,
    })
}

/// Whether BODYSTRUCTURE shows a user-facing attachment. Deliberately
/// simple: any part with `Content-Disposition: attachment`, any attached
/// message (`message/rfc822`), or a non-text leaf that is neither marked
/// `inline` nor referenced by Content-ID (inline images in HTML mail).
/// The parsed body (`mail_core::has_attachments`) refines this later.
pub fn has_attachments(structure: &BodyStructure<'_>) -> bool {
    let disposition = |common: &imap_proto::types::BodyContentCommon<'_>| {
        common
            .disposition
            .as_ref()
            .map(|d| d.ty.to_ascii_lowercase())
    };
    match structure {
        BodyStructure::Multipart { bodies, common, .. } => {
            disposition(common).as_deref() == Some("attachment")
                || bodies.iter().any(has_attachments)
        }
        BodyStructure::Message { .. } => true,
        BodyStructure::Text { common, .. } => disposition(common).as_deref() == Some("attachment"),
        BodyStructure::Basic { common, other, .. } => match disposition(common).as_deref() {
            Some("attachment") => true,
            Some("inline") => false,
            _ => other.id.is_none(),
        },
    }
}

/// Expands `1:3,5` members into UIDs.
fn expand_uid_set(members: &[UidSetMember]) -> Vec<u32> {
    members
        .iter()
        .flat_map(|member| match member {
            UidSetMember::Uid(uid) => *uid..=*uid,
            UidSetMember::UidRange(range) => {
                let (a, b) = (*range.start(), *range.end());
                a.min(b)..=a.max(b)
            }
        })
        .collect()
}

/// `source → destination` UID pairs from every `COPYUID` code (RFC 4315) in
/// the responses; `None` when the server sent none (no UIDPLUS).
fn copy_uids(responses: &[Response<'_>]) -> Option<Vec<(u32, u32)>> {
    let mut pairs = Vec::new();
    let mut found = false;
    for response in responses {
        let code = match response {
            Response::Data { code, .. } | Response::Done { code, .. } => code,
            _ => continue,
        };
        if let Some(ResponseCode::CopyUid(_, source, dest)) = code {
            found = true;
            let source = expand_uid_set(source);
            let dest = expand_uid_set(dest);
            if source.len() == dest.len() {
                pairs.extend(source.into_iter().zip(dest));
            }
        }
    }
    found.then_some(pairs)
}

/// imap errors → [`RemoteErrorKind`]: I/O → Network/Tls, `NO` with
/// `[TRYCREATE]` or a "no such mailbox" text → NotFound, other `NO`/`BAD` →
/// Protocol, `BYE` / lost connection → Network.
pub fn map_error(err: imap::Error) -> RemoteError {
    use imap::Error as E;
    match err {
        E::Io(io) => tls::io_error(io),
        E::No(no) => {
            let kind = no_kind(&no.information, no.code.as_ref());
            RemoteError::new(kind, no.information)
        }
        E::Bad(bad) => RemoteError::new(RemoteErrorKind::Protocol, bad.information),
        E::Bye(bye) => RemoteError::new(
            RemoteErrorKind::Network,
            format!("server closed the connection: {}", bye.information),
        ),
        E::ConnectionLost => RemoteError::new(RemoteErrorKind::Network, "connection lost"),
        E::Validate(err) => RemoteError::new(RemoteErrorKind::Other, err.to_string()),
        other => RemoteError::new(RemoteErrorKind::Protocol, other.to_string()),
    }
}

/// Kind of a `NO` answer: `[TRYCREATE]` or a "no such mailbox"-style text
/// means the folder/message is missing; anything else is a protocol refusal.
fn no_kind(information: &str, code: Option<&ResponseCode<'_>>) -> RemoteErrorKind {
    let text = information.to_ascii_lowercase();
    let missing = matches!(code, Some(ResponseCode::TryCreate))
        || [
            "nonexist",
            "not exist",
            "doesn't exist",
            "no such",
            "unknown mailbox",
            "not found",
        ]
        .iter()
        .any(|needle| text.contains(needle));
    if missing {
        RemoteErrorKind::NotFound
    } else {
        RemoteErrorKind::Protocol
    }
}

/// Like [`map_error`], but a `NO`/`BAD` answer to LOGIN/AUTHENTICATE means
/// the credentials were rejected.
pub fn map_login_error(err: imap::Error) -> RemoteError {
    match err {
        imap::Error::No(no) => {
            RemoteError::new(login_refusal_kind(&no.information), no.information)
        }
        imap::Error::Bad(bad) => RemoteError::new(RemoteErrorKind::Auth, bad.information),
        other => map_error(other),
    }
}

/// Why a server said `NO` to a login. RFC 5530 codes for a passing problem aren't a
/// verdict on the password: treated as one, the account would stop syncing until the
/// user intervened. They arrive in the text, since the parser doesn't know them.
fn login_refusal_kind(information: &str) -> RemoteErrorKind {
    let text = information.trim_start().to_ascii_uppercase();
    let passing = ["[UNAVAILABLE]", "[SERVERBUG]", "[INUSE]", "[LIMIT]"];
    if passing.iter().any(|code| text.starts_with(code)) {
        RemoteErrorKind::Network
    } else {
        RemoteErrorKind::Auth
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! Scripted IMAP server: each step expects a command prefix (after the
    //! tag) and answers with a reply where `{tag}` is the command's tag.
    //! A step whose expectation starts with `=` matches a whole untagged
    //! line (APPEND literal; `=*` matches any line) and replies using the
    //! previous tag.

    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    /// Longest wait for the client's next line; well under `tls::IO_TIMEOUT`,
    /// so the server reports the stuck step first.
    const CLIENT_WAIT: Duration = Duration::from_secs(30);

    pub type Step = (&'static str, &'static str);

    pub fn start(
        greeting: &'static str,
        steps: Vec<Step>,
    ) -> (u16, JoinHandle<Result<(), String>>) {
        start_n(greeting, steps, 1)
    }

    /// Serves `connections` consecutive connections with the same script.
    pub fn start_n(
        greeting: &'static str,
        steps: Vec<Step>,
        connections: usize,
    ) -> (u16, JoinHandle<Result<(), String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for _ in 0..connections {
                let (stream, _) = listener.accept().map_err(|e| e.to_string())?;
                serve(stream, greeting, &steps)?;
            }
            Ok(())
        });
        (port, handle)
    }

    fn serve(stream: TcpStream, greeting: &str, steps: &[Step]) -> Result<(), String> {
        // A client stuck in a test fails it instead of hanging the whole run.
        stream
            .set_read_timeout(Some(CLIENT_WAIT))
            .map_err(|e| e.to_string())?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        writer
            .write_all(greeting.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut last_tag = String::new();
        for (expect, reply) in steps {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .map_err(|e| format!("{e} while waiting for {expect:?}"))?;
            if read == 0 {
                return Err(format!("client hung up, expected {expect:?}"));
            }
            let line = line.trim_end_matches("\r\n");
            let tag = if let Some(exact) = expect.strip_prefix('=') {
                if exact != "*" && line != exact {
                    return Err(format!("expected line {exact:?}, got {line:?}"));
                }
                last_tag.clone()
            } else {
                let (tag, command) = line.split_once(' ').unwrap_or((line, ""));
                if !command.starts_with(expect) {
                    let _ = writer.write_all(format!("{tag} BAD unexpected\r\n").as_bytes());
                    return Err(format!("expected {expect:?}, got {command:?}"));
                }
                tag.to_string()
            };
            last_tag = tag.clone();
            writer
                .write_all(reply.replace("{tag}", &tag).as_bytes())
                .map_err(|e| e.to_string())?;
        }
        // Drain until the client hangs up so it never sees a reset.
        let mut rest = String::new();
        loop {
            match reader.read_line(&mut rest) {
                Ok(0) => return Ok(()),
                Ok(_) => rest.clear(),
                Err(e) if e.kind() == ErrorKind::ConnectionReset => return Ok(()),
                Err(e) => return Err(format!("{e} while waiting for the client to hang up")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{start, Step};
    use super::*;

    #[test]
    fn a_passing_login_refusal_is_not_a_verdict_on_the_password() {
        for passing in [
            "[UNAVAILABLE] Temporary authentication failure",
            "[serverbug] oops",
            "  [INUSE] try later",
            "[LIMIT] too many",
        ] {
            assert_eq!(login_refusal_kind(passing), RemoteErrorKind::Network);
        }
        for refused in ["[AUTHENTICATIONFAILED] nope", "Invalid credentials", ""] {
            assert_eq!(login_refusal_kind(refused), RemoteErrorKind::Auth);
        }
    }
    use std::ops::RangeInclusive;
    use std::thread;
    use std::time::Instant;

    const GREETING: &str = "* OK fake ready\r\n";

    fn server(port: u16) -> ServerConfig {
        ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
        }
    }

    fn password() -> Secret {
        Secret::Password("pw".into())
    }

    /// Login + CAPABILITY with `caps`, then `steps`.
    fn session(
        caps: &'static str,
        steps: Vec<Step>,
    ) -> (ImapRemote, thread::JoinHandle<Result<(), String>>) {
        let mut all: Vec<Step> = vec![("LOGIN \"me\" \"pw\"", "{tag} OK logged in\r\n")];
        all.push(("CAPABILITY", caps));
        all.extend(steps);
        let (port, handle) = start(GREETING, all);
        let remote = ImapRemote::connect(
            &server(port),
            "me",
            &password(),
            ConnectOptions::default(),
            &CancelToken::new(),
        )
        .unwrap();
        (remote, handle)
    }

    const BASIC_CAPS: &str = "* CAPABILITY IMAP4rev1 IDLE UIDPLUS\r\n{tag} OK done\r\n";
    const FULL_CAPS: &str =
        "* CAPABILITY IMAP4rev1 IDLE MOVE UIDPLUS CONDSTORE X-GM-EXT-1 SPECIAL-USE\r\n{tag} OK done\r\n";
    const SELECT_INBOX: Step = (
        "SELECT \"INBOX\"",
        "* 3 EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n* OK [UIDNEXT 12] ok\r\n{tag} OK [READ-WRITE] done\r\n",
    );

    fn finish(mut remote: ImapRemote, handle: thread::JoinHandle<Result<(), String>>) {
        remote.logout();
        // Close before waiting: only closing reliably ends the server's read
        // on Windows, where `shutdown` was seen not to reach it.
        drop(remote);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn connect_reads_capabilities() {
        let (remote, handle) = session(FULL_CAPS, vec![("LOGOUT", "* BYE\r\n{tag} OK bye\r\n")]);
        let caps = remote.capabilities();
        assert!(caps.idle && caps.r#move && caps.uidplus && caps.condstore && caps.gmail);
        assert!(caps.special_use);
        assert!(format!("{remote:?}").contains("ImapRemote"));
        finish(remote, handle);
    }

    #[test]
    fn login_rejection_is_auth_error() {
        let (port, handle) = start(
            GREETING,
            vec![(
                "LOGIN",
                "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n",
            )],
        );
        let err = ImapRemote::connect(
            &server(port),
            "me",
            &password(),
            ConnectOptions::default(),
            &CancelToken::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Auth);
        drop(handle);
    }

    #[test]
    fn bye_greeting_is_refused() {
        let (port, _handle) = start("* BYE too busy\r\n", vec![]);
        let err = ImapRemote::connect(
            &server(port),
            "me",
            &password(),
            ConnectOptions::default(),
            &CancelToken::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Network);
        assert!(err.message.contains("too busy"));
    }

    #[test]
    fn xoauth2_login_sends_the_bearer_payload() {
        let payload = crate::oauth::xoauth2_payload("me", "tok");
        let encoded = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(payload)
        };
        let expected: &'static str = Box::leak(format!("={encoded}").into_boxed_str());
        let (port, handle) = start(
            GREETING,
            vec![
                ("AUTHENTICATE XOAUTH2", "+ \r\n"),
                (expected, "{tag} OK welcome\r\n"),
                ("CAPABILITY", BASIC_CAPS),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let remote = ImapRemote::connect(
            &server(port),
            "me",
            &Secret::AccessToken("tok".into()),
            ConnectOptions::default(),
            &CancelToken::new(),
        )
        .unwrap();
        finish(remote, handle);
    }

    #[test]
    fn xoauth2_error_challenge_gets_an_empty_answer() {
        let auth = XOAuth2 {
            payload: "p".into(),
        };
        use imap::Authenticator;
        assert_eq!(auth.process(b""), "p");
        assert_eq!(auth.process(b"{\"status\":\"401\"}"), "");
    }

    #[test]
    fn cancelled_token_refuses_to_connect() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = ImapRemote::connect(
            &server(1),
            "me",
            &password(),
            ConnectOptions::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Cancelled);
    }

    #[test]
    fn list_folders_keeps_raw_names_and_attributes() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                (
                    "LIST \"\" \"*\"",
                    "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n\
                     * LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n\
                     * LIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\"\r\n\
                     * LIST () \"/\" \"Entw&APw-rfe\"\r\n\
                     {tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let folders = remote.list_folders().unwrap();
        assert_eq!(folders.len(), 4);
        assert_eq!(folders[1].name, "[Gmail]/Sent Mail");
        assert_eq!(folders[1].role, Some(mail_core::FolderRole::Sent));
        assert!(folders[1].attributes.contains(&"\\Sent".to_string()));
        assert!(!folders[2].selectable);
        assert_eq!(folders[3].name, "Entw&APw-rfe");
        assert_eq!(folders[3].display_name(), "Entwürfe");
        finish(remote, handle);
    }

    #[test]
    fn select_status_and_selection_cache() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                ("UID SEARCH ALL", "* SEARCH 9 3 5\r\n{tag} OK done\r\n"),
                // No second SELECT: INBOX is cached.
                ("UID SEARCH ALL", "* SEARCH\r\n{tag} OK done\r\n"),
                ("SELECT \"Missing\"", "{tag} NO Mailbox does not exist\r\n"),
                SELECT_INBOX,
                ("UID SEARCH ALL", "* SEARCH 1\r\n{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let status = remote.select("INBOX").unwrap();
        assert_eq!(
            status,
            FolderStatus {
                uid_validity: 7,
                uid_next: Some(12),
                exists: 3,
                highest_modseq: None,
            }
        );
        assert_eq!(remote.uids("INBOX").unwrap(), vec![3, 5, 9]);
        assert!(remote.uids("INBOX").unwrap().is_empty());
        let err = remote.select("Missing").unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::NotFound);
        assert_eq!(remote.uids("INBOX").unwrap(), vec![1]);
        finish(remote, handle);
    }

    #[test]
    fn condstore_select_reports_highest_modseq() {
        let (mut remote, handle) = session(
            FULL_CAPS,
            vec![
                (
                    "SELECT \"INBOX\" (CONDSTORE)",
                    "* 1 EXISTS\r\n* OK [UIDVALIDITY 1] x\r\n* OK [HIGHESTMODSEQ 90210] x\r\n{tag} OK done\r\n",
                ),
                (
                    "UID FETCH 4:5 (UID FLAGS MODSEQ) (CHANGEDSINCE 100)",
                    "* 1 FETCH (UID 4 FLAGS (\\Seen) MODSEQ (101))\r\n{tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        assert_eq!(remote.select("INBOX").unwrap().highest_modseq, Some(90210));
        let flags = remote.fetch_flags("INBOX", &[5, 4], Some(100)).unwrap();
        assert_eq!(
            flags,
            vec![RemoteFlags {
                uid: 4,
                flags: MessageFlags {
                    seen: true,
                    ..Default::default()
                },
                modseq: Some(101),
            }]
        );
        finish(remote, handle);
    }

    #[test]
    fn fetch_headers_parses_gmail_items_and_skips_unknown() {
        let (mut remote, handle) = session(
            FULL_CAPS,
            vec![
                ("SELECT \"INBOX\" (CONDSTORE)", "* OK [UIDVALIDITY 1] x\r\n{tag} OK done\r\n"),
                (
                    "UID FETCH 1:2,7 (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER] BODYSTRUCTURE X-GM-MSGID X-GM-THRID MODSEQ)",
                    "* 1 FETCH (UID 1 FLAGS (\\Seen \\Flagged) RFC822.SIZE 321 X-GM-MSGID 1278455344230334865 X-GM-THRID 1266894439832287888 MODSEQ (12) BODYSTRUCTURE (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 5 1 NIL NIL NIL NIL) BODY[HEADER] {16}\r\nSubject: hello\r\n)\r\n\
                     * 2 FETCH (FLAGS (\\Seen))\r\n\
                     * 3 FETCH (UID 2 FLAGS () RFC822.SIZE 9 BODYSTRUCTURE ((\"text\" \"plain\" NIL NIL NIL \"7bit\" 5 1 NIL NIL NIL NIL)(\"application\" \"pdf\" (\"name\" \"a.pdf\") NIL NIL \"base64\" 100 NIL (\"attachment\" (\"filename\" \"a.pdf\")) NIL NIL) \"mixed\" (\"boundary\" \"b\") NIL NIL NIL) BODY[HEADER] {4}\r\nX: y)\r\n\
                     {tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let headers = remote.fetch_headers("INBOX", &[7, 2, 1]).unwrap();
        assert_eq!(headers.len(), 2);
        let first = &headers[0];
        assert_eq!(first.uid, 1);
        assert!(first.flags.seen && first.flags.flagged);
        assert_eq!(first.size, 321);
        assert_eq!(first.gm_msgid, Some(1278455344230334865));
        assert_eq!(first.gm_thrid, Some(1266894439832287888));
        assert_eq!(first.modseq, Some(12));
        assert_eq!(first.header, b"Subject: hello\r\n");
        assert_eq!(first.has_attachments, Some(false));
        assert_eq!(headers[1].uid, 2);
        assert_eq!(headers[1].has_attachments, Some(true));
        assert!(remote.fetch_headers("INBOX", &[]).unwrap().is_empty());
        finish(remote, handle);
    }

    #[test]
    fn fetch_flags_without_condstore_ignores_changed_since() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                (
                    "UID FETCH 3 (UID FLAGS)",
                    "* 1 FETCH (UID 3 FLAGS (\\Answered \\Draft))\r\n{tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let flags = remote.fetch_flags("INBOX", &[3], Some(55)).unwrap();
        assert_eq!(flags.len(), 1);
        assert!(flags[0].flags.answered && flags[0].flags.draft);
        assert_eq!(flags[0].modseq, None);
        assert!(remote.fetch_flags("INBOX", &[], None).unwrap().is_empty());
        finish(remote, handle);
    }

    #[test]
    fn fetch_body_returns_raw_or_not_found() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                (
                    "UID FETCH 5 (UID BODY.PEEK[])",
                    "* 2 FETCH (UID 5 BODY[] {9}\r\nA: b\r\n\r\nx)\r\n{tag} OK done\r\n",
                ),
                ("UID FETCH 6 (UID BODY.PEEK[])", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        assert_eq!(remote.fetch_body("INBOX", 5).unwrap(), b"A: b\r\n\r\nx");
        let err = remote.fetch_body("INBOX", 6).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::NotFound);
        finish(remote, handle);
    }

    #[test]
    fn store_flags_adds_and_removes_silently() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                (
                    "UID STORE 1:3 +FLAGS.SILENT (\\Seen \\Flagged)",
                    "{tag} OK done\r\n",
                ),
                ("UID STORE 1:3 -FLAGS.SILENT (\\Draft)", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let add = MessageFlags {
            seen: true,
            flagged: true,
            ..Default::default()
        };
        let remove = MessageFlags {
            draft: true,
            ..Default::default()
        };
        remote
            .store_flags("INBOX", &[3, 1, 2], add, remove)
            .unwrap();
        // Nothing to do: no command sent.
        remote
            .store_flags(
                "INBOX",
                &[1],
                MessageFlags::default(),
                MessageFlags::default(),
            )
            .unwrap();
        remote.store_flags("INBOX", &[], add, remove).unwrap();
        finish(remote, handle);
    }

    #[test]
    fn move_uses_move_and_reads_copyuid() {
        let (mut remote, handle) = session(
            FULL_CAPS,
            vec![
                ("SELECT \"INBOX\" (CONDSTORE)", "{tag} OK done\r\n"),
                (
                    "UID MOVE 4,6 \"Archive\"",
                    "* OK [COPYUID 99 4,6 100:101] moved\r\n* 1 EXPUNGE\r\n* 1 EXPUNGE\r\n{tag} OK done\r\n",
                ),
                ("UID MOVE 8 \"Archive\"", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        assert_eq!(
            remote.move_messages("INBOX", &[6, 4], "Archive").unwrap(),
            Some(vec![101, 100])
        );
        assert_eq!(
            remote.move_messages("INBOX", &[8], "Archive").unwrap(),
            None
        );
        assert_eq!(
            remote.move_messages("INBOX", &[], "Archive").unwrap(),
            Some(vec![])
        );
        finish(remote, handle);
    }

    #[test]
    fn move_falls_back_to_copy_store_expunge() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                (
                    "UID COPY 2 \"Trash\"",
                    "{tag} OK [COPYUID 5 2 40] copied\r\n",
                ),
                ("UID STORE 2 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("UID EXPUNGE 2", "* 1 EXPUNGE\r\n{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        assert_eq!(
            remote.move_messages("INBOX", &[2], "Trash").unwrap(),
            Some(vec![40])
        );
        finish(remote, handle);
    }

    #[test]
    fn move_to_missing_folder_is_not_found() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                ("UID COPY 2 \"Nope\"", "{tag} NO [TRYCREATE] no\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let err = remote.move_messages("INBOX", &[2], "Nope").unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::NotFound);
        finish(remote, handle);
    }

    #[test]
    fn delete_with_and_without_uidplus() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                ("UID STORE 1 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("UID EXPUNGE 1", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        remote.delete_messages("INBOX", &[1]).unwrap();
        remote.delete_messages("INBOX", &[]).unwrap();
        finish(remote, handle);

        let (mut remote, handle) = session(
            "* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n",
            vec![
                SELECT_INBOX,
                ("UID STORE 1 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("UID SEARCH DELETED", "* SEARCH 1\r\n{tag} OK done\r\n"),
                ("EXPUNGE", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        remote.delete_messages("INBOX", &[1]).unwrap();
        finish(remote, handle);
    }

    #[test]
    fn expunge_without_uidplus_spares_other_deleted_messages() {
        let (mut remote, handle) = session(
            "* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n",
            vec![
                SELECT_INBOX,
                ("UID STORE 5 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("UID SEARCH DELETED", "* SEARCH 2 5 9\r\n{tag} OK done\r\n"),
                (
                    "UID STORE 2,9 -FLAGS.SILENT (\\Deleted)",
                    "{tag} OK done\r\n",
                ),
                ("EXPUNGE", "* 3 EXPUNGE\r\n{tag} OK done\r\n"),
                (
                    "UID STORE 2,9 +FLAGS.SILENT (\\Deleted)",
                    "{tag} OK done\r\n",
                ),
                // Move fallback (no MOVE): COPY, flag, guarded EXPUNGE.
                ("UID COPY 7 \"Trash\"", "{tag} OK copied\r\n"),
                ("UID STORE 7 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("UID SEARCH DELETED", "* SEARCH 4 7\r\n{tag} OK done\r\n"),
                ("UID STORE 4 -FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("EXPUNGE", "{tag} NO busy\r\n"),
                // Restored even though the EXPUNGE failed.
                ("UID STORE 4 +FLAGS.SILENT (\\Deleted)", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        remote.delete_messages("INBOX", &[5]).unwrap();
        let err = remote.move_messages("INBOX", &[7], "Trash").unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Protocol);
        finish(remote, handle);
    }

    #[test]
    fn find_uid_by_message_id_searches_the_bracketed_header() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                (
                    "UID SEARCH HEADER Message-ID \"<a@x>\"",
                    "* SEARCH 3 8\r\n{tag} OK done\r\n",
                ),
                (
                    "UID SEARCH HEADER Message-ID \"<b@x>\"",
                    "* SEARCH\r\n{tag} OK done\r\n",
                ),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        assert_eq!(
            remote.find_uid_by_message_id("INBOX", "a@x").unwrap(),
            Some(8)
        );
        assert_eq!(
            remote.find_uid_by_message_id("INBOX", "<b@x>").unwrap(),
            None
        );
        assert_eq!(remote.find_uid_by_message_id("INBOX", "  ").unwrap(), None);
        finish(remote, handle);
    }

    #[test]
    fn append_create_and_noop() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                ("APPEND \"Sent \\\"x\\\"\" (\\Seen)", "+ go\r\n"),
                ("=Subject: hi", "{tag} OK [APPENDUID 1 5] done\r\n"),
                ("CREATE \"Projects/New\"", "{tag} OK done\r\n"),
                ("NOOP", "* 4 EXISTS\r\n{tag} OK done\r\n"),
                ("CREATE \"Dup\"", "{tag} NO already exists\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let seen = MessageFlags {
            seen: true,
            ..Default::default()
        };
        remote.append("Sent \"x\"", seen, b"Subject: hi").unwrap();
        remote.create_folder("Projects/New").unwrap();
        remote.noop().unwrap();
        assert_eq!(
            remote.create_folder("Dup").unwrap_err().kind,
            RemoteErrorKind::Protocol
        );
        assert_eq!(
            remote.create_folder("bad\r\nname").unwrap_err().kind,
            RemoteErrorKind::Other
        );
        finish(remote, handle);
    }

    #[test]
    fn idle_reports_changes_and_timeouts() {
        let (mut remote, handle) = session(
            BASIC_CAPS,
            vec![
                SELECT_INBOX,
                ("IDLE", "+ idling\r\n* 4 EXISTS\r\n"),
                ("=DONE", "{tag} OK idle done\r\n"),
                ("IDLE", "+ idling\r\n"),
                ("=DONE", "{tag} OK idle done\r\n"),
                ("NOOP", "{tag} OK done\r\n"),
                ("LOGOUT", "{tag} OK bye\r\n"),
            ],
        );
        let cancel = CancelToken::new();
        assert_eq!(
            remote
                .idle("INBOX", Duration::from_secs(5), &cancel)
                .unwrap(),
            IdleOutcome::Changed
        );
        let started = Instant::now();
        assert_eq!(
            remote
                .idle("INBOX", Duration::from_millis(200), &cancel)
                .unwrap(),
            IdleOutcome::Timeout
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // The read timeout went back to the normal I/O timeout.
        assert_eq!(remote.socket.read_timeout().unwrap(), Some(IO_TIMEOUT));
        remote.noop().unwrap();
        finish(remote, handle);
    }

    #[test]
    fn idle_cancel_returns_stopped_quickly() {
        let (mut remote, _handle) =
            session(BASIC_CAPS, vec![SELECT_INBOX, ("IDLE", "+ idling\r\n")]);
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            trigger.cancel();
        });
        let started = Instant::now();
        assert_eq!(
            remote
                .idle("INBOX", Duration::from_secs(30), &cancel)
                .unwrap(),
            IdleOutcome::Stopped
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        canceller.join().unwrap();
        assert_eq!(
            remote
                .idle("INBOX", Duration::from_secs(1), &cancel)
                .unwrap(),
            IdleOutcome::Stopped
        );
    }

    #[test]
    fn session_cancel_interrupts_any_call() {
        let (port, _handle) = start(
            GREETING,
            vec![
                ("LOGIN", "{tag} OK hi\r\n"),
                ("CAPABILITY", BASIC_CAPS),
                // Never answers the NOOP.
            ],
        );
        let cancel = CancelToken::new();
        let mut remote = ImapRemote::connect(
            &server(port),
            "me",
            &password(),
            ConnectOptions::default(),
            &cancel,
        )
        .unwrap();
        let trigger = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            trigger.cancel();
        });
        let err = remote.noop().unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Cancelled);
        assert_eq!(remote.noop().unwrap_err().kind, RemoteErrorKind::Cancelled);
        remote.logout();
    }

    #[test]
    fn idle_without_capability_is_unsupported() {
        let (mut remote, handle) = session(
            "* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n",
            vec![("LOGOUT", "{tag} OK bye\r\n")],
        );
        let err = remote
            .idle("INBOX", Duration::from_secs(1), &CancelToken::new())
            .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Unsupported);
        finish(remote, handle);
    }

    #[test]
    fn imap_stream_tls_variant_reads_and_writes() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut tls = crate::tls::test_support::accept_tls(tcp).unwrap();
            let mut buf = [0u8; 4];
            tls.read_exact(&mut buf).unwrap();
            tls.write_all(&buf).unwrap();
            tls.flush().unwrap();
        });
        let tcp = tls::connect_tcp("127.0.0.1", port).unwrap();
        let tls = tls::connect_tls(
            tcp,
            "localhost",
            ConnectOptions {
                accept_invalid_certs: true,
            },
        )
        .unwrap();
        let mut stream = ImapStream::Tls(Box::new(tls));
        stream.write_all(b"ping").unwrap();
        stream.flush().unwrap();
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        stream.set_read_timeout(Some(Duration::ZERO)).unwrap();
        assert_eq!(
            stream.tcp().read_timeout().unwrap(),
            Some(Duration::from_millis(10))
        );
        stream.set_read_timeout(None).unwrap();
        assert_eq!(stream.tcp().read_timeout().unwrap(), Some(IO_TIMEOUT));
        server.join().unwrap();
    }

    #[test]
    fn parse_capabilities_is_case_insensitive() {
        let caps = parse_capabilities(["idle", "Move", "QRESYNC", "x-gm-ext-1", "AUTH=PLAIN"]);
        assert!(caps.idle && caps.r#move && caps.condstore && caps.gmail);
        assert!(!caps.uidplus && !caps.special_use);
        assert_eq!(parse_capabilities([]), Capabilities::default());
    }

    #[test]
    fn uid_set_compresses_ranges() {
        assert_eq!(uid_set(&[9, 1, 2, 3, 5, 3, 10]), "1:3,5,9:10");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
        assert_eq!(
            uid_set(&[u32::MAX - 1, u32::MAX]),
            format!("{}:{}", u32::MAX - 1, u32::MAX)
        );
    }

    #[test]
    fn uid_chunks_split_by_count() {
        assert_eq!(uid_chunks(&[5, 1, 2, 3, 4], 2), vec!["1:2", "3:4", "5"]);
        assert_eq!(uid_chunks(&[1, 2], 0), vec!["1", "2"]);
        assert!(uid_chunks(&[], 10).is_empty());
    }

    #[test]
    fn quote_escapes_and_rejects_line_breaks() {
        assert_eq!(quote("INBOX").unwrap(), "\"INBOX\"");
        assert_eq!(quote(r#"a"b\c"#).unwrap(), r#""a\"b\\c""#);
        assert_eq!(quote("a\nb").unwrap_err().kind, RemoteErrorKind::Other);
        assert_eq!(escape_quoted("x\"").unwrap(), "x\\\"");
    }

    #[test]
    fn name_attributes_render_as_atoms() {
        assert_eq!(name_attribute(&NameAttribute::NoSelect), "\\Noselect");
        assert_eq!(name_attribute(&NameAttribute::Junk), "\\Junk");
        assert_eq!(
            name_attribute(&NameAttribute::Extension("\\Important".into())),
            "\\Important"
        );
    }

    #[test]
    fn parse_responses_skips_garbage_lines() {
        let data = b"* 3 EXISTS\r\n* garbage (((\r\n* OK [UIDNEXT 4] x\r\nA1 OK done\r\n";
        let responses = parse_responses(data);
        assert_eq!(responses.len(), 3);
        let status = folder_status(&responses);
        assert_eq!(status.exists, 3);
        assert_eq!(status.uid_next, Some(4));
    }

    #[test]
    fn header_items_follow_capabilities() {
        let caps = Capabilities::default();
        assert_eq!(
            header_items(&caps),
            "UID FLAGS RFC822.SIZE BODY.PEEK[HEADER] BODYSTRUCTURE"
        );
        let caps = Capabilities {
            gmail: true,
            condstore: true,
            ..Default::default()
        };
        assert_eq!(
            header_items(&caps),
            "UID FLAGS RFC822.SIZE BODY.PEEK[HEADER] BODYSTRUCTURE X-GM-MSGID X-GM-THRID MODSEQ"
        );
    }

    fn fetch_attrs(line: &[u8]) -> Vec<AttributeValue<'_>> {
        match imap_proto::parser::parse_response(line).unwrap().1 {
            Response::Fetch(_, attrs) => attrs,
            other => panic!("not a fetch: {other:?}"),
        }
    }

    #[test]
    fn attribute_helpers_require_their_items() {
        let attrs = fetch_attrs(b"* 1 FETCH (FLAGS (\\Seen))\r\n");
        assert!(header_from_attributes(&attrs).is_none());
        assert!(flags_from_attributes(&attrs).is_none());
        let attrs = fetch_attrs(b"* 1 FETCH (UID 3 FLAGS (\\Seen) MODSEQ (5))\r\n");
        let flags = flags_from_attributes(&attrs).unwrap();
        assert_eq!((flags.uid, flags.modseq), (3, Some(5)));
        assert!(header_from_attributes(&attrs).is_none());
        assert!(body_from_attributes(&attrs, 3).is_none());
        let attrs = fetch_attrs(b"* 1 FETCH (UID 3 BODY[] {2}\r\nhi)\r\n");
        assert_eq!(body_from_attributes(&attrs, 3).unwrap(), b"hi");
        assert!(body_from_attributes(&attrs, 4).is_none());
        assert_eq!(fetch_uid(&attrs), Some(3));
        assert_eq!(fetch_flags(&attrs), None);
    }

    fn structure(text: &str) -> BodyStructure<'static> {
        let line = format!("* 1 FETCH (BODYSTRUCTURE {text})\r\n");
        let line: &'static [u8] = Box::leak(line.into_bytes().into_boxed_slice());
        match imap_proto::parser::parse_response(line).unwrap().1 {
            Response::Fetch(_, mut attrs) => match attrs.remove(0) {
                AttributeValue::BodyStructure(s) => s,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn has_attachments_rules() {
        // Plain text only.
        assert!(!has_attachments(&structure(
            r#"("text" "plain" NIL NIL NIL "7bit" 5 1 NIL NIL NIL NIL)"#
        )));
        // Leaf PDF without disposition counts.
        assert!(has_attachments(&structure(
            r#"(("text" "plain" NIL NIL NIL "7bit" 5 1 NIL NIL NIL NIL)("application" "pdf" NIL NIL NIL "base64" 10 NIL NIL NIL NIL) "mixed" NIL NIL NIL NIL)"#
        )));
        // Inline image referenced by Content-ID does not.
        assert!(!has_attachments(&structure(
            r#"(("text" "html" NIL NIL NIL "7bit" 5 1 NIL NIL NIL NIL)("image" "png" NIL "<logo@x>" NIL "base64" 10 NIL NIL NIL NIL) "related" NIL NIL NIL NIL)"#
        )));
        // Explicitly inline image without cid does not either.
        assert!(!has_attachments(&structure(
            r#"(("text" "plain" NIL NIL NIL "7bit" 5 1 NIL NIL NIL NIL)("image" "png" NIL NIL NIL "base64" 10 NIL ("inline" NIL) NIL NIL) "mixed" NIL NIL NIL NIL)"#
        )));
        // Text part marked as attachment counts.
        assert!(has_attachments(&structure(
            r#"(("text" "plain" NIL NIL NIL "7bit" 5 1 NIL NIL NIL NIL)("text" "csv" NIL NIL NIL "7bit" 5 1 NIL ("attachment" ("filename" "a.csv")) NIL NIL) "mixed" NIL NIL NIL NIL)"#
        )));
    }

    #[test]
    fn copy_uids_and_expand() {
        assert_eq!(
            expand_uid_set(&[
                UidSetMember::UidRange(RangeInclusive::new(3, 1)),
                UidSetMember::Uid(9)
            ]),
            vec![1, 2, 3, 9]
        );
        let responses = parse_responses(b"A1 OK [COPYUID 7 1:2 10:11] done\r\n");
        assert_eq!(copy_uids(&responses), Some(vec![(1, 10), (2, 11)]));
        let responses = parse_responses(b"A1 OK done\r\n");
        assert_eq!(copy_uids(&responses), None);
    }

    #[test]
    fn errors_map_to_kinds() {
        assert_eq!(
            no_kind("x", Some(&ResponseCode::TryCreate)),
            RemoteErrorKind::NotFound
        );
        assert_eq!(
            no_kind("Mailbox doesn't exist: X", None),
            RemoteErrorKind::NotFound
        );
        assert_eq!(no_kind("No such mailbox", None), RemoteErrorKind::NotFound);
        assert_eq!(no_kind("over quota", None), RemoteErrorKind::Protocol);
        assert_eq!(
            map_error(imap::Error::Io(io::Error::from(
                io::ErrorKind::ConnectionReset
            )))
            .kind,
            RemoteErrorKind::Network
        );
        assert_eq!(
            map_error(imap::Error::ConnectionLost).kind,
            RemoteErrorKind::Network
        );
        assert_eq!(
            map_error(imap::Error::Append).kind,
            RemoteErrorKind::Protocol
        );
        assert_eq!(
            map_login_error(imap::Error::ConnectionLost).kind,
            RemoteErrorKind::Network
        );
        assert_eq!(check_cancel(&CancelToken::new()), Ok(()));
    }
}
