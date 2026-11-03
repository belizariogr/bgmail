//! Optimistic mailbox actions run on the UI's own `storage::Database`.
//!
//! Each function applies the change locally right away and, for server
//! accounts, queues the matching server op (`pending_ops`, payload =
//! [`PendingAction`]). When the outcome says `needs_sync`, the caller sends
//! `Command::ProcessQueue` to the account's worker. `local` accounts only get
//! the local change.
//!
//! Ops are queued *before* the local write so a sync running concurrently
//! already treats the UIDs as in flight and never reverts the edit.

use mail_core::MessageFlags;
use storage::{
    is_manual_move_destination_forbidden, parse_folders_csv, system, Account, Database,
    MessageDetail, PROVIDER_LOCAL,
};

use crate::plan::PendingAction;

/// What an action did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionOutcome {
    /// The local database changed (reload the list).
    pub changed: bool,
    /// A server op was queued (send `Command::ProcessQueue`).
    pub needs_sync: bool,
    /// Nothing happened: the server has no folder to move it to.
    pub no_destination: bool,
}

impl ActionOutcome {
    const LOCAL: Self = Self {
        changed: true,
        needs_sync: false,
        no_destination: false,
    };
}

/// The message and its account, or `None` when either is gone.
fn context(db: &Database, message_id: i64) -> rusqlite::Result<Option<(MessageDetail, Account)>> {
    let Some(message) = db.get_message(message_id)? else {
        return Ok(None);
    };
    Ok(db
        .get_account(message.account_id)?
        .map(|account| (message, account)))
}

fn is_local(account: &Account) -> bool {
    account.provider == PROVIDER_LOCAL
}

/// Rounds of [`move_to_folder`] / [`delete_permanently`] retried when the
/// worker changed a queued op between our read and our compare-and-swap.
const CAS_ROUNDS: usize = 8;

/// A queued op as read, so it can be rewritten with a compare-and-swap.
struct QueuedMove {
    id: i64,
    kind: String,
    payload: String,
    action: PendingAction,
}

/// Queued Move ops of the message (it is in flight), oldest first.
fn pending_moves(
    db: &Database,
    account_id: i64,
    message_id: i64,
) -> rusqlite::Result<Vec<QueuedMove>> {
    Ok(db
        .pending_ops(account_id)?
        .into_iter()
        .filter(|op| op.message_id == Some(message_id))
        .filter_map(|op| {
            PendingAction::decode(&op.payload)
                .filter(|action| matches!(action, PendingAction::Move { .. }))
                .map(|action| QueuedMove {
                    id: op.id,
                    kind: op.kind,
                    payload: op.payload,
                    action,
                })
        })
        .collect())
}

/// The server folder of a message with neither a location nor a queued move:
/// a move that reported no COPYUID left it there until a sync re-attaches it.
/// Ops go there with UID 0, found by Message-ID when they run. `None` without
/// a Message-ID, which could never resolve.
fn unresolved_folder(
    db: &Database,
    account_id: i64,
    message: &MessageDetail,
) -> rusqlite::Result<Option<String>> {
    if message.message_id_hdr.is_none() {
        return Ok(None);
    }
    let folders = db.list_folders(account_id)?;
    Ok(primary_folders(message).into_iter().find_map(|path| {
        folders
            .iter()
            .find(|folder| folder.path == *path)
            .and_then(|folder| folder.remote_name.clone())
    }))
}

/// Where a queued op for the message must act: `(folder, uid, uid_validity)`
/// of the first server location; for a message in flight, the source of its
/// last queued move (the worker re-points the op once the move reports the
/// new UID); else its [`unresolved_folder`].
fn flags_target(
    db: &Database,
    account_id: i64,
    message: &MessageDetail,
) -> rusqlite::Result<Option<(String, u32, Option<u32>)>> {
    let message_id = message.id;
    if let Some(location) = db
        .message_locations(message_id)?
        .into_iter()
        .find(|l| l.remote_name.is_some())
    {
        let validity = db.folder_sync_state(location.folder_id)?.uid_validity;
        return Ok(location
            .remote_name
            .map(|name| (name, location.uid, validity)));
    }
    if let Some(queued) = pending_moves(db, account_id, message_id)?.pop() {
        let (folder, uid) = queued.action.source();
        return Ok(Some((
            folder.to_string(),
            uid,
            queued.action.uid_validity(),
        )));
    }
    Ok(unresolved_folder(db, account_id, message)?.map(|folder| (folder, 0, None)))
}

