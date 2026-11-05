//! Mail synchronization engine: per-account worker threads that mirror a
//! server (through `mail_core::MailConnector`) into `storage`, replay the
//! UI's queued actions and send the outbox, plus the optimistic UI-side
//! [`actions`]. Blocking threads, no async runtime and no GPUI: the UI only
//! sees [`Engine`], [`Command`] and the [`SyncEvent`] channel.

#![forbid(unsafe_code)]

pub mod actions;
pub mod body;
mod contacts;
mod engine;
mod events;
pub mod plan;
mod worker;

#[cfg(test)]
mod tests;

pub use actions::ActionOutcome;
pub use body::{body_content, BodyContent};
pub use engine::{Engine, EngineOptions, STOP_TIMEOUT};
pub use events::{AccountStatus, Command, FolderRefusal, NewMailNotice, SyncEvent};
pub use plan::PendingAction;
