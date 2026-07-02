//! Folder path conventions stored in SQLite.
//!
//! System folders use the `sys:` prefix so a user-created folder with the same
//! display name never collides. Custom folders use `user:`.

/// Prefix for built-in folders that cannot be renamed or deleted.
pub const SYSTEM_PREFIX: &str = "sys:";
/// Prefix for user-created folders.
pub const USER_PREFIX: &str = "user:";

/// Built-in folder paths (stored in `folders.path` and inside `messages.folders_csv`).
pub mod system {
    pub const INBOX: &str = "sys:inbox";
    pub const DRAFTS: &str = "sys:drafts";
    pub const SENT: &str = "sys:sent";
    pub const JUNK: &str = "sys:junk";
    pub const TRASH: &str = "sys:trash";
    pub const ARCHIVE: &str = "sys:archive";
    pub const FLAGGED: &str = "sys:flagged";
}

/// Builds the `folders_csv` value: `,sys:inbox,sys:flagged,` so `LIKE '%,sys:inbox,%'`
/// is safe and unambiguous.
pub fn folders_csv<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let inner = paths.into_iter().collect::<Vec<_>>().join(",");
    format!(",{inner},")
}

/// `LIKE` pattern that matches a folder path inside [`folders_csv`].
pub fn folder_like_pattern(folder_path: &str) -> String {
    format!("%,{folder_path},%")
}

/// Path for a user-created folder from its display name.
pub fn user_folder_path(display_name: &str) -> String {
    format!("{USER_PREFIX}{display_name}")
}

/// Whether `path` refers to a system folder.
pub fn is_system_path(path: &str) -> bool {
    path.starts_with(SYSTEM_PREFIX)
}

/// System folders that must not be chosen as a manual move destination.
pub fn is_manual_move_destination_forbidden(path: &str) -> bool {
    matches!(path, system::FLAGGED | system::DRAFTS | system::SENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_csv_wraps_with_commas() {
        assert_eq!(
            folders_csv([system::INBOX, system::FLAGGED]),
            ",sys:inbox,sys:flagged,"
        );
    }

    #[test]
    fn folder_like_pattern_matches_inside_csv() {
        assert_eq!(folder_like_pattern(system::INBOX), "%,sys:inbox,%");
        assert!(",sys:flagged,sys:inbox,".contains("sys:inbox,"));
    }

    #[test]
    fn user_folder_path_uses_prefix() {
        assert_eq!(user_folder_path("Clients"), "user:Clients");
    }

    #[test]
    fn manual_move_forbidden_for_sent_drafts_and_flagged() {
        assert!(is_manual_move_destination_forbidden(system::SENT));
        assert!(is_manual_move_destination_forbidden(system::DRAFTS));
        assert!(is_manual_move_destination_forbidden(system::FLAGGED));
        assert!(!is_manual_move_destination_forbidden(system::INBOX));
        assert!(!is_manual_move_destination_forbidden(&user_folder_path(
            "Clients"
        )));
    }
}
