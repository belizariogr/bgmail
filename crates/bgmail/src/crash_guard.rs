//! Keeps a message that crashes the renderer from crashing the app on every start: the
//! next start shows it as plain text instead.
//!
//! What it guards against is a crash loop (the reader reopening that message at startup),
//! not every way the process can end: a closed window, a logout or a kill from a task
//! manager isn't the message's fault, and must not cost it its formatting. So:
//!
//! - While a formatted message is being built and first painted, its id is in a tiny
//!   marker file next to the mail database ([`CrashGuard::begin`]). Whatever ends the
//!   process in that short window (panic, stack overflow, out of memory, a kill) leaves
//!   it, and the next start shows the message as text, once, with a way back.
//! - Once it painted with nothing left to load ([`RenderMark::rendered`]) it is safe: the
//!   id leaves the disk and stays only in memory while the message shows. A panic on the
//!   UI thread then (hover, selection, scrolling and zoom run the renderer again) writes
//!   it back ([`install_panic_hook`]). Anything else after that blames nothing: it can't
//!   loop, since a start doesn't repeat the scrolling or resizing that hit it.
//!
//! A marker file lists one id per line: several reply windows can each render a quote.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};

/// A render in progress or showing: which marker it belongs to, the message, and whether
/// its first paint finished (then it is only in memory).
struct Mark {
    path: PathBuf,
    token: u64,
    id: i64,
    rendered: bool,
}

/// Every live [`RenderMark`], so the panic hook can write them back.
static MARKS: Mutex<Vec<Mark>> = Mutex::new(Vec::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

fn marks() -> MutexGuard<'static, Vec<Mark>> {
    MARKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Writes `path`'s marker from the marks that pass `keep`, or removes it when none does.
/// Best effort: without the marker only the crash-loop protection is lost.
fn write_marker(path: &Path, marks: &[Mark], keep: impl Fn(&Mark) -> bool) {
    let ids: Vec<String> = marks
        .iter()
        .filter(|mark| mark.path == path && keep(mark))
        .map(|mark| mark.id.to_string())
        .collect();
    if ids.is_empty() {
        let _ = std::fs::remove_file(path);
    } else {
        let _ = std::fs::write(path, ids.join("\n"));
    }
}

/// On a panic of the calling thread (the UI thread, where the renderer runs again for
/// hover, selection, scrolling and zoom), writes back the markers of every message
/// showing. Other threads' panics (sync, network) aren't a message's fault.
pub fn install_panic_hook() {
    install_panic_hook_for(|_| true);
}

/// [`install_panic_hook`] for the markers whose path passes `which` (tests keep to their own).
fn install_panic_hook_for(which: impl Fn(&Path) -> bool + Send + Sync + 'static) {
    let ui_thread = std::thread::current().id();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == ui_thread {
            write_back_all(&which);
        }
        previous(info);
    }));
}

