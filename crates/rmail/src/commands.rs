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
    MoveToFolder(SharedString),
}

/// Snapshot of UI state used to decide which commands are available.
#[derive(Debug, Clone, Default)]
pub struct CommandContext {
    pub selected_message_id: Option<i64>,
    pub message_detail: Option<MessageDetail>,
    pub folders_by_account: HashMap<i64, Vec<Folder>>,
}

impl CommandContext {
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

    pub fn message_in_archive(&self) -> bool {
        self.message_detail
            .as_ref()
            .is_some_and(|detail| message_has_folder(&detail.folders_csv, system::ARCHIVE))
    }

    pub fn message_starred(&self) -> bool {
        self.message_detail
            .as_ref()
            .is_some_and(|detail| detail.starred)
    }

    pub fn move_targets(&self) -> Vec<(SharedString, SharedString)> {
        let Some(detail) = &self.message_detail else {
            return Vec::new();
        };
        self.folders_by_account
            .get(&detail.account_id)
            .into_iter()
            .flatten()
            .filter(|folder| !is_manual_move_destination_forbidden(&folder.path))
            .map(|folder| {
                (
                    folder_display_name(folder, Language::English).into(),
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
        CommandId::ComposeNew | CommandId::OpenSettings | CommandId::ToggleSidebar => true,
        CommandId::MessageDelete => ctx.selected_message_id.is_some() && !ctx.message_in_trash(),
        CommandId::MessageDeletePermanent | CommandId::MessageRestore => {
            ctx.selected_message_id.is_some() && ctx.message_in_trash()
        }
        CommandId::MessageArchive => {
            ctx.selected_message_id.is_some()
                && !ctx.message_in_trash()
                && !ctx.message_in_archive()
        }
        CommandId::MessageMarkJunk => {
            ctx.selected_message_id.is_some() && !ctx.message_in_trash() && !ctx.message_in_junk()
        }
        CommandId::MessageToggleFlag | CommandId::MoveToFolder(_) => {
            ctx.selected_message_id.is_some() && !ctx.message_in_trash()
        }
    }
}

/// All commands for the palette, filtered by selection and availability.
pub fn palette_commands(language: Language, ctx: &CommandContext) -> Vec<CommandEntry> {
    let has_message = ctx.selected_message_id.is_some();
    let mut ids = vec![
        CommandId::ComposeNew,
        CommandId::OpenSettings,
        CommandId::ToggleSidebar,
    ];
    if has_message {
        ids.extend([
            CommandId::MessageDelete,
            CommandId::MessageDeletePermanent,
            CommandId::MessageRestore,
            CommandId::MessageArchive,
            CommandId::MessageMarkJunk,
            CommandId::MessageToggleFlag,
        ]);
        for (_label, path) in ctx.move_targets() {
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
                },
                Folder {
                    id: 2,
                    account_id,
                    path: user_folder_path("Clients"),
                    display_name: "Clients".into(),
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
                unread: false,
                starred,
                has_attachment: false,
                folders_csv: folders_csv.into(),
            }),
            folders_by_account,
        }
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

    #[test]
    fn flag_label_reflects_starred_state() {
        let starred = ctx_with_message(false, true);
        let label = label_for(&CommandId::MessageToggleFlag, Language::English, &starred);
        assert_eq!(label.as_ref(), "Unflag");
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
            .move_targets()
            .into_iter()
            .map(|(_, path)| path.to_string())
            .collect();
        assert!(!paths.contains(&system::SENT.to_string()));
        assert!(!paths.contains(&system::DRAFTS.to_string()));
        assert!(!paths.contains(&system::FLAGGED.to_string()));
        assert!(paths.contains(&user_folder_path("Clients")));
    }
}
