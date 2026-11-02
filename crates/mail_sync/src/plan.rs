//! Pure sync decisions (no I/O): which server folders map to which storage
//! folders, in what order they sync, what one folder sync must fetch/remove,
//! queued-op payloads, and header → storage conversion.
//!
//! Everything here is table-tested; the worker only executes these plans.

use std::collections::HashSet;

use mail_core::{
    decode_imap_utf7, format_address_list, thread_key, FolderRole, FolderStatus, MessageFlags,
    ParsedMessage, RemoteFolder, RemoteHeader,
};
use serde::{Deserialize, Serialize};
use storage::{system, user_folder_path, Folder, FolderSyncState, NewRemoteMessage, PendingOp};

use crate::events::NewMailNotice;

/// Storage path + display name for a server folder, or `None` when the
/// folder is not synced (non-selectable, Flagged/Important virtual views, a
/// second All-Mail-like folder). The first folder of each system role claims
/// the system path (recorded in `used_system_paths`); later ones become user
/// folders. Gmail's All Mail (`All` role) takes the Archive path when no
/// Archive-role folder claimed it — callers must offer Archive-role folders
/// first (see [`plan_folders`]).
pub fn folder_path_for(
    remote: &RemoteFolder,
    used_system_paths: &mut HashSet<&'static str>,
) -> Option<(String, String)> {
    if !remote.selectable {
        return None;
    }
    let system_path = match remote.role {
        Some(FolderRole::Flagged | FolderRole::Important) => return None,
        Some(FolderRole::Inbox) => Some(system::INBOX),
        Some(FolderRole::Sent) => Some(system::SENT),
        Some(FolderRole::Drafts) => Some(system::DRAFTS),
        Some(FolderRole::Trash) => Some(system::TRASH),
        Some(FolderRole::Junk) => Some(system::JUNK),
        Some(FolderRole::Archive | FolderRole::All) => Some(system::ARCHIVE),
        Some(FolderRole::Custom) | None => None,
    };
    if let Some(path) = system_path {
        if used_system_paths.insert(path) {
            return Some((path.to_string(), String::new()));
        }
        // Syncing a second "everything" folder would duplicate the archive.
        if remote.role == Some(FolderRole::All) {
            return None;
        }
    }
    let display = user_display_name(remote);
    Some((user_folder_path(&display), display))
}

/// Full decoded name of a user folder: every path segment decoded from IMAP
/// modified UTF-7 and joined with `/`. A leading `INBOX` segment (Courier /
/// cPanel servers nest everything under it) is dropped.
fn user_display_name(remote: &RemoteFolder) -> String {
    let segments: Vec<String> = match remote.delimiter.as_deref().filter(|d| !d.is_empty()) {
        Some(delimiter) => remote.name.split(delimiter).map(decode_imap_utf7).collect(),
        None => vec![decode_imap_utf7(&remote.name)],
    };
    let skip = usize::from(segments.len() > 1 && segments[0].eq_ignore_ascii_case("inbox"));
    segments[skip..].join("/")
}

/// One storage folder to create or update from the server list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderUpsert {
    pub path: String,
    pub display_name: String,
    pub remote_name: String,
    pub role: Option<String>,
    /// The local folder at `path` pointed at another server mailbox: its
    /// locations and sync checkpoint are stale and must be cleared.
    pub reset: bool,
}

/// Changes that bring the local folder table in line with the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPlan {
    /// Only new or changed folders.
    pub upserts: Vec<FolderUpsert>,
    /// Local server-backed folders that vanished (or are no longer synced).
    pub deletes: Vec<i64>,
}

impl FolderPlan {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.deletes.is_empty()
    }
}

/// Diffs the server folder list against the local one. Local-only folders
/// (`remote_name == None`) are never touched.
pub fn plan_folders(remote: &[RemoteFolder], local: &[Folder]) -> FolderPlan {
    // Archive-role folders must claim the archive path before All Mail does.
    let mut ordered: Vec<&RemoteFolder> = remote.iter().collect();
    ordered.sort_by_key(|folder| folder.role == Some(FolderRole::All));

    let mut used_system = HashSet::new();
    let mut planned_paths = HashSet::new();
    let mut plan = FolderPlan::default();
    for folder in ordered {
        let Some((path, display_name)) = folder_path_for(folder, &mut used_system) else {
            continue;
        };
        if !planned_paths.insert(path.clone()) {
            continue; // two server folders decode to the same user path
        }
        let role = folder.role.map(|role| role.as_str().to_string());
        let existing = local.iter().find(|l| l.path == path);
        let unchanged = existing.is_some_and(|l| {
            l.display_name == display_name
                && l.remote_name.as_deref() == Some(folder.name.as_str())
                && l.role == role
        });
        if unchanged {
            continue;
        }
        plan.upserts.push(FolderUpsert {
            reset: existing
                .and_then(|l| l.remote_name.as_deref())
                .is_some_and(|name| name != folder.name),
            path,
            display_name,
            remote_name: folder.name.clone(),
            role,
        });
    }
    plan.deletes = local
        .iter()
        .filter(|l| l.remote_name.is_some() && !planned_paths.contains(&l.path))
        .map(|l| l.id)
        .collect();
    plan
}