/// Puts every mark whose path passes `which` back on disk, rendered or not.
fn write_back_all(which: &dyn Fn(&Path) -> bool) {
    // Never block inside a panic: a lock held by this very thread would deadlock.
    let marks = match MARKS.try_lock() {
        Ok(marks) => marks,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    let mut paths: Vec<&Path> = marks.iter().map(|mark| mark.path.as_path()).collect();
    paths.sort();
    paths.dedup();
    for path in paths.into_iter().filter(|path| which(path)) {
        write_marker(path, &marks, |_| true);
    }
}

/// A marker file: the reader's, or that of quotes in reply windows.
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

    /// The messages whose render never finished in the last run; the marker is cleared.
    pub fn take_unfinished(&self) -> Vec<i64> {
        let ids = std::fs::read_to_string(&self.path)
            .map(|text| {
                text.lines()
                    .filter_map(|id| id.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let _ = std::fs::remove_file(&self.path);
        ids
    }

    /// Message `id` is about to be rendered as formatted HTML: on disk until the returned
    /// mark is [`RenderMark::rendered`], tracked until it is dropped.
    pub fn begin(&self, id: i64) -> RenderMark {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let mut marks = marks();
        marks.push(Mark {
            path: self.path.clone(),
            token,
            id,
            rendered: false,
        });
        write_marker(&self.path, &marks, |mark| !mark.rendered);
        RenderMark {
            path: self.path.clone(),
            token,
        }
    }

    /// The app is quitting on its own: nothing rendering made it crash, whatever views
    /// still hold marks (on Linux they may be gone, or not yet, when the quit hooks run).
    pub fn finish_all(&self) {
        marks().retain(|mark| mark.path != self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One formatted render of a message, from before it is parsed until its view goes.
#[derive(Debug)]
pub struct RenderMark {
    path: PathBuf,
    token: u64,
}

impl RenderMark {
    /// The message rendered.
    #[cfg(test)]
    pub fn id(&self) -> i64 {
        marks()
            .iter()
            .find(|mark| mark.token == self.token)
            .map_or(-1, |mark| mark.id)
    }

    /// Its first paint finished with nothing left to load: safe from now on, so a kill or
    /// a closed window leaves nothing; only a panic while it shows puts it back.
    pub fn rendered(&self) {
        let mut marks = marks();
        let Some(mark) = marks.iter_mut().find(|mark| mark.token == self.token) else {
            return;
        };
        if !mark.rendered {
            mark.rendered = true;
            write_marker(&self.path, &marks, |mark| !mark.rendered);
        }
    }
}

impl Drop for RenderMark {
    fn drop(&mut self) {
        let mut marks = marks();
        let before = marks.len();
        marks.retain(|mark| mark.token != self.token);
        // Gone already when the app quit (`finish_all`): so is the file.
        if marks.len() != before {
            write_marker(&self.path, &marks, |mark| !mark.rendered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(dir: &tempfile::TempDir) -> CrashGuard {
        CrashGuard::beside(&dir.path().join("mail.db"))
    }

    #[test]
    fn a_render_cut_short_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let guard = reader(&dir);
        assert!(guard.take_unfinished().is_empty());
        // The process ends before the render finished: the mark never goes.
        std::mem::forget(guard.begin(42));
        assert_eq!(guard.take_unfinished(), vec![42]);
        assert!(guard.take_unfinished().is_empty());
        guard.finish_all();
    }

    #[test]
    fn a_finished_render_leaves_nothing_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let guard = reader(&dir);
        let mark = guard.begin(5);
        assert_eq!(mark.id(), 5);
        mark.rendered();
        // Killed or closed now: nothing to report.
        assert!(!dir.path().join("reader-render.guard").exists());
        drop(mark);
        assert!(!dir.path().join("reader-render.guard").exists());
    }

    #[test]
    fn a_mark_that_goes_clears_only_its_own_message() {
        let dir = tempfile::tempdir().unwrap();
        let guard = reader(&dir);
        let first = guard.begin(7);
        let second = guard.begin(8);
        drop(first);
        assert_eq!(guard.take_unfinished(), vec![8]);
        drop(second);
        assert!(guard.take_unfinished().is_empty());
    }

    #[test]
    fn one_quote_rendering_doesnt_clear_another_still_building() {
        let dir = tempfile::tempdir().unwrap();
        let quotes = CrashGuard::beside_for_quotes(&dir.path().join("mail.db"));
        let building = quotes.begin(2);
        let done = quotes.begin(1);
        done.rendered();
        assert_eq!(quotes.take_unfinished(), vec![2]);
        drop((building, done));
    }

    #[test]
    fn quotes_keep_a_marker_of_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mail.db");
        let (reader, quotes) = (CrashGuard::beside(&db), CrashGuard::beside_for_quotes(&db));
        let _read = reader.begin(1);
        let _quoted = quotes.begin(2);
        assert_eq!(reader.take_unfinished(), vec![1]);
        assert_eq!(quotes.take_unfinished(), vec![2]);
    }

    #[test]
    fn quitting_clears_marks_views_still_hold() {
        let dir = tempfile::tempdir().unwrap();
        let guard = reader(&dir);
        let mark = guard.begin(3);
        guard.finish_all();
        assert!(guard.take_unfinished().is_empty());
        // The view going later doesn't bring it back.
        drop(mark);
        assert!(!dir.path().join("reader-render.guard").exists());
    }

    #[test]
    fn a_panic_on_the_ui_thread_writes_back_what_shows() {
        let dir = tempfile::tempdir().unwrap();
        let guard = reader(&dir);
        let path = dir.path().join("reader-render.guard");
        let shown = guard.begin(9);
        shown.rendered();
        let own = path.clone();
        // This thread plays the UI thread; a panic on another one blames nothing.
        std::thread::spawn(move || {
            install_panic_hook_for(move |marker| marker == own);
            let _ = std::thread::spawn(|| panic!("a sync thread")).join();
            assert!(!path.exists());
            let _ = std::panic::catch_unwind(|| panic!("the renderer, while hovering"));
            assert!(path.exists());
        })
        .join()
        .unwrap();
        assert_eq!(guard.take_unfinished(), vec![9]);
        drop(shown);
    }
}
