//! Protocol-neutral contracts between the sync engine and mail servers.
//!
//! The API is blocking on purpose: each account connection runs on its own
//! background thread, which keeps implementors (IMAP/SMTP in the future
//! `protocols` crate) free of any async runtime choice. Traits are
//! object-safe so the engine can hold `Box<dyn MailRemote>` and tests can swap
//! in [`crate::fake::FakeRemote`].

use std::fmt;
use std::time::Duration;

use crate::cancel::CancelToken;

use crate::flags::MessageFlags;
use crate::folder::RemoteFolder;

/// Coarse failure class; drives retry/back-off and what the UI tells the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteErrorKind {
    Auth,
    Network,
    Tls,
    Protocol,
    NotFound,
    Unsupported,
    Cancelled,
    /// The server's answer couldn't be parsed. The session can't be used any more, since
    /// the rest of that answer is unread, but this message may fetch fine another way.
    Unreadable,
    /// The message is over a limit and was rejected: trying again won't help.
    TooLarge,
    Other,
}

impl RemoteErrorKind {
    /// Stable lowercase name, used in logs and `Display`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Network => "network",
            Self::Tls => "tls",
            Self::Protocol => "protocol",
            Self::NotFound => "not found",
            Self::Unsupported => "unsupported",
            Self::Cancelled => "cancelled",
            Self::Unreadable => "unreadable",
            Self::TooLarge => "too large",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteError {
    pub kind: RemoteErrorKind,
    pub message: String,
}

impl RemoteError {
    pub fn new(kind: RemoteErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} error: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for RemoteError {}

pub type RemoteResult<T> = Result<T, RemoteError>;

/// Result of selecting a folder; `uid_validity` changes invalidate cached UIDs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderStatus {
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    pub exists: u32,
    /// Present when the server supports CONDSTORE.
    pub highest_modseq: Option<u64>,
}

/// What a header fetch returns per message: enough to list it without the body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteHeader {
    pub uid: u32,
    pub flags: MessageFlags,
    /// RFC822.SIZE in bytes.
    pub size: u32,
    /// Gmail `X-GM-MSGID` (stable across labels).
    pub gm_msgid: Option<u64>,
    /// Gmail `X-GM-THRID` (server-side thread id).
    pub gm_thrid: Option<u64>,
    pub modseq: Option<u64>,
    /// Raw RFC 822 header block (parse with [`crate::parse::parse_message`]).
    pub header: Vec<u8>,
    /// From BODYSTRUCTURE when the server provided it.
    pub has_attachments: Option<bool>,
}

/// Flag state of one message (incremental flag sync).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoteFlags {
    pub uid: u32,
    pub flags: MessageFlags,
    pub modseq: Option<u64>,
}

/// Server extensions the sync engine adapts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub idle: bool,
    pub r#move: bool,
    pub uidplus: bool,
    pub condstore: bool,
    /// `X-GM-EXT-1`.
    pub gmail: bool,
    pub special_use: bool,
}

/// How a push wait (IDLE) ended; used by the protocols crate's IDLE loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleOutcome {
    Changed,
    Timeout,
    Stopped,
}