/// Queues a flag change for one location of the message; whether one was
/// queued. Reading the target and queueing are one transaction: otherwise the
/// worker could finish a move in between, and an op pointing at the expunged
/// source would "succeed" without flagging anything.
fn enqueue_flags(
    db: &Database,
    account_id: i64,
    message: &MessageDetail,
    add: MessageFlags,
    remove: MessageFlags,
) -> rusqlite::Result<bool> {
    db.atomically(|| {
        let Some((folder, uid, uid_validity)) = flags_target(db, account_id, message)? else {
            return Ok(false);
        };
        let action = PendingAction::Flags {
            folder,
            uid,
            add,
            remove,
            uid_validity,
            message_id_hdr: message.message_id_hdr.clone(),
        };
        db.enqueue_op(
            account_id,
            action.kind(),
            Some(message.id),
            &action.encode(),
        )?;
        Ok(true)
    })
}

/// `(add, remove)` masks setting or clearing one flag.
fn flag_change(mask: MessageFlags, set: bool) -> (MessageFlags, MessageFlags) {
    if set {
        (mask, MessageFlags::default())
    } else {
        (MessageFlags::default(), mask)
    }
}

/// Marks the message read or unread (`\Seen`).
pub fn set_read(db: &Database, message_id: i64, read: bool) -> rusqlite::Result<ActionOutcome> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(ActionOutcome::default());
    };
    if message.unread != read {
        return Ok(ActionOutcome::default());
    }
    let mut needs_sync = false;
    if !is_local(&account) {
        let seen = MessageFlags {
            seen: true,
            ..Default::default()
        };
        let (add, remove) = flag_change(seen, read);
        needs_sync = enqueue_flags(db, account.id, &message, add, remove)?;
    }
    db.set_message_unread(message_id, !read)?;
    Ok(ActionOutcome {
        changed: true,
        needs_sync,
        no_destination: false,
    })
}

/// Stars or unstars the message (`\Flagged`).
pub fn set_starred(
    db: &Database,
    message_id: i64,
    starred: bool,
) -> rusqlite::Result<ActionOutcome> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(ActionOutcome::default());
    };
    if message.starred == starred {
        return Ok(ActionOutcome::default());
    }
    let mut needs_sync = false;
    if !is_local(&account) {
        let flagged = MessageFlags {
            flagged: true,
            ..Default::default()
        };
        let (add, remove) = flag_change(flagged, starred);
        needs_sync = enqueue_flags(db, account.id, &message, add, remove)?;
    }
    db.toggle_message_starred(message_id)?;
    Ok(ActionOutcome {
        changed: true,
        needs_sync,
        no_destination: false,
    })
}

/// Non-flagged folders the message is shown in.
fn primary_folders(message: &MessageDetail) -> Vec<String> {
    parse_folders_csv(&message.folders_csv)
        .into_iter()
        .filter(|path| path != system::FLAGGED)
        .collect()
}

/// Runs `round` until it completes without losing a compare-and-swap race
/// against the worker (bounded; the last round's outcome wins).
fn with_cas_retry(
    mut round: impl FnMut() -> rusqlite::Result<Option<ActionOutcome>>,
) -> rusqlite::Result<ActionOutcome> {
    for _ in 1..CAS_ROUNDS {
        if let Some(outcome) = round()? {
            return Ok(outcome);
        }
    }
    Ok(round()?.unwrap_or(ActionOutcome {
        changed: true,
        needs_sync: true,
        no_destination: false,
    }))
}

/// Moves the message to `target_path` (a folder of its account). Server
/// accounts: the target needs a server mailbox; every location outside the
/// target is detached and gets a queued Move. Gmail-style "All Mail"
/// locations stay attached unless the target is Trash/Junk (All Mail keeps
/// every non-trash message). Moves still queued for the message are
/// re-pointed at the new target (or cancelled when it is their source).
/// Queued ops are rewritten with compare-and-swap: if the worker completed
/// or re-pointed one meanwhile, the whole decision is taken again from the
/// fresh state.
pub fn move_to_folder(
    db: &Database,
    message_id: i64,
    target_path: &str,
) -> rusqlite::Result<ActionOutcome> {
    with_cas_retry(|| move_to_folder_round(db, message_id, target_path))
}

