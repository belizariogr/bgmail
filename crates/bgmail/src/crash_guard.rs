//! Remembers which message the reader was rendering, so a crash while
//! rendering it (a renderer panic, stack overflow or out-of-memory, which all
//! abort the process) doesn't repeat every time the app restarts: the next
//! start shows that message as plain text instead.
//!
//! The marker is a tiny file next to the mail database, written before a
//! formatted body is rendered and removed once a frame showing it was drawn.

use std::path::{Path, PathBuf};

/// The reader's render marker file.
#[derive(Clone, Debug)]
pub struct CrashGuard {
    path: PathBuf,
}

impl CrashGuard {
    /// The guard for the mail database at `db_path` (marker beside it).
    pub fn beside(db_path: &Path) -> Self {
        Self {
            path: db_path.with_file_name("reader-render.guard"),
        }
    }

    /// The message whose rendering never finished in the last run, if any;
    /// the marker is cleared.
    pub fn take_unfinished(&self) -> Option<i64> {
        let id = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        self.finish();
        id
    }

    /// Records that message `id` is about to be rendered as formatted HTML.
    pub fn begin(&self, id: i64) {
        // Best effort: without the marker only the crash-loop protection is lost.
        let _ = std::fs::write(&self.path, id.to_string());
    }

    /// [`Self::finish`] for a build of message `id` that won't be shown, unless the marker
    /// is by now that of another message, still being rendered.
    pub fn finish_if(&self, id: i64) {
        let current = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        if current == Some(id) {
            self.finish();
        }
    }

    /// Records that the rendering finished (a frame showing it was drawn).
    pub fn finish(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_renders_are_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let guard = CrashGuard::beside(&dir.path().join("mail.db"));
        assert_eq!(guard.take_unfinished(), None);
        guard.begin(42);
        // The process "crashed" here: the next start sees message 42.
        assert_eq!(guard.take_unfinished(), Some(42));
        assert_eq!(guard.take_unfinished(), None);
    }

    #[test]
    fn an_abandoned_build_only_clears_its_own_marker() {
        let dir = tempfile::tempdir().unwrap();
        let guard = CrashGuard::beside(&dir.path().join("mail.db"));
        guard.begin(7);
        guard.finish_if(8);
        assert_eq!(guard.take_unfinished(), Some(7));
        guard.begin(7);
        guard.finish_if(7);
        assert_eq!(guard.take_unfinished(), None);
    }

    #[test]
    fn finished_renders_leave_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        let guard = CrashGuard::beside(&dir.path().join("mail.db"));
        guard.begin(7);
        guard.finish();
        assert_eq!(guard.take_unfinished(), None);
        assert!(!dir.path().join("reader-render.guard").exists());
    }
}