/// One authenticated mailbox-server connection (IMAP-shaped). Methods taking
/// `folder` select it as needed, so callers never track selection state.
pub trait MailRemote: Send {
    fn capabilities(&self) -> Capabilities;
    fn list_folders(&mut self) -> RemoteResult<Vec<RemoteFolder>>;
    /// EXAMINE/SELECT; later calls act on it.
    fn select(&mut self, folder: &str) -> RemoteResult<FolderStatus>;
    /// All UIDs (`UID SEARCH ALL`), ascending.
    fn uids(&mut self, folder: &str) -> RemoteResult<Vec<u32>>;
    /// Unknown UIDs are skipped, not errors (they may have been expunged).
    fn fetch_headers(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<Vec<RemoteHeader>>;

    /// [`Self::fetch_headers`] without the message structure (`has_attachments` stays
    /// `None`), for when that is what the server's answer can't be parsed for.
    fn fetch_headers_plain(
        &mut self,
        folder: &str,
        uids: &[u32],
    ) -> RemoteResult<Vec<RemoteHeader>> {
        self.fetch_headers(folder, uids)
    }
    /// With `changed_since` (CONDSTORE), only messages whose modseq is greater.
    fn fetch_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        changed_since: Option<u64>,
    ) -> RemoteResult<Vec<RemoteFlags>>;
    /// UID of the message whose `Message-ID` header is `message_id` (with or
    /// without angle brackets) in `folder`, selecting it; the highest UID
    /// when several match. Used to re-find a message after its cached UID
    /// went stale (UIDVALIDITY change, move without COPYUID). The default
    /// (servers that cannot search) reports `None`.
    fn find_uid_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> RemoteResult<Option<u32>> {
        let _ = (folder, message_id);
        Ok(None)
    }
    /// Full raw message via `BODY.PEEK[]`; must not set `\Seen`.
    fn fetch_body(&mut self, folder: &str, uid: u32) -> RemoteResult<Vec<u8>>;
    /// Only `true` fields of `add` / `remove` are applied.
    fn store_flags(
        &mut self,
        folder: &str,
        uids: &[u32],
        add: MessageFlags,
        remove: MessageFlags,
    ) -> RemoteResult<()>;
    /// Returns the new UIDs (same order) when the server reports COPYUID.
    fn move_messages(
        &mut self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> RemoteResult<Option<Vec<u32>>>;
    /// `\Deleted` + `UID EXPUNGE`. Without UIDPLUS implementors must not
    /// expunge other messages already flagged `\Deleted` (see the IMAP
    /// client's guarded `EXPUNGE`).
    fn delete_messages(&mut self, folder: &str, uids: &[u32]) -> RemoteResult<()>;
    fn append(&mut self, folder: &str, flags: MessageFlags, raw: &[u8]) -> RemoteResult<()>;
    fn create_folder(&mut self, name: &str) -> RemoteResult<()>;
    /// Keep-alive and a cheap way to receive pending untagged updates.
    fn noop(&mut self) -> RemoteResult<()>;
    /// Best effort; never fails.
    fn logout(&mut self);
    /// Waits for changes in `folder` (IMAP IDLE) until something changes,
    /// `timeout` passes or `cancel` fires. Servers without IDLE return
    /// `Unsupported`, and the engine falls back to polling.
    fn idle(
        &mut self,
        folder: &str,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> RemoteResult<IdleOutcome> {
        let _ = (folder, timeout, cancel);
        Err(RemoteError::new(
            RemoteErrorKind::Unsupported,
            "IDLE is not supported",
        ))
    }
}

/// Opens connections for one account. Implemented by the protocol clients
/// (IMAP/SMTP, with password or OAuth); the sync engine only sees this trait.
pub trait MailConnector: Send + Sync {
    /// Connects and authenticates an IMAP-like session. `cancel` interrupts
    /// blocking reads on the connection for its whole lifetime.
    fn connect(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailRemote>>;
    /// Connects an SMTP-like submission session.
    fn connect_sender(&self, cancel: &CancelToken) -> RemoteResult<Box<dyn MailSender>>;
    /// Whether the server files sent mail itself (Gmail), so the client must
    /// not APPEND a copy to the Sent folder.
    fn saves_sent_copy(&self) -> bool {
        false
    }
}

/// What a failed submission means for the message (drives the outbox
/// state machine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SendFailure {
    /// Certainly not accepted and likely to work later: connection / TLS /
    /// auth failures before the message data, `4xx` replies. Retried with
    /// back-off without using up the message's attempts.
    Transient,
    /// Rejected by the server (`5xx`) or unsendable (bad address).
    Permanent,
    /// The connection broke after the message data started, so the server
    /// may have accepted it. Never resent automatically (duplicate risk).
    Uncertain,
}

/// A failed [`MailSender::send`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendError {
    pub failure: SendFailure,
    pub error: RemoteError,
}

impl SendError {
    pub fn transient(error: RemoteError) -> Self {
        Self {
            failure: SendFailure::Transient,
            error,
        }
    }

    pub fn permanent(error: RemoteError) -> Self {
        Self {
            failure: SendFailure::Permanent,
            error,
        }
    }

