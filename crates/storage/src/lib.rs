//! Local SQLite persistence for accounts, folders and messages.

mod database;
mod folder;
mod message_ops;
mod schema;
mod search;
mod seed;
mod types;

pub use database::{database_path, global_folder_path, Database};
pub use folder::{
    folders_csv, is_manual_move_destination_forbidden, is_system_path, system, user_folder_path,
    SYSTEM_PREFIX, USER_PREFIX,
};
pub use search::{build_search_text, fold_for_search, search_like_pattern};
pub use seed::{
    plain_text_from_raw, preview_from_plain, seed, seed_if_empty, SeedAccount, SeedMailbox,
    SeedMessage,
};
pub use types::{Account, Folder, MailListQuery, MessageDetail, MessageListItem};