/// Rank of a storage path in a sync cycle (lower first).
fn sync_rank(path: &str) -> u8 {
    match path {
        system::INBOX => 0,
        system::SENT => 1,
        system::DRAFTS => 2,
        system::ARCHIVE => 3,
        system::JUNK => 5,
        system::TRASH => 6,
        _ => 4,
    }
}

/// Server-backed folders in sync order: Inbox, Sent, Drafts, Archive/All
/// Mail, user folders, Junk, Trash (ties by id). Local-only folders are
/// dropped.
pub fn sync_order(folders: &[Folder]) -> Vec<Folder> {
    let mut ordered: Vec<Folder> = folders
        .iter()
        .filter(|f| f.remote_name.is_some())
        .cloned()
        .collect();
    ordered.sort_by_key(|f| (sync_rank(&f.path), f.id));
    ordered
}

/// What storage knows about a folder before syncing it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalFolderState {
    pub sync: FolderSyncState,
    /// UIDs with a stored location.
    pub uids: Vec<u32>,
}

/// Work for one folder sync.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderSyncPlan {
    /// UIDVALIDITY changed: clear every location first; nothing local is
    /// trusted.
    pub reset: bool,
    /// Known UIDs expunged on the server.
    pub removed: Vec<u32>,
    /// Headers to fetch, newest first.
    pub to_fetch: Vec<u32>,
    /// Known UIDs whose flags must be re-read.
    pub flags_check: Vec<u32>,
    /// CONDSTORE `CHANGEDSINCE` value for `flags_check`.
    pub flags_changed_since: Option<u64>,
}

/// Plans one folder sync. Only the newest `window` server UIDs are fetched
/// (older known messages are kept); UIDs in `in_flight` (referenced by queued
/// ops) are neither fetched nor flag-checked so a just-moved message is not
/// re-attached to its source folder and local flag edits are not reverted.
/// Flags are skipped entirely when both sides report the same modseq.
pub fn plan_folder(
    local: &LocalFolderState,
    remote: &FolderStatus,
    remote_uids: &[u32],
    window: u32,
    in_flight: &HashSet<u32>,
) -> FolderSyncPlan {
    let reset = local
        .sync
        .uid_validity
        .is_some_and(|validity| validity != remote.uid_validity);
    let known: HashSet<u32> = if reset {
        HashSet::new()
    } else {
        local.uids.iter().copied().collect()
    };
    let on_server: HashSet<u32> = remote_uids.iter().copied().collect();

    let mut removed: Vec<u32> = known
        .iter()
        .copied()
        .filter(|uid| !on_server.contains(uid))
        .collect();
    removed.sort_unstable();

    let mut sorted_remote = remote_uids.to_vec();
    sorted_remote.sort_unstable();
    sorted_remote.dedup();
    let window_start = sorted_remote.len().saturating_sub(window as usize);
    let to_fetch = sorted_remote[window_start..]
        .iter()
        .rev()
        .copied()
        .filter(|uid| !known.contains(uid) && !in_flight.contains(uid))
        .collect();

    let mut candidates: Vec<u32> = known
        .iter()
        .copied()
        .filter(|uid| on_server.contains(uid) && !in_flight.contains(uid))
        .collect();
    candidates.sort_unstable();
    let (flags_check, flags_changed_since) = if reset {
        (Vec::new(), None)
    } else {
        match (local.sync.highest_modseq, remote.highest_modseq) {
            (Some(ours), Some(theirs)) if ours == theirs => (Vec::new(), None),
            // A lower server value means the modseq space was reset: re-read all.
            (Some(ours), Some(theirs)) if ours < theirs => (candidates, Some(ours)),
            _ => (candidates, None),
        }
    };
    FolderSyncPlan {
        reset,
        removed,
        to_fetch,
        flags_check,
        flags_changed_since,
    }
}

/// A server-side action queued by the UI (`storage::PendingOp` payload).
/// Folder names are server (remote) names.
///
/// Every variant records where the message was when the op was queued:
/// `uid` plus the folder's `uid_validity` at that time, and the message's
/// `Message-ID` header (`message_id_hdr`, no angle brackets). Before running
/// an op the worker SELECTs the folder; when the UIDVALIDITY differs (or was
/// never recorded — payloads written before these fields existed decode
/// with `None`) the UID is re-resolved by Message-ID instead of trusted.
/// `uid == 0` means "unresolved": the message is in `folder` but its UID is
/// unknown (a move without COPYUID); it is found by Message-ID too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PendingAction {
    Flags {
        folder: String,
        uid: u32,
        add: MessageFlags,
        remove: MessageFlags,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uid_validity: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id_hdr: Option<String>,
    },
    Move {
        from: String,
        uid: u32,
        to: String,
        to_folder_id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uid_validity: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id_hdr: Option<String>,
    },
    Delete {
        folder: String,
        uid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uid_validity: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id_hdr: Option<String>,
    },
}