    pub fn uncertain(error: RemoteError) -> Self {
        Self {
            failure: SendFailure::Uncertain,
            error,
        }
    }
}

/// Errors raised before any message data was sent: connection-level kinds
/// (`Auth`, `Network`, `Tls`, `Cancelled`) are transient, the rest
/// permanent.
impl From<RemoteError> for SendError {
    fn from(error: RemoteError) -> Self {
        match error.kind {
            RemoteErrorKind::Auth
            | RemoteErrorKind::Network
            | RemoteErrorKind::Tls
            | RemoteErrorKind::Cancelled
            | RemoteErrorKind::Unreadable => Self::transient(error),
            RemoteErrorKind::Protocol
            | RemoteErrorKind::NotFound
            | RemoteErrorKind::Unsupported
            | RemoteErrorKind::TooLarge
            | RemoteErrorKind::Other => Self::permanent(error),
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for SendError {}

/// Message submission (SMTP-shaped). `raw` is sent as-is; `recipients` is the
/// envelope (see [`crate::compose::envelope_recipients`]). Implementors must
/// classify failures honestly: [`SendFailure::Uncertain`] whenever the
/// server may already have accepted the message.
pub trait MailSender: Send {
    fn send(&mut self, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), SendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_idle_and_search_are_unsupported() {
        struct Minimal;
        impl MailRemote for Minimal {
            fn capabilities(&self) -> Capabilities {
                Capabilities::default()
            }
            fn list_folders(&mut self) -> RemoteResult<Vec<RemoteFolder>> {
                Ok(Vec::new())
            }
            fn select(&mut self, _: &str) -> RemoteResult<FolderStatus> {
                Ok(FolderStatus::default())
            }
            fn uids(&mut self, _: &str) -> RemoteResult<Vec<u32>> {
                Ok(Vec::new())
            }
            fn fetch_headers(&mut self, _: &str, _: &[u32]) -> RemoteResult<Vec<RemoteHeader>> {
                Ok(Vec::new())
            }
            fn fetch_flags(
                &mut self,
                _: &str,
                _: &[u32],
                _: Option<u64>,
            ) -> RemoteResult<Vec<RemoteFlags>> {
                Ok(Vec::new())
            }
            fn fetch_body(&mut self, _: &str, _: u32) -> RemoteResult<Vec<u8>> {
                Ok(Vec::new())
            }
            fn store_flags(
                &mut self,
                _: &str,
                _: &[u32],
                _: MessageFlags,
                _: MessageFlags,
            ) -> RemoteResult<()> {
                Ok(())
            }
            fn move_messages(
                &mut self,
                _: &str,
                _: &[u32],
                _: &str,
            ) -> RemoteResult<Option<Vec<u32>>> {
                Ok(None)
            }
            fn delete_messages(&mut self, _: &str, _: &[u32]) -> RemoteResult<()> {
                Ok(())
            }
            fn append(&mut self, _: &str, _: MessageFlags, _: &[u8]) -> RemoteResult<()> {
                Ok(())
            }
            fn create_folder(&mut self, _: &str) -> RemoteResult<()> {
                Ok(())
            }
            fn noop(&mut self) -> RemoteResult<()> {
                Ok(())
            }
            fn logout(&mut self) {}
        }
        let err = Minimal
            .idle("INBOX", Duration::from_secs(1), &CancelToken::new())
            .unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Unsupported);
        assert_eq!(
            Minimal.find_uid_by_message_id("INBOX", "<a@x>").unwrap(),
            None,
            "default cannot search"
        );
    }

    #[test]
    fn send_errors_classify_by_kind() {
        use RemoteErrorKind::*;
        for kind in [Auth, Network, Tls, Cancelled] {
            let err = SendError::from(RemoteError::new(kind, "x"));
            assert_eq!(err.failure, SendFailure::Transient, "{kind:?}");
        }
        for kind in [Protocol, NotFound, Unsupported, Other] {
            let err = SendError::from(RemoteError::new(kind, "x"));
            assert_eq!(err.failure, SendFailure::Permanent, "{kind:?}");
        }
        let err = SendError::uncertain(RemoteError::new(Network, "reset"));
        assert_eq!(err.failure, SendFailure::Uncertain);
        assert_eq!(err.to_string(), "network error: reset");
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert_eq!(boxed.to_string(), "network error: reset");
    }

    #[test]
    fn error_display_and_source() {
        let err = RemoteError::new(RemoteErrorKind::NotFound, "no such folder");
        assert_eq!(err.to_string(), "not found error: no such folder");
        let boxed: Box<dyn std::error::Error> = Box::new(err.clone());
        assert_eq!(boxed.to_string(), err.to_string());
    }

    #[test]
    fn kind_names_are_distinct() {
        let kinds = [
            RemoteErrorKind::Auth,
            RemoteErrorKind::Network,
            RemoteErrorKind::Tls,
            RemoteErrorKind::Protocol,
            RemoteErrorKind::NotFound,
            RemoteErrorKind::Unsupported,
            RemoteErrorKind::Cancelled,
            RemoteErrorKind::Other,
        ];
        let mut names: Vec<_> = kinds.iter().map(|k| k.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), kinds.len());
    }

    #[test]
    fn traits_are_object_safe_and_send() {
        fn assert_send<T: Send + ?Sized>() {}
        assert_send::<Box<dyn MailRemote>>();
        assert_send::<Box<dyn MailSender>>();
    }
}
