//! Catalog of application commands and their availability rules.

use std::collections::HashMap;

use gpui::SharedString;
use storage::{is_manual_move_destination_forbidden, system, Folder, MessageDetail};

use crate::data::MailboxKind;
use crate::locale::{Key, Language};

/// Identifies a command shown in the palette or menus.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommandId {
    ComposeNew,
    OpenSettings,
    ToggleSidebar,
    MessageDelete,
    MessageDeletePermanent,
    MessageRestore,
    MessageArchive,
    MessageMarkJunk,
    MessageToggleFlag,
    MessagePrint,
    MessageToggleRead,
    MessageReply,
    MessageReplyAll,
    MessageForward,
    SyncNow,
    MoveToFolder(SharedString),
}

/// What the commands need to know about a multiple selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkSelection {
    pub account_ids: Vec<i64>,
    pub has_unread: bool,
    pub all_starred: bool,
    /// Some row is only in Sent: the selection can't be moved.
    pub any_sent_only: bool,
}

/// Snapshot of UI state used to decide which commands are available.
#[derive(Debug, Clone, Default)]
pub struct CommandContext {
    pub selected_message_id: Option<i64>,
    pub message_detail: Option<MessageDetail>,
    pub folders_by_account: HashMap<i64, Vec<Folder>>,
    /// Several list rows are selected: only actions that work on all of them
    /// are available.
    pub bulk: Option<BulkSelection>,
    /// The list shows the Trash, where trashing means deleting for good.
    pub list_in_trash: bool,
    /// The list shows Sent, whose items can't leave it.
    pub list_in_sent: bool,
}

impl CommandContext {
    /// Folders every account of a multiple selection has.
    fn bulk_move_targets(
        &self,
        bulk: &BulkSelection,
        language: Language,
    ) -> Vec<(SharedString, SharedString)> {
        let folders_of = |id: &i64| self.folders_by_account.get(id).into_iter().flatten();
        let Some((first, others)) = bulk.account_ids.split_first() else {
            return Vec::new();
        };
        folders_of(first)
            .filter(|folder| !is_manual_move_destination_forbidden(&folder.path))
            .filter(|folder| {
                others
                    .iter()
                    .all(|id| folders_of(id).any(|other| other.path == folder.path))
            })
            .map(|folder| {
                (
                    folder_display_name(folder, language).into(),
                    folder.path.clone().into(),
                )
            })
            .collect()
    }

    /// Whether the action can run on the rows of a bulk selection.
    fn bulk_action_available(&self) -> bool {
        self.bulk.is_some() && !self.list_in_trash
    }

    /// Whether the selection may go to another folder: sent mail is only deleted.
    pub fn selection_movable(&self) -> bool {
        if self.list_in_sent {
            return false;
        }
        match &self.bulk {
            Some(bulk) => !bulk.any_sent_only,
            None => !self
                .message_detail
                .as_ref()
                .is_some_and(|detail| message_only_in_sent(&detail.folders_csv)),
        }
    }

    pub fn message_in_trash(&self) -> bool {
        self.message_detail
            .as_ref()
            .is_some_and(|detail| message_has_folder(&detail.folders_csv, system::TRASH))
    }

    pub fn message_in_junk(&self) -> bool {
        self.message_detail
            .as_ref()
            .is_some_and(|detail| message_has_folder(&detail.folders_csv, system::JUNK))
    }

    /// Whether archiving would change anything. Gmail keeps every message in
    /// All Mail (which maps to Archive), Inbox included, so being there alone
    /// doesn't mean it was archived.
    pub fn message_archivable(&self) -> bool {
        self.message_detail.as_ref().is_some_and(|detail| {
            !message_has_folder(&detail.folders_csv, system::ARCHIVE)
                || message_has_folder(&detail.folders_csv, system::INBOX)
        })
    }

    /// A multiple selection counts as unread while any of its rows is: the
    /// toggle then marks them all read.
    pub fn message_unread(&self) -> bool {
        match &self.bulk {
            Some(bulk) => bulk.has_unread,
            None => self
                .message_detail
                .as_ref()
                .is_some_and(|detail| detail.unread),
        }
    }

