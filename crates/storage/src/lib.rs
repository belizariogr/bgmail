//! Local SQLite persistence for accounts, folders and messages.

mod accounts;
mod contacts;
mod database;
mod folder;
mod membership;
mod message_ops;
mod migration;
mod queue;
mod schema;
mod search;
mod seed;
mod sync;
#[cfg(test)]
mod test_support;
mod types;

pub use contacts::Contact;
pub use database::{database_path, global_folder_path, is_corrupt, Database};
pub use folder::{
    child_display_name, folders_csv, is_manual_move_destination_forbidden, is_system_path, system,
    user_folder_path, FOLDER_LEVEL_SEPARATOR, SYSTEM_PREFIX, USER_PREFIX,
};
pub use message_ops::{message_has_folder, parse_folders_csv};
pub use schema::SCHEMA_VERSION;
pub use search::{build_search_text, fold_for_search, search_like_pattern};
pub use seed::{
    plain_text_from_raw, preview_from_plain, seed, seed_if_empty, SeedAccount, SeedMailbox,
    SeedMessage,
};
pub use sync::{BODY_STATE_FULL, BODY_STATE_HEADERS};
pub use types::{
    Account, AttachmentRow, Folder, FolderSyncState, MailListQuery, MessageDetail, MessageListItem,
    MessageLocation, NewAttachment, NewRemoteMessage, OutgoingRow, OutgoingState, PendingOp,
    RemoteInsert, PROVIDER_GMAIL, PROVIDER_IMAP, PROVIDER_LOCAL,
};
