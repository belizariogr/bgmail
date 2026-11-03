//! Remembers which message the reader was rendering, so a crash while
//! rendering it (a renderer panic, stack overflow or out-of-memory, which all
//! abort the process) doesn't repeat every time the app restarts: the next
//! start shows that message as plain text instead.
//!
//! The marker is a tiny file next to the mail database, written before a
//! formatted body is rendered and removed when another message replaces it or the
//! app quits. It stays while the message shows, since hovering, selecting and zooming
//! run the renderer again; so if the app is killed, that message opens once as text.

use std::path::{Path, PathBuf};

/// The reader's render marker file.
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// The guard of quotes in reply windows: they show beside the reader, whose marker
    /// this one must not replace.
    pub fn beside_for_quotes(db_path: &Path) -> Self {
        Self {
            path: db_path.with_file_name("quote-render.guard"),
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

    /// [`Self::finish`] when a view of message `id` goes away, unless the marker is by now
    /// that of another message, still showing.
    pub fn finish_if(&self, id: i64) {
        let current = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        if current == Some(id) {
            self.finish();
        }
    }

    /// Records that nothing of the guard's kind is being rendered any more.
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
    fn a_closed_view_only_clears_its_own_marker() {
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
    fn quotes_keep_a_marker_of_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mail.db");
        let (reader, quotes) = (CrashGuard::beside(&db), CrashGuard::beside_for_quotes(&db));
        reader.begin(1);
        quotes.begin(2);
        assert_eq!(reader.take_unfinished(), Some(1));
        assert_eq!(quotes.take_unfinished(), Some(2));
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