    /// A multiple selection counts as starred once all its rows are: until
    /// then the toggle stars them all.
    pub fn message_starred(&self) -> bool {
        match &self.bulk {
            Some(bulk) => bulk.all_starred,
            None => self
                .message_detail
                .as_ref()
                .is_some_and(|detail| detail.starred),
        }
    }

    /// Folders the selected message can move to, as (label, path): not the
    /// ones it is already in, except Archive, where Gmail keeps all its mail.
    pub fn move_targets(&self, language: Language) -> Vec<(SharedString, SharedString)> {
        if !self.selection_movable() {
            return Vec::new();
        }
        if let Some(bulk) = &self.bulk {
            return self.bulk_move_targets(bulk, language);
        }
        let Some(detail) = &self.message_detail else {
            return Vec::new();
        };
        self.folders_by_account
            .get(&detail.account_id)
            .into_iter()
            .flatten()
            .filter(|folder| !is_manual_move_destination_forbidden(&folder.path))
            .filter(|folder| {
                folder.path == system::ARCHIVE
                    || !message_has_folder(&detail.folders_csv, &folder.path)
            })
            .map(|folder| {
                (
                    folder_display_name(folder, language).into(),
                    folder.path.clone().into(),
                )
            })
            .collect()
    }
}

/// One row in the command palette.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub id: CommandId,
    pub label: SharedString,
}

fn message_has_folder(csv: &str, folder_path: &str) -> bool {
    csv.split(',')
        .filter(|segment| !segment.is_empty())
        .any(|path| path == folder_path)
}

/// Mail sent to oneself is also in the Inbox: from there it is received mail, which
/// has to remain archivable like any other.
pub fn message_only_in_sent(folders_csv: &str) -> bool {
    message_has_folder(folders_csv, system::SENT) && !message_has_folder(folders_csv, system::INBOX)
}

/// Localized display name for a storage folder path.
pub fn folder_display_name(folder: &Folder, language: Language) -> String {
    if !folder.display_name.is_empty() {
        return folder.display_name.clone();
    }
    system_folder_label(&folder.path, language)
}

fn system_folder_label(path: &str, language: Language) -> String {
    let kind = match path {
        system::INBOX => Some(MailboxKind::Inbox),
        system::DRAFTS => Some(MailboxKind::Drafts),
        system::SENT => Some(MailboxKind::Sent),
        system::JUNK => Some(MailboxKind::Junk),
        system::TRASH => Some(MailboxKind::Trash),
        system::ARCHIVE => Some(MailboxKind::Archive),
        _ => None,
    };
    kind.map(|kind| kind.display_name(language).to_string())
        .unwrap_or_else(|| path.to_string())
}

fn label_for(id: &CommandId, language: Language, ctx: &CommandContext) -> SharedString {
    let text = match id {
        CommandId::ComposeNew => Key::ComposeWindowTitle.tr(language),
        CommandId::OpenSettings => Key::SettingsTitle.tr(language),
        CommandId::ToggleSidebar => Key::ToolbarToggleSidebar.tr(language),
        CommandId::MessageDelete => Key::CommandDelete.tr(language),
        CommandId::MessageDeletePermanent => Key::CommandDeletePermanent.tr(language),
        CommandId::MessageRestore => Key::CommandRestore.tr(language),
        CommandId::MessageArchive => Key::CommandArchive.tr(language),
        CommandId::MessageMarkJunk => Key::CommandMarkJunk.tr(language),
        CommandId::MessageToggleFlag if ctx.message_starred() => Key::CommandUnflag.tr(language),
        CommandId::MessageToggleFlag => Key::CommandFlag.tr(language),
        CommandId::MessagePrint => Key::CommandPrint.tr(language),
        CommandId::MessageToggleRead if ctx.message_unread() => Key::CommandMarkRead.tr(language),
        CommandId::MessageToggleRead => Key::CommandMarkUnread.tr(language),
        CommandId::MessageReply => Key::ToolbarReply.tr(language),
        CommandId::MessageReplyAll => Key::ToolbarReplyAll.tr(language),
        CommandId::MessageForward => Key::ToolbarForward.tr(language),
        CommandId::SyncNow => Key::CommandSyncNow.tr(language),
        CommandId::MoveToFolder(path) => {
            let folder = ctx
                .folders_by_account
                .values()
                .flatten()
                .find(|folder| folder.path == path.as_ref());
            return folder
                .map(|folder| folder_display_name(folder, language))
                .unwrap_or_else(|| path.to_string())
                .into();
        }
    };
    text.into()
}