/// One attempt of [`move_to_folder`]; `None` = lost a race, retry.
fn move_to_folder_round(
    db: &Database,
    message_id: i64,
    target_path: &str,
) -> rusqlite::Result<Option<ActionOutcome>> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(Some(ActionOutcome::default()));
    };
    if is_manual_move_destination_forbidden(target_path)
        || primary_folders(&message) == [target_path]
    {
        return Ok(Some(ActionOutcome::default()));
    }
    if is_local(&account) {
        db.move_message_to_folder(message_id, target_path)?;
        return Ok(Some(ActionOutcome::LOCAL));
    }
    let folders = db.list_folders(account.id)?;
    let Some(target) = folders
        .iter()
        .find(|f| f.path == target_path && f.remote_name.is_some())
    else {
        return Ok(Some(ActionOutcome {
            no_destination: true,
            ..Default::default()
        }));
    };
    let to = target.remote_name.clone().unwrap_or_default();
    let removes_everywhere = matches!(target_path, system::TRASH | system::JUNK);
    let all_mail_ids: Vec<i64> = folders
        .iter()
        .filter(|f| f.role.as_deref() == Some("all"))
        .map(|f| f.id)
        .collect();

    let mut needs_sync = false;
    let queued_moves = pending_moves(db, account.id, message_id)?;
    let in_flight = !queued_moves.is_empty();
    // Moves still queued for this message now go to the new target.
    for queued in queued_moves {
        let PendingAction::Move {
            from,
            uid,
            uid_validity,
            message_id_hdr,
            ..
        } = queued.action
        else {
            continue;
        };
        if from == to {
            // Back where the server still has it: cancel the move and
            // re-attach the location so later actions see it again.
            if !db.complete_op_if_unchanged(queued.id, &queued.kind, &queued.payload)? {
                return Ok(None);
            }
            if uid != 0 {
                db.attach_location(message_id, target.id, uid)?;
            }
            continue;
        }
        let retargeted = PendingAction::Move {
            from,
            uid,
            to: to.clone(),
            to_folder_id: target.id,
            uid_validity,
            message_id_hdr,
        };
        if !db.update_op_if_unchanged(
            queued.id,
            &queued.kind,
            &queued.payload,
            retargeted.kind(),
            &retargeted.encode(),
        )? {
            return Ok(None);
        }
        needs_sync = true;
    }
    let locations = db.message_locations(message_id)?;
    if locations.is_empty() && !in_flight {
        if let Some(from) = unresolved_folder(db, account.id, &message)?.filter(|from| *from != to)
        {
            let action = PendingAction::Move {
                from,
                uid: 0,
                to: to.clone(),
                to_folder_id: target.id,
                uid_validity: None,
                message_id_hdr: message.message_id_hdr.clone(),
            };
            db.enqueue_op(
                account.id,
                action.kind(),
                Some(message_id),
                &action.encode(),
            )?;
            needs_sync = true;
        }
    }
    let has_other_than_all = locations
        .iter()
        .any(|l| l.folder_id != target.id && !all_mail_ids.contains(&l.folder_id));
    for location in locations.iter().filter(|l| l.folder_id != target.id) {
        let keep_in_all_mail =
            all_mail_ids.contains(&location.folder_id) && !removes_everywhere && has_other_than_all;
        if keep_in_all_mail {
            continue;
        }
        let Some(from) = location.remote_name.clone() else {
            continue;
        };
        let action = PendingAction::Move {
            from,
            uid: location.uid,
            to: to.clone(),
            to_folder_id: target.id,
            uid_validity: db.folder_sync_state(location.folder_id)?.uid_validity,
            message_id_hdr: message.message_id_hdr.clone(),
        };
        db.enqueue_op(
            account.id,
            action.kind(),
            Some(message_id),
            &action.encode(),
        )?;
        db.detach_location(message_id, location.folder_id)?;
        needs_sync = true;
    }
    db.move_message_to_folder(message_id, target_path)?;
    Ok(Some(ActionOutcome {
        changed: true,
        needs_sync,
        no_destination: false,
    }))
}

/// Moves to the Archive folder (Gmail: All Mail, which sync maps there). A message
/// with labels only leaves the Inbox and keeps them, as in Gmail (user decision).
pub fn archive(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    if let Some(outcome) = archive_keeping_labels(db, message_id)? {
        return Ok(outcome);
    }
    move_to_folder(db, message_id, system::ARCHIVE)
}

/// [`archive`] for a message of an All Mail account that also has labels; `None`
/// for any other, or with moves queued elsewhere, which [`move_to_folder`] re-points.
fn archive_keeping_labels(
    db: &Database,
    message_id: i64,
) -> rusqlite::Result<Option<ActionOutcome>> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(None);
    };
    if is_local(&account) {
        return Ok(None);
    }
    let folders = db.list_folders(account.id)?;
    let Some(all_mail) = folders.iter().find(|folder| {
        folder.path == system::ARCHIVE
            && folder.role.as_deref() == Some("all")
            && folder.remote_name.is_some()
    }) else {
        return Ok(None);
    };
    let inbox = folders.iter().find(|folder| folder.path == system::INBOX);
    let locations = db.message_locations(message_id)?;
    // Spam and Trash are where a message is, not labels: it is archived out of them.
    let has_label = locations.iter().any(|location| {
        folders.iter().any(|folder| {
            folder.id == location.folder_id
                && !matches!(
                    folder.path.as_str(),
                    system::INBOX | system::ARCHIVE | system::JUNK | system::TRASH
                )
        })
    });
    if !has_label {
        return Ok(None);
    }
    let queued = pending_moves(db, account.id, message_id)?;
    if !queued.is_empty() {
        // Already on its way out of the Inbox: going on would take the labels too.
        let archiving = queued.iter().all(|queued| {
            matches!(&queued.action, PendingAction::Move { to, .. }
                if Some(to) == all_mail.remote_name.as_ref())
        });
        return Ok(archiving.then(ActionOutcome::default));
    }
    let Some(location) = locations
        .iter()
        .find(|location| inbox.is_some_and(|i| i.id == location.folder_id))
    else {
        // Out of the Inbox already: archived, as far as labels go.
        return Ok(Some(ActionOutcome::default()));
    };
    let Some(from) = location.remote_name.clone() else {
        return Ok(None);
    };
    let action = PendingAction::Move {
        from,
        uid: location.uid,
        to: all_mail.remote_name.clone().unwrap_or_default(),
        to_folder_id: all_mail.id,
        uid_validity: db.folder_sync_state(location.folder_id)?.uid_validity,
        message_id_hdr: message.message_id_hdr.clone(),
    };
    db.enqueue_op(
        account.id,
        action.kind(),
        Some(message_id),
        &action.encode(),
    )?;
    db.detach_location(message_id, location.folder_id)?;
    db.swap_message_folder(message_id, system::INBOX, system::ARCHIVE)?;
    Ok(Some(ActionOutcome {
        changed: true,
        needs_sync: true,
        no_destination: false,
    }))
}

