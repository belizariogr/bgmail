//! Remembers which message the reader was rendering, so a crash while
//! rendering it (a renderer panic, stack overflow or out-of-memory, which all
//! abort the process) doesn't repeat every time the app restarts: the next
//! start shows that message as plain text instead.
//!
//! The marker is a tiny file next to the mail database, written before a
//! formatted body is rendered and removed once it has painted with nothing left to
//! load: a render that finished is safe, so a kill or a closed window after it leaves
//! nothing behind. While the message keeps showing, hovering, selecting and zooming
//! run the renderer again; a panic on the UI thread then writes the marker back
//! ([`install_panic_hook`]), so that message opens as text next time too.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};

/// The message each guard has showing, by marker path, kept in memory once its render
/// finished so the panic hook can put its marker back.
static SHOWING: Mutex<Vec<(PathBuf, i64)>> = Mutex::new(Vec::new());

/// Writes back, on a panic of the calling thread (the UI thread, where the renderer
/// runs again for hover, selection and zoom), the markers of the messages showing.
/// Other threads' panics (sync, network) aren't a message's fault.
pub fn install_panic_hook() {
    let ui_thread = std::thread::current().id();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == ui_thread {
            write_showing_markers(|_| true);
        }
        previous(info);
    }));
}

/// Puts the markers of the showing messages whose path passes `which` back on disk.
fn write_showing_markers(which: impl Fn(&Path) -> bool) {
    // Never block inside a panic: a lock held by this very thread would deadlock.
    let showing = match SHOWING.try_lock() {
        Ok(showing) => showing,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    for (path, id) in showing.iter().filter(|(path, _)| which(path)) {
        let _ = std::fs::write(path, id.to_string());
    }
}

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
        // Memory first, so a panic hook writing in between can't put the previous id back.
        self.set_showing(Some(id));
        // Best effort: without the marker only the crash-loop protection is lost.
        let _ = std::fs::write(&self.path, id.to_string());
    }

    /// Message `id` finished rendering: its marker leaves the disk, and stays in memory
    /// for the panic hook while it shows. Nothing when the guard has moved on to another
    /// message, whose render may still be running.
    pub fn rendered(&self, id: i64) {
        if self.showing() == Some(id) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// [`Self::finish`] when a view of message `id` goes away, unless the guard is by now
    /// that of another message, still showing.
    pub fn finish_if(&self, id: i64) {
        if self.showing() == Some(id) {
            self.finish();
        }
    }

    /// Records that nothing of the guard's kind is being rendered any more.
    pub fn finish(&self) {
        // Memory first: a panic hook writing in between can't bring the file back.
        self.set_showing(None);
        let _ = std::fs::remove_file(&self.path);
    }

    /// The message this guard has showing (marker on disk or, once rendered, in memory).
    pub fn showing(&self) -> Option<i64> {
        let showing = SHOWING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        showing
            .iter()
            .find(|(path, _)| *path == self.path)
            .map(|(_, id)| *id)
    }

    fn set_showing(&self, id: Option<i64>) {
        let mut showing = SHOWING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        showing.retain(|(path, _)| *path != self.path);
        if let Some(id) = id {
            showing.push((self.path.clone(), id));
        }
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
    fn a_finished_render_leaves_the_disk_until_a_panic_while_it_shows() {
        let dir = tempfile::tempdir().unwrap();
        let guard = CrashGuard::beside(&dir.path().join("mail.db"));
        guard.begin(5);
        guard.rendered(5);
        // Killed now: nothing to report, the render had finished.
        assert!(!dir.path().join("reader-render.guard").exists());
        assert_eq!(guard.showing(), Some(5));
        // A panic on the UI thread while it shows puts the marker back (this guard's
        // only: tests running beside it have markers of their own).
        let own = |path: &Path| path == guard.path;
        write_showing_markers(own);
        assert_eq!(guard.take_unfinished(), Some(5));
        // Gone from memory too: a later panic blames nothing.
        write_showing_markers(own);
        assert_eq!(guard.take_unfinished(), None);
    }

    #[test]
    fn a_late_render_of_another_message_keeps_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let guard = CrashGuard::beside(&dir.path().join("mail.db"));
        guard.begin(1);
        guard.begin(2);
        guard.rendered(1);
        assert_eq!(guard.take_unfinished(), Some(2));
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