/// Whether `id` can run in the current context.
pub fn command_enabled(id: &CommandId, ctx: &CommandContext) -> bool {
    match id {
        CommandId::ComposeNew
        | CommandId::OpenSettings
        | CommandId::ToggleSidebar
        | CommandId::SyncNow => true,
        CommandId::MessageDelete => {
            ctx.bulk_action_available()
                || (ctx.selected_message_id.is_some() && !ctx.message_in_trash())
        }
        CommandId::MessageDeletePermanent | CommandId::MessageRestore => {
            (ctx.bulk.is_some() && ctx.list_in_trash)
                || (ctx.selected_message_id.is_some() && ctx.message_in_trash())
        }
        CommandId::MessageArchive => {
            ctx.selection_movable()
                && (ctx.bulk_action_available()
                    || (ctx.selected_message_id.is_some()
                        && !ctx.message_in_trash()
                        && ctx.message_archivable()))
        }
        CommandId::MessageMarkJunk => {
            ctx.selection_movable()
                && (ctx.bulk_action_available()
                    || (ctx.selected_message_id.is_some()
                        && !ctx.message_in_trash()
                        && !ctx.message_in_junk()))
        }
        CommandId::MessageToggleRead => ctx.selected_message_id.is_some() || ctx.bulk.is_some(),
        CommandId::MessagePrint
        | CommandId::MessageReply
        | CommandId::MessageReplyAll
        | CommandId::MessageForward => ctx.selected_message_id.is_some(),
        CommandId::MessageToggleFlag => {
            ctx.bulk_action_available()
                || (ctx.selected_message_id.is_some() && !ctx.message_in_trash())
        }
        CommandId::MoveToFolder(_) => {
            ctx.selection_movable()
                && (ctx.bulk_action_available()
                    || (ctx.selected_message_id.is_some() && !ctx.message_in_trash()))
        }
    }
}

/// All commands for the palette, filtered by selection and availability.
pub fn palette_commands(language: Language, ctx: &CommandContext) -> Vec<CommandEntry> {
    let has_message = ctx.selected_message_id.is_some() || ctx.bulk.is_some();
    let mut ids = vec![
        CommandId::ComposeNew,
        CommandId::OpenSettings,
        CommandId::ToggleSidebar,
        CommandId::SyncNow,
    ];
    if has_message {
        ids.extend([
            CommandId::MessageReply,
            CommandId::MessageReplyAll,
            CommandId::MessageForward,
            CommandId::MessageToggleRead,
            CommandId::MessageDelete,
            CommandId::MessageDeletePermanent,
            CommandId::MessageRestore,
            CommandId::MessageArchive,
            CommandId::MessageMarkJunk,
            CommandId::MessageToggleFlag,
            CommandId::MessagePrint,
        ]);
        for (_label, path) in ctx.move_targets(language) {
            ids.push(CommandId::MoveToFolder(path));
        }
    }

    ids.into_iter()
        .filter(|id| command_enabled(id, ctx))
        .map(|id| CommandEntry {
            label: label_for(&id, language, ctx),
            id,
        })
        .collect()
}