impl PendingAction {
    /// `pending_ops.kind` value.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Flags { .. } => "flags",
            Self::Move { .. } => "move",
            Self::Delete { .. } => "delete",
        }
    }

    /// JSON payload for `pending_ops.payload`.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parses a payload; `None` for anything this version did not write.
    pub fn decode(payload: &str) -> Option<Self> {
        serde_json::from_str(payload).ok()
    }

    /// The `(folder, uid)` the action operates on (`uid` 0 = unresolved).
    pub fn source(&self) -> (&str, u32) {
        match self {
            Self::Flags { folder, uid, .. } | Self::Delete { folder, uid, .. } => (folder, *uid),
            Self::Move { from, uid, .. } => (from, *uid),
        }
    }

    /// UIDVALIDITY of the source folder when `uid` was recorded.
    pub fn uid_validity(&self) -> Option<u32> {
        match self {
            Self::Flags { uid_validity, .. }
            | Self::Move { uid_validity, .. }
            | Self::Delete { uid_validity, .. } => *uid_validity,
        }
    }

    /// The message's `Message-ID` header, when known.
    pub fn message_id_hdr(&self) -> Option<&str> {
        match self {
            Self::Flags { message_id_hdr, .. }
            | Self::Move { message_id_hdr, .. }
            | Self::Delete { message_id_hdr, .. } => message_id_hdr.as_deref(),
        }
    }

    /// Points the action at a new location (the message was moved first).
    /// `new_uid` 0 / `uid_validity` `None` leave it unresolved.
    pub fn retarget(&mut self, folder: &str, new_uid: u32, new_validity: Option<u32>) {
        match self {
            Self::Flags {
                folder: f,
                uid,
                uid_validity,
                ..
            }
            | Self::Delete {
                folder: f,
                uid,
                uid_validity,
                ..
            }
            | Self::Move {
                from: f,
                uid,
                uid_validity,
                ..
            } => {
                *f = folder.to_string();
                *uid = new_uid;
                *uid_validity = new_validity;
            }
        }
    }
}

/// UIDs of `folder_remote_name` that queued ops still act on (unresolved
/// `uid == 0` entries excluded).
pub fn in_flight_uids(ops: &[PendingOp], folder_remote_name: &str) -> HashSet<u32> {
    ops.iter()
        .filter_map(|op| PendingAction::decode(&op.payload))
        .filter_map(|action| {
            let (folder, uid) = action.source();
            (folder == folder_remote_name && uid != 0).then_some(uid)
        })
        .collect()
}

/// Storage ids of messages with a queued flag edit: a sync must not
/// overwrite their local flags with the server's (not yet updated) ones,
/// wherever the op currently points (it may await a Message-ID lookup).
pub fn messages_with_pending_flags(ops: &[PendingOp]) -> HashSet<i64> {
    ops.iter()
        .filter(|op| {
            matches!(
                PendingAction::decode(&op.payload),
                Some(PendingAction::Flags { .. })
            )
        })
        .filter_map(|op| op.message_id)
        .collect()
}

/// Whether new mail found in this folder sync should be announced: only in
/// the Inbox, never on its first sync or after a UIDVALIDITY reset (that
/// would announce the whole mailbox).
pub fn should_announce(folder_path: &str, before: &FolderSyncState, reset: bool) -> bool {
    folder_path == system::INBOX && before.synced_at.is_some() && !reset
}

/// Whether a header stored by a folder sync is a new arrival in that folder.
/// Gmail shows one message under many labels: a sync pass may meet new mail
/// in All Mail just before the Inbox, so a merged row still counts when its
/// UID is above the folder's previous UIDNEXT. Plain IMAP only merges mail
/// moved by the UI, which never is an arrival.
pub fn is_new_arrival(
    created: bool,
    gm_msgid: Option<u64>,
    uid: u32,
    uid_next_before: Option<u32>,
) -> bool {
    created || (gm_msgid.is_some() && uid_next_before.is_some_and(|next| uid >= next))
}

/// Notification for a stored header: only for unread new arrivals (see
/// [`is_new_arrival`]) when the folder sync announces (see [`should_announce`]).
pub fn notice_for_new(
    message_id: i64,
    message: &NewRemoteMessage,
    arrived: bool,
    announce: bool,
) -> Option<NewMailNotice> {
    (arrived && announce && message.unread).then(|| NewMailNotice {
        message_id,
        sender: message.sender.clone(),
        subject: message.subject.clone(),
    })
}

