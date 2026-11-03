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

/// Stands for a `,` of a path inside `folders_csv`, whose separator it is: a
/// server's folder can be called "Clients, VIP". No folder name has this control
/// character, so nothing else needs escaping.
const CSV_COMMA: char = '\u{1f}';

/// `path` as it is written inside `folders_csv`.
pub(crate) fn encode_csv_path(path: &str) -> String {
    path.replace(',', &CSV_COMMA.to_string())
}

/// The path written as `segment` inside `folders_csv`.
pub(crate) fn decode_csv_path(segment: &str) -> String {
    segment.replace(CSV_COMMA, ",")
}

/// Builds the `folders_csv` value: `,sys:inbox,sys:flagged,`.
pub fn folders_csv<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let inner = paths
        .into_iter()
        .map(encode_csv_path)
        .collect::<Vec<_>>()
        .join(",");
    format!(",{inner},")
}

/// What `instr(folders_csv, ?)` looks for to find a folder path. Not `LIKE`: a
/// server's `My_Work` would also match `MyXWork`, and `work` would match `WORK`.
pub fn folder_csv_needle(folder_path: &str) -> String {
    format!(",{},", encode_csv_path(folder_path))
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
    fn folder_csv_needle_matches_inside_csv() {
        assert_eq!(folder_csv_needle(system::INBOX), ",sys:inbox,");
        assert!(",sys:flagged,sys:inbox,".contains(&folder_csv_needle(system::INBOX)));
    }

    #[test]
    fn a_comma_in_a_path_stays_inside_its_segment() {
        let csv = folders_csv(["user:Clients, VIP", system::INBOX]);
        assert_eq!(csv.matches(',').count(), 3);
        assert!(csv.contains(&folder_csv_needle("user:Clients, VIP")));
        assert!(!csv.contains(&folder_csv_needle("user:Clients")));
        assert_eq!(
            decode_csv_path(&encode_csv_path("user:Clients, VIP")),
            "user:Clients, VIP"
        );
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