/// Case-insensitive substring match used by the command palette filter.
pub fn command_matches_query(label: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query = query.to_ascii_lowercase();
    label.to_ascii_lowercase().contains(&query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::user_folder_path;

    fn only_moves_are_blocked(ctx: &CommandContext) {
        for id in [
            CommandId::MessageArchive,
            CommandId::MessageMarkJunk,
            CommandId::MoveToFolder(user_folder_path("Clients").into()),
        ] {
            assert!(!command_enabled(&id, ctx), "{id:?} must be blocked");
        }
        assert!(ctx.move_targets(Language::default()).is_empty());
        for id in [
            CommandId::MessageDelete,
            CommandId::MessageToggleFlag,
            CommandId::MessageToggleRead,
        ] {
            assert!(command_enabled(&id, ctx), "{id:?} must stay available");
        }
    }

    #[test]
    fn a_sent_message_can_only_be_deleted() {
        let mut ctx = ctx_with_message(false, false);
        ctx.message_detail.as_mut().unwrap().folders_csv = ",sys:sent,".into();
        only_moves_are_blocked(&ctx);
    }

    #[test]
    fn gmails_all_mail_copy_does_not_free_a_sent_message() {
        let mut ctx = ctx_with_message(false, false);
        ctx.message_detail.as_mut().unwrap().folders_csv = ",sys:sent,sys:archive,".into();
        only_moves_are_blocked(&ctx);
    }

    #[test]
    fn mail_sent_to_oneself_moves_from_the_inbox_but_not_from_sent() {
        let mut ctx = ctx_with_message(false, false);
        ctx.message_detail.as_mut().unwrap().folders_csv = ",sys:inbox,sys:sent,".into();
        assert!(command_enabled(&CommandId::MessageArchive, &ctx));
        ctx.list_in_sent = true;
        only_moves_are_blocked(&ctx);
    }

    #[test]
    fn one_sent_row_blocks_moving_a_multiple_selection() {
        let mut ctx = ctx_with_message(false, false);
        ctx.bulk = Some(BulkSelection {
            account_ids: vec![1],
            any_sent_only: true,
            ..Default::default()
        });
        only_moves_are_blocked(&ctx);
        ctx.bulk.as_mut().unwrap().any_sent_only = false;
        assert!(command_enabled(&CommandId::MessageArchive, &ctx));
    }

    fn ctx_with_message(in_trash: bool, starred: bool) -> CommandContext {
        let folders_csv = if in_trash {
            ",sys:trash,"
        } else if starred {
            ",sys:inbox,sys:flagged,"
        } else {
            ",sys:inbox,"
        };
        let account_id = 1;
        let mut folders_by_account = HashMap::new();
        folders_by_account.insert(
            account_id,
            vec![
                Folder {
                    id: 1,
                    account_id,
                    path: system::INBOX.into(),
                    display_name: String::new(),
                    remote_name: None,
                    role: None,
                },
                Folder {
                    id: 2,
                    account_id,
                    path: user_folder_path("Clients"),
                    display_name: "Clients".into(),
                    remote_name: None,
                    role: None,
                },
            ],
        );
        CommandContext {
            selected_message_id: Some(7),
            message_detail: Some(MessageDetail {
                id: 7,
                account_id,
                sender: "A".into(),
                sender_email: "a@test.com".into(),
                subject: "Hi".into(),
                plain_text: "p".into(),
                raw_content: "p".into(),
                raw_format: "text".into(),
                received_at: "1".into(),
                date_ts: 0,
                to_addrs: String::new(),
                cc_addrs: String::new(),
                reply_to: String::new(),
                message_id_hdr: None,
                in_reply_to: None,
                references_hdr: String::new(),
                thread_id: None,
                body_state: 1,
                answered: false,
                unread: false,
                starred,
                has_attachment: false,
                folders_csv: folders_csv.into(),
            }),
            folders_by_account,
            ..Default::default()
        }
    }

    #[test]
    fn read_toggle_label_follows_unread_state() {
        let mut ctx = ctx_with_message(false, false);
        ctx.message_detail.as_mut().unwrap().unread = true;
        assert_eq!(
            label_for(&CommandId::MessageToggleRead, Language::English, &ctx).as_ref(),
            "Mark as Read"
        );
        ctx.message_detail.as_mut().unwrap().unread = false;
        assert_eq!(
            label_for(&CommandId::MessageToggleRead, Language::English, &ctx).as_ref(),
            "Mark as Unread"
        );
    }

    #[test]
    fn reply_commands_need_a_message_and_sync_never_does() {
        let empty = CommandContext::default();
        assert!(!command_enabled(&CommandId::MessageReply, &empty));
        assert!(!command_enabled(&CommandId::MessageForward, &empty));
        assert!(command_enabled(&CommandId::SyncNow, &empty));
        let ctx = ctx_with_message(true, false);
        assert!(command_enabled(&CommandId::MessageReplyAll, &ctx));
        let ids: Vec<_> = palette_commands(Language::English, &ctx)
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert!(ids.contains(&CommandId::MessageReply));
        assert!(ids.contains(&CommandId::MessageToggleRead));
    }

    #[test]
    fn print_needs_a_selected_message_even_in_trash() {
        assert!(command_enabled(
            &CommandId::MessagePrint,
            &ctx_with_message(true, false)
        ));
        assert!(!command_enabled(
            &CommandId::MessagePrint,
            &CommandContext::default()
        ));
        let labels: Vec<_> = palette_commands(Language::English, &ctx_with_message(false, false))
            .into_iter()
            .map(|entry| entry.label.to_string())
            .collect();
        assert!(labels.iter().any(|label| label == "Print…"));
    }

    #[test]
    fn delete_disabled_in_trash_restore_enabled() {
        let ctx = ctx_with_message(true, false);
        assert!(!command_enabled(&CommandId::MessageDelete, &ctx));
        assert!(command_enabled(&CommandId::MessageRestore, &ctx));
        assert!(command_enabled(&CommandId::MessageDeletePermanent, &ctx));
    }

    #[test]
    fn inbox_message_can_delete_not_restore() {
        let ctx = ctx_with_message(false, false);
        assert!(command_enabled(&CommandId::MessageDelete, &ctx));
        assert!(!command_enabled(&CommandId::MessageRestore, &ctx));
    }

    #[test]
    fn palette_never_lists_command_palette_action() {
        let ctx = CommandContext::default();
        let entries = palette_commands(Language::English, &ctx);
        let labels: Vec<_> = entries.iter().map(|e| e.label.as_ref()).collect();
        assert!(!labels.iter().any(|label| label.contains("Command Palette")));
        assert!(!labels
            .iter()
            .any(|label| label.contains("Paleta de comandos")));
    }

    #[test]
    fn palette_without_message_omits_message_actions() {
        let ctx = CommandContext::default();
        let entries = palette_commands(Language::English, &ctx);
        assert!(!entries.iter().any(|entry| matches!(
            entry.id,
            CommandId::MessageDelete | CommandId::MessageArchive | CommandId::MoveToFolder(_)
        )));
        assert!(entries
            .iter()
            .any(|entry| entry.id == CommandId::ComposeNew));
    }

    #[test]
    fn palette_with_message_includes_move_targets() {
        let ctx = ctx_with_message(false, false);
        let entries = palette_commands(Language::English, &ctx);
        assert!(entries
            .iter()
            .any(|entry| matches!(entry.id, CommandId::MoveToFolder(_))));
    }

    fn ctx_in_folders(folders_csv: &str) -> CommandContext {
        let mut ctx = ctx_with_message(false, false);
        ctx.message_detail.as_mut().unwrap().folders_csv = folders_csv.into();
        ctx
    }

    #[test]
    fn gmail_inbox_mail_can_be_archived_despite_all_mail() {
        let enabled = |csv| command_enabled(&CommandId::MessageArchive, &ctx_in_folders(csv));
        assert!(enabled(",sys:archive,sys:inbox,"));
        assert!(enabled(",sys:inbox,"));
        assert!(enabled(",user:Work,"));
        assert!(!enabled(",sys:archive,"));
        assert!(!enabled(",sys:archive,user:Work,"));
    }

    #[test]
    fn bulk_selection_offers_only_actions_on_every_row() {
        let mut ctx = CommandContext {
            bulk: Some(BulkSelection::default()),
            ..Default::default()
        };
        let enabled = |ctx: &CommandContext, id: CommandId| command_enabled(&id, ctx);
        for id in [
            CommandId::MessageDelete,
            CommandId::MessageArchive,
            CommandId::MessageMarkJunk,
            CommandId::MessageToggleFlag,
            CommandId::MessageToggleRead,
            CommandId::MoveToFolder("x".into()),
        ] {
            assert!(enabled(&ctx, id.clone()), "{id:?}");
        }
        for id in [
            CommandId::MessageReply,
            CommandId::MessagePrint,
            CommandId::MessageRestore,
            CommandId::MessageDeletePermanent,
        ] {
            assert!(!enabled(&ctx, id.clone()), "{id:?}");
        }

        ctx.list_in_trash = true;
        for id in [
            CommandId::MessageDelete,
            CommandId::MessageArchive,
            CommandId::MessageToggleFlag,
            CommandId::MoveToFolder("x".into()),
        ] {
            assert!(!enabled(&ctx, id.clone()), "{id:?}");
        }
        assert!(enabled(&ctx, CommandId::MessageRestore));
        assert!(enabled(&ctx, CommandId::MessageDeletePermanent));
    }

    #[test]
    fn bulk_toggles_follow_the_whole_selection() {
        let ctx = CommandContext {
            bulk: Some(BulkSelection {
                has_unread: true,
                all_starred: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(ctx.message_unread() && !ctx.message_starred());
    }

    #[test]
    fn bulk_move_targets_are_the_folders_all_accounts_share() {
        let folder = |account_id, path: String| Folder {
            id: 0,
            account_id,
            path,
            display_name: String::new(),
            remote_name: None,
            role: None,
        };
        let mut ctx = CommandContext {
            bulk: Some(BulkSelection {
                account_ids: vec![1, 2],
                ..Default::default()
            }),
            ..Default::default()
        };
        ctx.folders_by_account.insert(
            1,
            vec![
                folder(1, system::TRASH.into()),
                folder(1, system::SENT.into()),
                folder(1, user_folder_path("Only one")),
            ],
        );
        ctx.folders_by_account
            .insert(2, vec![folder(2, system::TRASH.into())]);
        let targets = ctx.move_targets(Language::English);
        let paths: Vec<&str> = targets.iter().map(|(_, path)| path.as_ref()).collect();
        assert_eq!(paths, vec![system::TRASH]);
    }

    #[test]
    fn move_targets_are_localized_and_skip_the_current_folder() {
        let mut ctx = ctx_in_folders(",sys:inbox,sys:archive,");
        ctx.folders_by_account.insert(
            1,
            [system::INBOX, system::ARCHIVE, system::TRASH]
                .iter()
                .enumerate()
                .map(|(ix, path)| Folder {
                    id: ix as i64,
                    account_id: 1,
                    path: path.to_string(),
                    display_name: String::new(),
                    remote_name: None,
                    role: None,
                })
                .collect(),
        );
        ctx.message_detail.as_mut().unwrap().account_id = 1;
        let targets = ctx.move_targets(Language::Portuguese);
        let paths: Vec<&str> = targets.iter().map(|(_, path)| path.as_ref()).collect();
        assert_eq!(paths, vec![system::ARCHIVE, system::TRASH]);
        assert_eq!(targets[1].0.as_ref(), "Lixeira");
    }

    #[test]
    fn flag_label_reflects_starred_state() {
        let starred = ctx_with_message(false, true);
        let label = label_for(&CommandId::MessageToggleFlag, Language::English, &starred);
        assert_eq!(label.as_ref(), "Remove star");
    }

    #[test]
    fn command_filter_is_case_insensitive() {
        assert!(command_matches_query("Archive Message", "ARCH"));
        assert!(!command_matches_query("Archive Message", "junk"));
    }

    #[test]
    fn move_targets_exclude_sent_drafts_and_flagged() {
        let ctx = ctx_with_message(false, false);
        let paths: Vec<_> = ctx
            .move_targets(Language::English)
            .into_iter()
            .map(|(_, path)| path.to_string())
            .collect();
        assert!(!paths.contains(&system::SENT.to_string()));
        assert!(!paths.contains(&system::DRAFTS.to_string()));
        assert!(!paths.contains(&system::FLAGGED.to_string()));
        assert!(paths.contains(&user_folder_path("Clients")));
    }
}