/// Storage row for a fetched header (`parsed` = `parse_message(&header.header)`).
/// Body-derived fields (preview, plain text) stay empty until the body is
/// downloaded.
pub fn new_remote_message(header: &RemoteHeader, parsed: &ParsedMessage) -> NewRemoteMessage {
    let from = parsed.from.as_ref().or(parsed.sender.as_ref());
    let thread_id = match header.gm_thrid {
        Some(thread) => Some(format!("gm:{thread}")),
        None => thread_key(
            parsed.message_id.as_deref(),
            parsed.in_reply_to.as_deref(),
            &parsed.references,
        ),
    };
    let date_ts = parsed.date.unwrap_or(0);
    NewRemoteMessage {
        sender: from.map(|a| a.label().to_string()).unwrap_or_default(),
        sender_email: from.map(|a| a.email.clone()).unwrap_or_default(),
        subject: parsed.subject.clone(),
        preview: String::new(),
        plain_text: String::new(),
        to_addrs: format_address_list(&parsed.to),
        cc_addrs: format_address_list(&parsed.cc),
        reply_to: format_address_list(&parsed.reply_to),
        message_id_hdr: parsed.message_id.clone(),
        in_reply_to: parsed.in_reply_to.clone(),
        references_hdr: parsed.references.join(" "),
        thread_id,
        date_ts,
        received_at: received_at(date_ts),
        unread: !header.flags.seen,
        starred: header.flags.flagged,
        answered: header.flags.answered,
        has_attachment: header
            .has_attachments
            .unwrap_or_else(|| is_multipart_mixed(&header.header)),
        size: u64::from(header.size),
        gm_msgid: header.gm_msgid,
    }
}

/// Fallback attachment guess when the server sent no BODYSTRUCTURE: a
/// top-level `multipart/mixed` Content-Type usually carries attachments.
fn is_multipart_mixed(header: &[u8]) -> bool {
    let text = String::from_utf8_lossy(header).to_ascii_lowercase();
    let unfolded = text
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ");
    unfolded.lines().any(|line| {
        line.strip_prefix("content-type:")
            .is_some_and(|value| value.trim_start().starts_with("multipart/mixed"))
    })
}

/// UTC `YYYY-MM-DD HH:MM` for `messages.received_at`; empty for 0 (unknown
/// date). The UI formats `date_ts` itself; this is only a stable fallback.
pub fn received_at(date_ts: i64) -> String {
    if date_ts == 0 {
        return String::new();
    }
    let days = date_ts.div_euclid(86_400);
    let seconds = date_ts.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60
    )
}