/// Moves to Trash; a message already in Trash is deleted permanently.
pub fn trash(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    if db.message_is_in_trash(message_id)? {
        return delete_permanently(db, message_id);
    }
    move_to_folder(db, message_id, system::TRASH)
}

/// Moves to the Junk folder.
pub fn junk(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    move_to_folder(db, message_id, system::JUNK)
}

/// Moves a trashed message back to the Inbox (no-op outside Trash).
pub fn restore_from_trash(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    if !db.message_is_in_trash(message_id)? {
        return Ok(ActionOutcome::default());
    }
    move_to_folder(db, message_id, system::INBOX)
}

/// Deletes the message locally and queues a server delete for each of its
/// locations; queued moves of an in-flight message become deletes at their
/// source (compare-and-swap, retried like [`move_to_folder`]).
pub fn delete_permanently(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    with_cas_retry(|| delete_permanently_round(db, message_id))
}

/// One attempt of [`delete_permanently`]; `None` = lost a race, retry.
fn delete_permanently_round(
    db: &Database,
    message_id: i64,
) -> rusqlite::Result<Option<ActionOutcome>> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(Some(ActionOutcome::default()));
    };
    let mut needs_sync = false;
    if !is_local(&account) {
        let locations = db.message_locations(message_id)?;
        if locations.is_empty() {
            let queued_moves = pending_moves(db, account.id, message_id)?;
            if queued_moves.is_empty() {
                if let Some(folder) = unresolved_folder(db, account.id, &message)? {
                    let action = PendingAction::Delete {
                        folder,
                        uid: 0,
                        uid_validity: None,
                        message_id_hdr: message.message_id_hdr.clone(),
                    };
                    db.enqueue_op(
                        account.id,
                        action.kind(),
                        Some(message_id),
                        &action.encode(),
                    )?;
                    needs_sync = true;
                }
            }
            for queued in queued_moves {
                let (folder, uid) = queued.action.source();
                let delete = PendingAction::Delete {
                    folder: folder.to_string(),
                    uid,
                    uid_validity: queued.action.uid_validity(),
                    message_id_hdr: queued
                        .action
                        .message_id_hdr()
                        .map(str::to_string)
                        .or_else(|| message.message_id_hdr.clone()),
                };
                if !db.update_op_if_unchanged(
                    queued.id,
                    &queued.kind,
                    &queued.payload,
                    delete.kind(),
                    &delete.encode(),
                )? {
                    return Ok(None);
                }
                needs_sync = true;
            }
        }
        for location in &locations {
            let Some(folder) = location.remote_name.clone() else {
                continue;
            };
            let action = PendingAction::Delete {
                folder,
                uid: location.uid,
                uid_validity: db.folder_sync_state(location.folder_id)?.uid_validity,
                message_id_hdr: message.message_id_hdr.clone(),
            };
            db.enqueue_op(
                account.id,
                action.kind(),
                Some(message_id),
                &action.encode(),
            )?;
            needs_sync = true;
        }
    }
    let changed = db.delete_message(message_id)?;
    Ok(Some(ActionOutcome {
        changed,
        needs_sync,
        no_destination: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::{FolderSyncState, NewRemoteMessage, PendingOp, PROVIDER_GMAIL, PROVIDER_IMAP};
    use tempfile::NamedTempFile;

    struct Fx {
        _file: NamedTempFile,
        db: Database,
        account: i64,
        inbox: i64,
        archive: i64,
        trash: i64,
        work: i64,
    }

    /// `provider` account with INBOX/Archive/Trash/Junk/Work server folders
    /// (Gmail: Archive is `[Gmail]/All Mail` with role `all`) and a local
    /// folder without a server mailbox.
    fn fixture(provider: &str) -> Fx {
        let file = NamedTempFile::new().unwrap();
        let db = Database::open(file.path()).unwrap();
        let account = db.create_account("A", "a@x.com", provider, "{}").unwrap();
        // Every server folder has been synced once (UIDVALIDITY 7).
        let folder = |path: &str, remote: &str, role: Option<&str>| {
            let id = db
                .upsert_remote_folder(account, path, "", remote, role)
                .unwrap();
            let state = FolderSyncState {
                uid_validity: Some(7),
                ..Default::default()
            };
            db.set_folder_sync_state(id, &state).unwrap();
            id
        };
        let inbox = folder(system::INBOX, "INBOX", Some("inbox"));
        let archive = if provider == PROVIDER_GMAIL {
            folder(system::ARCHIVE, "[Gmail]/All Mail", Some("all"))
        } else {
            folder(system::ARCHIVE, "Archive", Some("archive"))
        };
        let trash = folder(system::TRASH, "Trash", Some("trash"));
        folder(system::JUNK, "Junk", Some("junk"));
        let work = folder("user:Work", "Work", None);
        db.conn()
            .execute(
                "INSERT INTO folders (account_id, path, display_name) VALUES (?1, 'user:Local', 'Local')",
                [account],
            )
            .unwrap();
        Fx {
            _file: file,
            db,
            account,
            inbox,
            archive,
            trash,
            work,
        }
    }

    fn message(fx: &Fx, folder: i64, uid: u32, subject: &str) -> i64 {
        let message = NewRemoteMessage {
            subject: subject.into(),
            message_id_hdr: Some(format!("{subject}@x")),
            gm_msgid: Some(u64::from(uid) + 1000),
            unread: true,
            ..Default::default()
        };
        fx.db
            .insert_remote_message(fx.account, folder, uid, &message)
            .unwrap()
    }

    fn ops(fx: &Fx) -> Vec<(PendingOp, PendingAction)> {
        fx.db
            .pending_ops(fx.account)
            .unwrap()
            .into_iter()
            .map(|op| {
                let action = PendingAction::decode(&op.payload).unwrap();
                assert_eq!(op.kind, action.kind());
                (op, action)
            })
            .collect()
    }

    fn csv(fx: &Fx, id: i64) -> String {
        fx.db.get_message(id).unwrap().unwrap().folders_csv
    }

    fn seen() -> MessageFlags {
        MessageFlags {
            seen: true,
            ..Default::default()
        }
    }

    const QUEUED: ActionOutcome = ActionOutcome {
        changed: true,
        needs_sync: true,
        no_destination: false,
    };

    #[test]
    fn set_read_queues_one_flags_op() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = message(&fx, fx.inbox, 5, "a");
        fx.db
            .insert_remote_message(
                fx.account,
                fx.archive,
                9,
                &NewRemoteMessage {
                    gm_msgid: Some(1005),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(set_read(&fx.db, id, true).unwrap(), QUEUED);
        assert!(!fx.db.get_message(id).unwrap().unwrap().unread);
        let queued = ops(&fx);
        assert_eq!(queued.len(), 1, "one location is enough");
        assert_eq!(queued[0].0.message_id, Some(id));
        assert_eq!(
            queued[0].1,
            PendingAction::Flags {
                folder: "INBOX".into(),
                uid: 5,
                add: seen(),
                remove: MessageFlags::default(),
                uid_validity: Some(7),
                message_id_hdr: Some("a@x".into()),
            }
        );
        assert_eq!(
            set_read(&fx.db, id, true).unwrap(),
            ActionOutcome::default()
        );
        assert_eq!(set_read(&fx.db, id, false).unwrap(), QUEUED);
        assert!(matches!(
            &ops(&fx)[1].1,
            PendingAction::Flags { add, remove, .. } if *add == MessageFlags::default() && *remove == seen()
        ));
        assert_eq!(
            set_read(&fx.db, 9999, true).unwrap(),
            ActionOutcome::default()
        );
    }

    #[test]
    fn set_starred_queues_flagged_and_updates_membership() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        assert_eq!(set_starred(&fx.db, id, true).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:flagged,sys:inbox,");
        assert!(matches!(
            &ops(&fx)[0].1,
            PendingAction::Flags { add, .. } if add.flagged
        ));
        assert_eq!(
            set_starred(&fx.db, id, true).unwrap(),
            ActionOutcome::default()
        );
        assert_eq!(set_starred(&fx.db, id, false).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:inbox,");
        assert_eq!(
            set_starred(&fx.db, 9999, true).unwrap(),
            ActionOutcome::default()
        );
    }

    #[test]
    fn local_accounts_change_locally_only() {
        let fx = fixture(PROVIDER_LOCAL);
        let id = message(&fx, fx.inbox, 1, "a");
        fx.db.detach_locations(id).unwrap();
        let local = ActionOutcome::LOCAL;
        assert_eq!(set_read(&fx.db, id, true).unwrap(), local);
        assert_eq!(set_starred(&fx.db, id, true).unwrap(), local);
        assert_eq!(archive(&fx.db, id).unwrap(), local);
        assert_eq!(csv(&fx, id), ",sys:archive,sys:flagged,");
        assert_eq!(move_to_folder(&fx.db, id, "user:Local").unwrap(), local);
        assert_eq!(trash(&fx.db, id).unwrap(), local);
        assert_eq!(restore_from_trash(&fx.db, id).unwrap(), local);
        assert_eq!(csv(&fx, id), ",sys:flagged,sys:inbox,");
        assert_eq!(delete_permanently(&fx.db, id).unwrap(), local);
        assert!(fx.db.get_message(id).unwrap().is_none());
        assert!(ops(&fx).is_empty());
    }

    #[test]
    fn move_detaches_source_and_queues_move() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        assert_eq!(move_to_folder(&fx.db, id, "user:Work").unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",user:Work,");
        assert!(fx.db.message_locations(id).unwrap().is_empty());
        assert_eq!(
            ops(&fx)[0].1,
            PendingAction::Move {
                from: "INBOX".into(),
                uid: 5,
                to: "Work".into(),
                to_folder_id: fx.work,
                uid_validity: Some(7),
                message_id_hdr: Some("a@x".into()),
            }
        );
        // Already there, forbidden, unknown or local-only targets: nothing.
        let nothing = ActionOutcome::default();
        assert_eq!(move_to_folder(&fx.db, id, "user:Work").unwrap(), nothing);
        assert_eq!(move_to_folder(&fx.db, id, system::SENT).unwrap(), nothing);
        for target in ["user:Nope", "user:Local"] {
            let outcome = move_to_folder(&fx.db, id, target).unwrap();
            assert!(outcome.no_destination && !outcome.changed && !outcome.needs_sync);
        }
        assert_eq!(
            move_to_folder(&fx.db, 9999, system::TRASH).unwrap(),
            nothing
        );
        assert_eq!(ops(&fx).len(), 1);
    }

    #[test]
    fn moving_an_in_flight_message_retargets_its_moves() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        move_to_folder(&fx.db, id, "user:Work").unwrap();
        // Flag edits while in flight target the move's source.
        set_read(&fx.db, id, true).unwrap();
        assert!(
            matches!(&ops(&fx)[1].1, PendingAction::Flags { folder, uid: 5, .. } if folder == "INBOX")
        );

        assert_eq!(archive(&fx.db, id).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:archive,");
        assert_eq!(
            ops(&fx)[0].1,
            PendingAction::Move {
                from: "INBOX".into(),
                uid: 5,
                to: "Archive".into(),
                to_folder_id: fx.archive,
                uid_validity: Some(7),
                message_id_hdr: Some("a@x".into()),
            }
        );
        // Back to where the server still has it: the move is dropped.
        assert_eq!(
            move_to_folder(&fx.db, id, system::INBOX).unwrap(),
            ActionOutcome::LOCAL
        );
        assert_eq!(ops(&fx).len(), 1, "only the flag op is left");
        assert_eq!(csv(&fx, id), ",sys:inbox,");
        assert_eq!(fx.db.folder_uids(fx.inbox).unwrap(), vec![(5, id)]);
    }

    #[test]
    fn gmail_keeps_all_mail_unless_trashing() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = message(&fx, fx.inbox, 5, "a");
        fx.db
            .insert_remote_message(
                fx.account,
                fx.archive,
                50,
                &NewRemoteMessage {
                    gm_msgid: Some(1005),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(csv(&fx, id), ",sys:archive,sys:inbox,");
        assert_eq!(move_to_folder(&fx.db, id, "user:Work").unwrap(), QUEUED);
        let queued = ops(&fx);
        assert_eq!(queued.len(), 1);
        assert!(matches!(&queued[0].1, PendingAction::Move { from, .. } if from == "INBOX"));
        let locations = fx.db.message_locations(id).unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].folder_id, fx.archive);

        assert_eq!(trash(&fx.db, id).unwrap(), QUEUED);
        let queued = ops(&fx);
        assert_eq!(
            queued.len(),
            2,
            "in-flight Work move retargeted + All Mail move"
        );
        assert!(queued
            .iter()
            .all(|(_, action)| matches!(action, PendingAction::Move { to, .. } if to == "Trash")));
        assert!(fx.db.message_locations(id).unwrap().is_empty());
        assert_eq!(csv(&fx, id), ",sys:trash,");
    }

    /// A Gmail message in the Inbox and the `work` label.
    fn labeled(fx: &Fx) -> i64 {
        let id = message(fx, fx.inbox, 5, "a");
        fx.db.attach_location(id, fx.work, 6).unwrap();
        id
    }

    #[test]
    fn archiving_only_takes_a_labeled_message_out_of_the_inbox() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = labeled(&fx);
        assert_eq!(archive(&fx.db, id).unwrap(), QUEUED);
        let queued = ops(&fx);
        assert_eq!(queued.len(), 1);
        assert!(
            matches!(&queued[0].1, PendingAction::Move { from, to, .. } if from == "INBOX" && to == "[Gmail]/All Mail")
        );
        assert_eq!(csv(&fx, id), ",sys:archive,user:Work,");
        let locations = fx.db.message_locations(id).unwrap();
        assert!(locations.iter().any(|l| l.folder_id == fx.work));
        assert!(!locations.iter().any(|l| l.folder_id == fx.inbox));

        // Already out of the Inbox: nothing left to do, and the label stays.
        assert_eq!(archive(&fx.db, id).unwrap(), ActionOutcome::default());
        assert_eq!(ops(&fx).len(), 1, "the queued move is enough");

        let queued = ops(&fx).remove(0).0;
        fx.db.complete_op(queued.id).unwrap();
        assert_eq!(archive(&fx.db, id).unwrap(), ActionOutcome::default());
        assert_eq!(csv(&fx, id), ",sys:archive,user:Work,");
    }

    #[test]
    fn spam_is_not_a_label_to_keep_when_archiving() {
        let fx = fixture(PROVIDER_GMAIL);
        let junk = fx
            .db
            .list_folders(fx.account)
            .unwrap()
            .into_iter()
            .find(|folder| folder.path == system::JUNK)
            .unwrap();
        let id = message(&fx, junk.id, 5, "a");
        assert_eq!(archive(&fx.db, id).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:archive,");
    }

    #[test]
    fn a_missing_destination_is_reported_and_nothing_changes() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        fx.db
            .conn()
            .execute("DELETE FROM folders WHERE id = ?1", [fx.trash])
            .unwrap();
        let outcome = trash(&fx.db, id).unwrap();
        assert!(outcome.no_destination);
        assert!(!outcome.changed);
        assert_eq!(csv(&fx, id), ",sys:inbox,");
    }

    #[test]
    fn moving_and_trashing_still_leave_every_label() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = labeled(&fx);
        assert_eq!(trash(&fx.db, id).unwrap(), QUEUED);
        assert_eq!(ops(&fx).len(), 2);
    }

    #[test]
    fn archive_only_message_moves_out_of_all_mail() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = message(&fx, fx.archive, 50, "a");
        assert_eq!(move_to_folder(&fx.db, id, system::INBOX).unwrap(), QUEUED);
        assert!(
            matches!(&ops(&fx)[0].1, PendingAction::Move { from, to, .. } if from == "[Gmail]/All Mail" && to == "INBOX")
        );
    }

    #[test]
    fn trash_junk_restore_and_delete() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        assert_eq!(
            restore_from_trash(&fx.db, id).unwrap(),
            ActionOutcome::default()
        );
        assert_eq!(junk(&fx.db, id).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:junk,");
        assert_eq!(trash(&fx.db, id).unwrap(), QUEUED);
        assert_eq!(csv(&fx, id), ",sys:trash,");
        // Restoring cancels the queued move and re-attaches the location.
        assert_eq!(
            restore_from_trash(&fx.db, id).unwrap(),
            ActionOutcome::LOCAL
        );
        assert_eq!(csv(&fx, id), ",sys:inbox,");
        assert!(ops(&fx).is_empty());
        assert_eq!(fx.db.folder_uids(fx.inbox).unwrap(), vec![(5, id)]);
        assert_eq!(trash(&fx.db, id).unwrap(), QUEUED);
        // Trash again = delete permanently: the queued move becomes a delete
        // at its source.
        assert_eq!(trash(&fx.db, id).unwrap(), QUEUED);
        assert!(fx.db.get_message(id).unwrap().is_none());
        let queued = ops(&fx);
        assert_eq!(queued.len(), 1);
        assert_eq!(
            queued[0].1,
            PendingAction::Delete {
                folder: "INBOX".into(),
                uid: 5,
                uid_validity: Some(7),
                message_id_hdr: Some("a@x".into()),
            }
        );
    }

    #[test]
    fn delete_permanently_queues_a_delete_per_location() {
        let fx = fixture(PROVIDER_GMAIL);
        let id = message(&fx, fx.trash, 3, "a");
        fx.db
            .insert_remote_message(
                fx.account,
                fx.archive,
                4,
                &NewRemoteMessage {
                    gm_msgid: Some(1003),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(delete_permanently(&fx.db, id).unwrap(), QUEUED);
        let actions: Vec<_> = ops(&fx).into_iter().map(|(_, a)| a).collect();
        assert_eq!(
            actions,
            vec![
                PendingAction::Delete {
                    folder: "Trash".into(),
                    uid: 3,
                    uid_validity: Some(7),
                    message_id_hdr: Some("a@x".into()),
                },
                PendingAction::Delete {
                    folder: "[Gmail]/All Mail".into(),
                    uid: 4,
                    uid_validity: Some(7),
                    message_id_hdr: Some("a@x".into()),
                },
            ]
        );
        assert_eq!(
            delete_permanently(&fx.db, id).unwrap(),
            ActionOutcome::default()
        );
        let _ = fx.trash;
    }

    #[test]
    fn move_retries_when_the_worker_changed_the_queued_op() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        move_to_folder(&fx.db, id, "user:Work").unwrap();
        let (op, _) = ops(&fx).remove(0);
        // The worker ran the move and completed the op, attaching the new
        // location, between our read and our rewrite.
        let mut calls = 0;
        let outcome = with_cas_retry(|| {
            calls += 1;
            if calls == 1 {
                fx.db.complete_op(op.id).unwrap();
                fx.db.attach_location(id, fx.work, 40).unwrap();
                return Ok(None);
            }
            move_to_folder_round(&fx.db, id, system::ARCHIVE)
        })
        .unwrap();
        assert_eq!(outcome, QUEUED);
        let queued = ops(&fx);
        assert_eq!(queued.len(), 1);
        assert!(matches!(
            &queued[0].1,
            PendingAction::Move { from, uid: 40, to, .. } if from == "Work" && to == "Archive"
        ));
        assert_eq!(csv(&fx, id), ",sys:archive,");
    }

    #[test]
    fn stale_op_rewrite_restarts_the_round() {
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        move_to_folder(&fx.db, id, "user:Work").unwrap();
        let (op, mut action) = ops(&fx).remove(0);
        // Rewritten behind our back: the round reports a lost race.
        action.retarget("Work", 41, Some(7));
        fx.db
            .update_op_if_unchanged(
                op.id,
                &op.kind,
                &op.payload,
                action.kind(),
                &action.encode(),
            )
            .unwrap();
        let stale = QueuedMove {
            id: op.id,
            kind: op.kind.clone(),
            payload: op.payload.clone(),
            action: PendingAction::decode(&op.payload).unwrap(),
        };
        assert!(!fx
            .db
            .update_op_if_unchanged(stale.id, &stale.kind, &stale.payload, "move", "{}")
            .unwrap());
        // A full call converges on the fresh state.
        assert_eq!(move_to_folder(&fx.db, id, system::ARCHIVE).unwrap(), QUEUED);
        assert!(matches!(
            &ops(&fx)[0].1,
            PendingAction::Move { uid: 41, to, .. } if to == "Archive"
        ));
        // Every round losing still terminates.
        let mut rounds = 0;
        let outcome = with_cas_retry(|| {
            rounds += 1;
            Ok(None)
        })
        .unwrap();
        assert_eq!(rounds, CAS_ROUNDS);
        assert_eq!(outcome, QUEUED);
    }

    #[test]
    fn helpers() {
        assert_eq!(flag_change(seen(), true), (seen(), MessageFlags::default()));
        assert_eq!(
            flag_change(seen(), false),
            (MessageFlags::default(), seen())
        );
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        assert_eq!(
            flags_target(&fx.db, fx.account, &fx.db.get_message(id).unwrap().unwrap()).unwrap(),
            Some(("INBOX".into(), 5, Some(7)))
        );
        assert!(pending_moves(&fx.db, fx.account, id).unwrap().is_empty());
        fx.db.detach_locations(id).unwrap();
        let detail = fx.db.get_message(id).unwrap().unwrap();
        assert_eq!(
            flags_target(&fx.db, fx.account, &detail).unwrap(),
            Some(("INBOX".into(), 0, None)),
            "no location, no queued move: found by Message-ID when it runs"
        );
        assert_eq!(
            unresolved_folder(&fx.db, fx.account, &detail)
                .unwrap()
                .as_deref(),
            Some("INBOX")
        );
        let no_header = MessageDetail {
            message_id_hdr: None,
            ..detail.clone()
        };
        assert_eq!(flags_target(&fx.db, fx.account, &no_header).unwrap(), None);
        assert!(!enqueue_flags(&fx.db, fx.account, &no_header, seen(), seen()).unwrap());
        assert!(enqueue_flags(&fx.db, fx.account, &detail, seen(), seen()).unwrap());
        assert_eq!(primary_folders(&detail), vec![system::INBOX]);
        assert!(context(&fx.db, 9999).unwrap().is_none());
        assert!(is_local(&Account {
            id: 1,
            name: String::new(),
            email: String::new(),
            provider: PROVIDER_LOCAL.into(),
            config_json: String::new(),
            sender_name: String::new(),
        }));
    }
}
