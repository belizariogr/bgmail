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
    is_manual_move_destination_forbidden, system, Account, Database, MessageDetail, PROVIDER_LOCAL,
};

use crate::plan::PendingAction;

/// What an action did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionOutcome {
    /// The local database changed (reload the list).
    pub changed: bool,
    /// A server op was queued (send `Command::ProcessQueue`).
    pub needs_sync: bool,
}

impl ActionOutcome {
    const LOCAL: Self = Self {
        changed: true,
        needs_sync: false,
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

/// Queued Move ops of the message (it is in flight), oldest first.
fn pending_moves(
    db: &Database,
    account_id: i64,
    message_id: i64,
) -> rusqlite::Result<Vec<(i64, PendingAction)>> {
    Ok(db
        .pending_ops(account_id)?
        .into_iter()
        .filter(|op| op.message_id == Some(message_id))
        .filter_map(|op| {
            PendingAction::decode(&op.payload)
                .filter(|action| matches!(action, PendingAction::Move { .. }))
                .map(|action| (op.id, action))
        })
        .collect())
}

/// Where a flag change must be stored: the first server location, or — for
/// a message in flight — the source of its last queued move (the worker
/// re-points the flag op once the move reports the new UID).
fn flags_target(
    db: &Database,
    account_id: i64,
    message_id: i64,
) -> rusqlite::Result<Option<(String, u32)>> {
    if let Some(location) = db
        .message_locations(message_id)?
        .into_iter()
        .find(|l| l.remote_name.is_some())
    {
        return Ok(location.remote_name.map(|name| (name, location.uid)));
    }
    Ok(pending_moves(db, account_id, message_id)?
        .pop()
        .map(|(_, action)| {
            let (folder, uid) = action.source();
            (folder.to_string(), uid)
        }))
}

/// Queues a flag change for one location of the message; whether one was
/// queued.
fn enqueue_flags(
    db: &Database,
    account_id: i64,
    message_id: i64,
    add: MessageFlags,
    remove: MessageFlags,
) -> rusqlite::Result<bool> {
    let Some((folder, uid)) = flags_target(db, account_id, message_id)? else {
        return Ok(false);
    };
    let action = PendingAction::Flags {
        folder,
        uid,
        add,
        remove,
    };
    db.enqueue_op(
        account_id,
        action.kind(),
        Some(message_id),
        &action.encode(),
    )?;
    Ok(true)
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
        needs_sync = enqueue_flags(db, account.id, message_id, add, remove)?;
    }
    db.set_message_unread(message_id, !read)?;
    Ok(ActionOutcome {
        changed: true,
        needs_sync,
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
        needs_sync = enqueue_flags(db, account.id, message_id, add, remove)?;
    }
    db.toggle_message_starred(message_id)?;
    Ok(ActionOutcome {
        changed: true,
        needs_sync,
    })
}

/// Non-flagged folders the message is shown in.
fn primary_folders(message: &MessageDetail) -> Vec<&str> {
    message
        .folders_csv
        .split(',')
        .filter(|path| !path.is_empty() && *path != system::FLAGGED)
        .collect()
}

/// Moves the message to `target_path` (a folder of its account). Server
/// accounts: the target needs a server mailbox; every location outside the
/// target is detached and gets a queued Move. Gmail-style "All Mail"
/// locations stay attached unless the target is Trash/Junk (All Mail keeps
/// every non-trash message). Moves still queued for the message are
/// re-pointed at the new target (or cancelled when it is their source).
pub fn move_to_folder(
    db: &Database,
    message_id: i64,
    target_path: &str,
) -> rusqlite::Result<ActionOutcome> {
    let Some((message, account)) = context(db, message_id)? else {
        return Ok(ActionOutcome::default());
    };
    if is_manual_move_destination_forbidden(target_path)
        || primary_folders(&message) == [target_path]
    {
        return Ok(ActionOutcome::default());
    }
    if is_local(&account) {
        db.move_message_to_folder(message_id, target_path)?;
        return Ok(ActionOutcome::LOCAL);
    }
    let folders = db.list_folders(account.id)?;
    let Some(target) = folders
        .iter()
        .find(|f| f.path == target_path && f.remote_name.is_some())
    else {
        return Ok(ActionOutcome::default());
    };
    let to = target.remote_name.clone().unwrap_or_default();
    let removes_everywhere = matches!(target_path, system::TRASH | system::JUNK);
    let all_mail_ids: Vec<i64> = folders
        .iter()
        .filter(|f| f.role.as_deref() == Some("all"))
        .map(|f| f.id)
        .collect();

    let mut needs_sync = false;
    // Moves still queued for this message now go to the new target.
    for (op_id, action) in pending_moves(db, account.id, message_id)? {
        let PendingAction::Move { from, uid, .. } = action else {
            continue;
        };
        if from == to {
            // Back where the server still has it: cancel the move and
            // re-attach the location so later actions see it again.
            db.complete_op(op_id)?;
            db.attach_location(message_id, target.id, uid)?;
            continue;
        }
        let retargeted = PendingAction::Move {
            from,
            uid,
            to: to.clone(),
            to_folder_id: target.id,
        };
        db.update_op(op_id, retargeted.kind(), &retargeted.encode())?;
        needs_sync = true;
    }
    let locations = db.message_locations(message_id)?;
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
    Ok(ActionOutcome {
        changed: true,
        needs_sync,
    })
}

/// Moves to the Archive folder (Gmail: All Mail, which sync maps there).
pub fn archive(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    move_to_folder(db, message_id, system::ARCHIVE)
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
/// source.
pub fn delete_permanently(db: &Database, message_id: i64) -> rusqlite::Result<ActionOutcome> {
    let Some((_, account)) = context(db, message_id)? else {
        return Ok(ActionOutcome::default());
    };
    let mut needs_sync = false;
    if !is_local(&account) {
        let locations = db.message_locations(message_id)?;
        for location in &locations {
            let Some(folder) = location.remote_name.clone() else {
                continue;
            };
            let action = PendingAction::Delete {
                folder,
                uid: location.uid,
            };
            db.enqueue_op(
                account.id,
                action.kind(),
                Some(message_id),
                &action.encode(),
            )?;
            needs_sync = true;
        }
        if locations.is_empty() {
            for (op_id, action) in pending_moves(db, account.id, message_id)? {
                let (folder, uid) = action.source();
                let delete = PendingAction::Delete {
                    folder: folder.to_string(),
                    uid,
                };
                db.update_op(op_id, delete.kind(), &delete.encode())?;
                needs_sync = true;
            }
        }
    }
    let changed = db.delete_message(message_id)?;
    Ok(ActionOutcome {
        changed,
        needs_sync,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::{NewRemoteMessage, PendingOp, PROVIDER_GMAIL, PROVIDER_IMAP};
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
        let folder = |path: &str, remote: &str, role: Option<&str>| {
            db.upsert_remote_folder(account, path, "", remote, role)
                .unwrap()
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
            }
        );
        // Already there, forbidden, unknown or local-only targets: nothing.
        let nothing = ActionOutcome::default();
        assert_eq!(move_to_folder(&fx.db, id, "user:Work").unwrap(), nothing);
        assert_eq!(move_to_folder(&fx.db, id, system::SENT).unwrap(), nothing);
        assert_eq!(move_to_folder(&fx.db, id, "user:Nope").unwrap(), nothing);
        assert_eq!(move_to_folder(&fx.db, id, "user:Local").unwrap(), nothing);
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
                uid: 5
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
                    uid: 3
                },
                PendingAction::Delete {
                    folder: "[Gmail]/All Mail".into(),
                    uid: 4
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
    fn helpers() {
        assert_eq!(flag_change(seen(), true), (seen(), MessageFlags::default()));
        assert_eq!(
            flag_change(seen(), false),
            (MessageFlags::default(), seen())
        );
        let fx = fixture(PROVIDER_IMAP);
        let id = message(&fx, fx.inbox, 5, "a");
        assert_eq!(
            flags_target(&fx.db, fx.account, id).unwrap(),
            Some(("INBOX".into(), 5))
        );
        assert!(pending_moves(&fx.db, fx.account, id).unwrap().is_empty());
        fx.db.detach_locations(id).unwrap();
        assert_eq!(flags_target(&fx.db, fx.account, id).unwrap(), None);
        assert!(!enqueue_flags(&fx.db, fx.account, id, seen(), seen()).unwrap());
        let detail = fx.db.get_message(id).unwrap().unwrap();
        assert_eq!(primary_folders(&detail), vec![system::INBOX]);
        assert!(context(&fx.db, 9999).unwrap().is_none());
        assert!(is_local(&Account {
            id: 1,
            name: String::new(),
            email: String::new(),
            provider: PROVIDER_LOCAL.into(),
            config_json: String::new(),
        }));
    }
}