/// Days since 1970-01-01 → proleptic Gregorian `(year, month, day)`
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::{parse_message, Address};
    use storage::is_system_path;

    fn remote(name: &str, attributes: &[&str]) -> RemoteFolder {
        RemoteFolder::new(
            name,
            Some("/"),
            attributes.iter().map(|a| a.to_string()).collect(),
        )
    }

    fn local(id: i64, path: &str, remote_name: Option<&str>, role: Option<&str>) -> Folder {
        Folder {
            id,
            account_id: 1,
            path: path.into(),
            display_name: if is_system_path(path) {
                String::new()
            } else {
                path.trim_start_matches("user:").into()
            },
            remote_name: remote_name.map(str::to_string),
            role: role.map(str::to_string),
        }
    }

    #[test]
    fn folder_path_for_maps_roles_and_user_folders() {
        type Case<'a> = (&'a str, &'a [&'a str], Option<(&'a str, &'a str)>);
        let cases: &[Case] = &[
            ("INBOX", &[], Some((system::INBOX, ""))),
            ("Sent", &["\\Sent"], Some((system::SENT, ""))),
            ("Drafts", &[], Some((system::DRAFTS, ""))),
            ("Lixeira", &[], Some((system::TRASH, ""))),
            ("Spam", &["\\Junk"], Some((system::JUNK, ""))),
            ("Archive", &["\\Archive"], Some((system::ARCHIVE, ""))),
            ("[Gmail]/Starred", &["\\Flagged"], None),
            ("[Gmail]/Important", &["\\Important"], None),
            ("[Gmail]", &["\\Noselect"], None),
            (
                "Clients/Acme",
                &[],
                Some(("user:Clients/Acme", "Clients/Acme")),
            ),
            ("INBOX/Work", &[], Some(("user:Work", "Work"))),
            ("Reuni&APU-es", &[], Some(("user:Reuniões", "Reuniões"))),
        ];
        for (name, attributes, expected) in cases {
            let mut used = HashSet::new();
            let got = folder_path_for(&remote(name, attributes), &mut used);
            let expected = expected.map(|(p, d)| (p.to_string(), d.to_string()));
            assert_eq!(got, expected, "{name}");
        }
    }

    #[test]
    fn first_folder_of_a_role_wins_and_all_mail_needs_free_archive() {
        let mut used = HashSet::new();
        assert_eq!(
            folder_path_for(&remote("Trash", &["\\Trash"]), &mut used)
                .unwrap()
                .0,
            system::TRASH
        );
        assert_eq!(
            folder_path_for(&remote("Old Trash", &["\\Trash"]), &mut used),
            Some(("user:Old Trash".into(), "Old Trash".into()))
        );
        let all = remote("[Gmail]/All Mail", &["\\All"]);
        assert_eq!(folder_path_for(&all, &mut used).unwrap().0, system::ARCHIVE);
        let mut with_archive: HashSet<&'static str> = [system::ARCHIVE].into();
        assert_eq!(folder_path_for(&all, &mut with_archive), None);
    }

    #[test]
    fn user_display_name_without_delimiter() {
        let folder = RemoteFolder::new("A.B", None, Vec::new());
        assert_eq!(user_display_name(&folder), "A.B");
        let dotted = RemoteFolder::new("INBOX.Clients.Acme", Some("."), Vec::new());
        assert_eq!(user_display_name(&dotted), "Clients/Acme");
    }

    #[test]
    fn plan_folders_creates_updates_resets_and_deletes() {
        let server = vec![
            remote("INBOX", &[]),
            remote("[Gmail]/All Mail", &["\\All"]),
            remote("[Gmail]/Sent Mail", &["\\Sent"]),
            remote("Work", &[]),
            remote("Work", &[]), // duplicate path ignored
            remote("[Gmail]/Starred", &["\\Flagged"]),
        ];
        let local = vec![
            local(1, system::INBOX, Some("INBOX"), Some("inbox")),
            local(2, system::SENT, Some("Sent"), Some("sent")),
            local(3, "user:Gone", Some("Gone"), None),
            local(4, "user:Local only", None, None),
        ];
        let plan = plan_folders(&server, &local);
        assert_eq!(plan.deletes, vec![3]);
        let paths: Vec<_> = plan.upserts.iter().map(|u| u.path.as_str()).collect();
        assert_eq!(paths, vec![system::SENT, "user:Work", system::ARCHIVE]);
        let sent = &plan.upserts[0];
        assert!(sent.reset, "Sent now points at another mailbox");
        assert_eq!(sent.remote_name, "[Gmail]/Sent Mail");
        assert_eq!(sent.role.as_deref(), Some("sent"));
        assert!(!plan.upserts[2].reset);
        assert_eq!(plan.upserts[2].role.as_deref(), Some("all"));
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_folders_prefers_archive_role_over_all_mail_regardless_of_order() {
        let server = vec![remote("All", &["\\All"]), remote("Archive", &["\\Archive"])];
        let plan = plan_folders(&server, &[]);
        assert_eq!(plan.upserts.len(), 1);
        assert_eq!(plan.upserts[0].remote_name, "Archive");
    }

    #[test]
    fn plan_folders_is_empty_when_in_sync() {
        let server = vec![remote("INBOX", &[]), remote("Work", &[])];
        let local = vec![
            local(1, system::INBOX, Some("INBOX"), Some("inbox")),
            local(2, "user:Work", Some("Work"), None),
        ];
        assert!(plan_folders(&server, &local).is_empty());
        assert!(FolderPlan::default().is_empty());
    }

    #[test]
    fn sync_order_follows_roles() {
        let folders = vec![
            local(1, system::TRASH, Some("Trash"), None),
            local(2, "user:B", Some("B"), None),
            local(3, system::JUNK, Some("Junk"), None),
            local(4, system::ARCHIVE, Some("All"), None),
            local(5, system::DRAFTS, Some("Drafts"), None),
            local(6, system::SENT, Some("Sent"), None),
            local(7, system::INBOX, Some("INBOX"), None),
            local(8, "user:A", Some("A"), None),
            local(9, "user:Local", None, None),
        ];
        let ids: Vec<i64> = sync_order(&folders).iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![7, 6, 5, 4, 2, 8, 3, 1]);
    }

    fn status(uid_validity: u32, modseq: Option<u64>) -> FolderStatus {
        FolderStatus {
            uid_validity,
            uid_next: Some(100),
            exists: 0,
            highest_modseq: modseq,
        }
    }

    fn state(validity: u32, modseq: Option<u64>, uids: &[u32]) -> LocalFolderState {
        LocalFolderState {
            sync: FolderSyncState {
                uid_validity: Some(validity),
                uid_next: None,
                highest_modseq: modseq,
                synced_at: Some(1),
            },
            uids: uids.to_vec(),
        }
    }

    #[test]
    fn plan_folder_table() {
        struct Case {
            name: &'static str,
            local: LocalFolderState,
            remote: FolderStatus,
            remote_uids: &'static [u32],
            window: u32,
            in_flight: &'static [u32],
            expected: FolderSyncPlan,
        }
        let cases = vec![
            Case {
                name: "first sync fetches newest first",
                local: LocalFolderState::default(),
                remote: status(1, Some(5)),
                remote_uids: &[1, 2, 3],
                window: 100,
                in_flight: &[],
                expected: FolderSyncPlan {
                    to_fetch: vec![3, 2, 1],
                    ..Default::default()
                },
            },
            Case {
                name: "window caps the fetch",
                local: LocalFolderState::default(),
                remote: status(1, None),
                remote_uids: &[5, 1, 9, 7],
                window: 2,
                in_flight: &[],
                expected: FolderSyncPlan {
                    to_fetch: vec![9, 7],
                    ..Default::default()
                },
            },
            Case {
                name: "window counts known messages",
                local: state(1, None, &[7]),
                remote: status(1, None),
                remote_uids: &[1, 5, 7, 9],
                window: 2,
                in_flight: &[],
                expected: FolderSyncPlan {
                    to_fetch: vec![9],
                    flags_check: vec![7],
                    ..Default::default()
                },
            },
            Case {
                name: "incremental with expunge and modseq",
                local: state(1, Some(10), &[1, 2, 3]),
                remote: status(1, Some(12)),
                remote_uids: &[1, 3, 4],
                window: 100,
                in_flight: &[],
                expected: FolderSyncPlan {
                    removed: vec![2],
                    to_fetch: vec![4],
                    flags_check: vec![1, 3],
                    flags_changed_since: Some(10),
                    ..Default::default()
                },
            },
            Case {
                name: "equal modseq skips flags",
                local: state(1, Some(10), &[1, 2]),
                remote: status(1, Some(10)),
                remote_uids: &[1, 2],
                window: 100,
                in_flight: &[],
                expected: FolderSyncPlan::default(),
            },
            Case {
                name: "lower server modseq re-reads everything",
                local: state(1, Some(10), &[1]),
                remote: status(1, Some(3)),
                remote_uids: &[1],
                window: 100,
                in_flight: &[],
                expected: FolderSyncPlan {
                    flags_check: vec![1],
                    ..Default::default()
                },
            },
            Case {
                name: "uidvalidity change resets",
                local: state(1, Some(10), &[1, 2]),
                remote: status(2, Some(10)),
                remote_uids: &[1, 2],
                window: 100,
                in_flight: &[],
                expected: FolderSyncPlan {
                    reset: true,
                    to_fetch: vec![2, 1],
                    ..Default::default()
                },
            },
            Case {
                name: "in-flight uids are neither fetched nor flag-checked",
                local: state(1, None, &[1, 2]),
                remote: status(1, None),
                remote_uids: &[1, 2, 3],
                window: 100,
                in_flight: &[2, 3],
                expected: FolderSyncPlan {
                    flags_check: vec![1],
                    ..Default::default()
                },
            },
        ];
        for case in cases {
            let in_flight: HashSet<u32> = case.in_flight.iter().copied().collect();
            let got = plan_folder(
                &case.local,
                &case.remote,
                case.remote_uids,
                case.window,
                &in_flight,
            );
            assert_eq!(got, case.expected, "{}", case.name);
        }
    }

    fn op(id: i64, action: &PendingAction) -> PendingOp {
        PendingOp {
            id,
            account_id: 1,
            kind: action.kind().into(),
            message_id: Some(id),
            payload: action.encode(),
            attempts: 0,
            last_error: None,
            created_at: 0,
        }
    }

    fn flags(folder: &str, uid: u32) -> PendingAction {
        PendingAction::Flags {
            folder: folder.into(),
            uid,
            add: MessageFlags {
                seen: true,
                ..Default::default()
            },
            remove: MessageFlags::default(),
            uid_validity: Some(3),
            message_id_hdr: Some("a@x".into()),
        }
    }

    fn mv(from: &str, uid: u32, to: &str) -> PendingAction {
        PendingAction::Move {
            from: from.into(),
            uid,
            to: to.into(),
            to_folder_id: 9,
            uid_validity: Some(3),
            message_id_hdr: None,
        }
    }

    fn del(folder: &str, uid: u32) -> PendingAction {
        PendingAction::Delete {
            folder: folder.into(),
            uid,
            uid_validity: None,
            message_id_hdr: Some("b@x".into()),
        }
    }

    #[test]
    fn pending_action_round_trips_and_retargets() {
        let mut actions = vec![flags("INBOX", 4), mv("INBOX", 5, "Trash"), del("Trash", 6)];
        let kinds: Vec<_> = actions.iter().map(PendingAction::kind).collect();
        assert_eq!(kinds, vec!["flags", "move", "delete"]);
        for action in &actions {
            assert_eq!(
                PendingAction::decode(&action.encode()).as_ref(),
                Some(action)
            );
        }
        assert!(actions[1].encode().contains(r#""type":"move""#));
        assert!(
            !actions[1].encode().contains("message_id_hdr"),
            "absent fields are not written"
        );
        assert_eq!(PendingAction::decode("{}"), None);
        assert_eq!(PendingAction::decode("garbage"), None);
        let sources: Vec<_> = actions.iter().map(|a| a.source()).collect();
        assert_eq!(sources, vec![("INBOX", 4), ("INBOX", 5), ("Trash", 6)]);
        let validities: Vec<_> = actions.iter().map(|a| a.uid_validity()).collect();
        assert_eq!(validities, vec![Some(3), Some(3), None]);
        let ids: Vec<_> = actions.iter().map(|a| a.message_id_hdr()).collect();
        assert_eq!(ids, vec![Some("a@x"), None, Some("b@x")]);
        for action in &mut actions {
            action.retarget("Archive", 77, Some(8));
            assert_eq!(action.source(), ("Archive", 77));
            assert_eq!(action.uid_validity(), Some(8));
        }
        actions[0].retarget("Archive", 0, None);
        assert_eq!(actions[0].source(), ("Archive", 0));
        assert_eq!(actions[0].uid_validity(), None);
        assert_eq!(actions[0].message_id_hdr(), Some("a@x"), "kept");
        assert!(matches!(&actions[1], PendingAction::Move { to, .. } if to == "Trash"));
    }

    #[test]
    fn payloads_without_validity_or_message_id_still_decode() {
        let old = r#"{"type":"move","from":"INBOX","uid":5,"to":"Trash","to_folder_id":2}"#;
        let action = PendingAction::decode(old).unwrap();
        assert_eq!(action.source(), ("INBOX", 5));
        assert_eq!(action.uid_validity(), None);
        assert_eq!(action.message_id_hdr(), None);
        let old = r#"{"type":"flags","folder":"INBOX","uid":4,
            "add":{"seen":true,"answered":false,"flagged":false,"deleted":false,"draft":false},
            "remove":{"seen":false,"answered":false,"flagged":false,"deleted":false,"draft":false}}"#;
        assert!(PendingAction::decode(old).is_some());
        let old = r#"{"type":"delete","folder":"Trash","uid":6}"#;
        assert_eq!(PendingAction::decode(old).unwrap().source(), ("Trash", 6));
    }

    #[test]
    fn in_flight_uids_filters_by_source_folder() {
        let ops = vec![
            op(1, &mv("INBOX", 5, "Trash")),
            op(2, &del("Trash", 6)),
            PendingOp {
                payload: "not json".into(),
                ..op(3, &del("INBOX", 1))
            },
            op(4, &flags("INBOX", 0)),
        ];
        assert_eq!(in_flight_uids(&ops, "INBOX"), [5].into());
        assert_eq!(in_flight_uids(&ops, "Trash"), [6].into());
        assert!(in_flight_uids(&ops, "Other").is_empty());
    }

    #[test]
    fn messages_with_pending_flags_ignores_moves_and_garbage() {
        let mut moved = op(2, &mv("INBOX", 5, "Trash"));
        moved.message_id = Some(20);
        let mut unknown = op(3, &flags("INBOX", 1));
        unknown.message_id = None;
        let ops = vec![
            op(1, &flags("INBOX", 0)),
            moved,
            unknown,
            PendingOp {
                payload: "not json".into(),
                ..op(4, &flags("INBOX", 1))
            },
        ];
        assert_eq!(messages_with_pending_flags(&ops), [1].into());
    }

    #[test]
    fn new_arrivals_include_gmail_merges_above_the_old_uidnext() {
        assert!(is_new_arrival(true, None, 1, Some(100)), "new row");
        assert!(
            is_new_arrival(false, Some(9), 100, Some(100)),
            "other label first"
        );
        assert!(!is_new_arrival(false, Some(9), 99, Some(100)), "older mail");
        assert!(!is_new_arrival(false, Some(9), 100, None), "no checkpoint");
        assert!(
            !is_new_arrival(false, None, 100, Some(100)),
            "plain IMAP move"
        );
    }

    #[test]
    fn announce_rules() {
        let synced = FolderSyncState {
            synced_at: Some(5),
            ..Default::default()
        };
        assert!(should_announce(system::INBOX, &synced, false));
        assert!(!should_announce(system::INBOX, &synced, true));
        assert!(!should_announce(
            system::INBOX,
            &FolderSyncState::default(),
            false
        ));
        assert!(!should_announce(system::SENT, &synced, false));

        let mut message = NewRemoteMessage {
            sender: "Ann".into(),
            subject: "Hi".into(),
            unread: true,
            ..Default::default()
        };
        assert_eq!(
            notice_for_new(7, &message, true, true),
            Some(NewMailNotice {
                message_id: 7,
                sender: "Ann".into(),
                subject: "Hi".into()
            })
        );
        assert_eq!(notice_for_new(7, &message, false, true), None);
        assert_eq!(notice_for_new(7, &message, true, false), None);
        message.unread = false;
        assert_eq!(notice_for_new(7, &message, true, true), None);
    }

    const HEADER: &[u8] = b"From: \"Ann Example\" <ann@example.com>\r\n\
To: bob@example.org, \"Doe, John\" <john@example.org>\r\n\
Cc: carol@example.net\r\n\
Reply-To: replies@example.com\r\n\
Subject: Lunch?\r\n\
Date: Tue, 14 Nov 2023 22:13:20 +0000\r\n\
Message-ID: <me@example.com>\r\n\
In-Reply-To: <parent@example.com>\r\n\
References: <root@example.com> <parent@example.com>\r\n\
Content-Type: multipart/mixed;\r\n boundary=\"x\"\r\n\r\n";

    #[test]
    fn new_remote_message_maps_headers_and_flags() {
        let header = RemoteHeader {
            uid: 3,
            flags: MessageFlags {
                seen: false,
                flagged: true,
                answered: true,
                ..Default::default()
            },
            size: 1234,
            gm_msgid: Some(99),
            header: HEADER.to_vec(),
            ..Default::default()
        };
        let parsed = parse_message(HEADER).unwrap();
        let message = new_remote_message(&header, &parsed);
        assert_eq!(message.sender, "Ann Example");
        assert_eq!(message.sender_email, "ann@example.com");
        assert_eq!(message.subject, "Lunch?");
        assert_eq!(
            message.to_addrs,
            "bob@example.org, \"Doe, John\" <john@example.org>"
        );
        assert_eq!(message.cc_addrs, "carol@example.net");
        assert_eq!(message.reply_to, "replies@example.com");
        assert_eq!(message.message_id_hdr.as_deref(), Some("me@example.com"));
        assert_eq!(message.in_reply_to.as_deref(), Some("parent@example.com"));
        assert_eq!(
            message.references_hdr,
            "root@example.com parent@example.com"
        );
        assert_eq!(message.thread_id.as_deref(), Some("root@example.com"));
        assert_eq!(message.date_ts, 1_700_000_000);
        assert_eq!(message.received_at, "2023-11-14 22:13");
        assert!(message.unread && message.starred && message.answered);
        assert!(message.has_attachment, "multipart/mixed sniff");
        assert_eq!(message.size, 1234);
        assert_eq!(message.gm_msgid, Some(99));
        assert!(message.preview.is_empty() && message.plain_text.is_empty());

        let gmail = RemoteHeader {
            gm_thrid: Some(42),
            has_attachments: Some(false),
            flags: MessageFlags {
                seen: true,
                ..Default::default()
            },
            ..header
        };
        let message = new_remote_message(&gmail, &parsed);
        assert_eq!(message.thread_id.as_deref(), Some("gm:42"));
        assert!(!message.has_attachment, "BODYSTRUCTURE wins over the sniff");
        assert!(!message.unread);
    }

    #[test]
    fn new_remote_message_tolerates_missing_headers() {
        let parsed = ParsedMessage {
            sender: Some(Address::new(None::<String>, "list@x.org")),
            ..Default::default()
        };
        let message = new_remote_message(&RemoteHeader::default(), &parsed);
        assert_eq!(message.sender, "list@x.org");
        assert_eq!(message.date_ts, 0);
        assert_eq!(message.received_at, "");
        assert_eq!(message.thread_id, None);
        assert!(!message.has_attachment);
        let empty = new_remote_message(&RemoteHeader::default(), &ParsedMessage::default());
        assert_eq!(
            (empty.sender.as_str(), empty.sender_email.as_str()),
            ("", "")
        );
    }

    #[test]
    fn multipart_mixed_sniff() {
        assert!(is_multipart_mixed(
            b"Content-Type: Multipart/Mixed; b=1\r\n"
        ));
        assert!(is_multipart_mixed(b"content-type:\n multipart/mixed\n"));
        assert!(!is_multipart_mixed(
            b"Content-Type: multipart/alternative\r\n"
        ));
        assert!(!is_multipart_mixed(b"X-Content-Type: multipart/mixed\r\n"));
        assert!(!is_multipart_mixed(b""));
    }

    #[test]
    fn received_at_and_civil_dates() {
        assert_eq!(received_at(0), "");
        assert_eq!(received_at(1), "1970-01-01 00:00");
        assert_eq!(received_at(951_782_400), "2000-02-29 00:00");
        assert_eq!(received_at(1_709_210_096), "2024-02-29 12:34");
        assert_eq!(received_at(-86_400), "1969-12-31 00:00");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
    }
}
