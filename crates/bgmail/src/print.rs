//! Printing a message: BGMail writes a standalone print page
//! (`mail_html::print_document`) to a private temporary file and opens it
//! with the system's default browser, which paginates it and shows the
//! native print dialog (printer, pages, "Save as PDF"). GPUI has no print
//! API; the browser is also the rendering reference the reader matches.
//!
//! The page is a plain copy of the message, so it lives in a per-user
//! directory only its owner can read, and pages older than a few minutes are
//! removed whenever another message is printed.

use std::fs::{File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Print pages older than this are deleted on the next print (the browser
/// has long read them by then).
const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

/// Directory holding the print pages.
pub fn print_dir() -> PathBuf {
    handoff_dir("print")
}

/// Where files for other programs to open go. Linux's temporary directory is shared,
/// so another user could already own a name there, keeping these from being written
/// or, with symlink protection off, redirecting them. macOS and Windows have one per
/// user.
pub(crate) fn handoff_dir(name: &str) -> PathBuf {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    if let Some(cache) = user_cache_dir(
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        crate::config::home_dir(),
    ) {
        return cache.join("BGMail").join(name);
    }
    std::env::temp_dir().join(format!("bgmail-{name}"))
}

/// The XDG cache directory; a relative `XDG_CACHE_HOME` is invalid per the spec.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn user_cache_dir(xdg_cache_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_cache_home
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".cache")))
}

/// A fresh value for the page's CSP nonce and file name: 64 bits from the
/// process's random hasher keys, the clock and a counter (unguessable enough
/// for a nonce guarding an already sanitized page).
pub fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    format!("{:016x}", hasher.finish())
}

/// Writes `html` as a new private print page in `dir` (named after `nonce`)
/// and removes stale pages; returns the page's path.
pub fn write_print_page(dir: &Path, html: &str, nonce: &str) -> io::Result<PathBuf> {
    create_private_dir(dir)?;
    remove_stale_pages(dir, SystemTime::now(), STALE_AFTER);
    let path = dir.join(format!("message-{nonce}.html"));
    let mut file = create_private_file(&path)?;
    file.write_all(html.as_bytes())?;
    Ok(path)
}

/// Deletes the print pages in `dir` last modified more than `age` before
/// `now`; returns how many were removed. Best effort.
pub fn remove_stale_pages(dir: &Path, now: SystemTime, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("message-") && name.ends_with(".html")
        })
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > age)
        })
        .filter(|entry| std::fs::remove_file(entry.path()).is_ok())
        .count()
}

/// The print directory, readable only by its owner (Unix; Windows' temp
/// directory is already per user).
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// A new file only its owner can read (fails if it exists).
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn files_for_other_programs_stay_out_of_the_shared_temp_directory() {
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            user_cache_dir(Some(PathBuf::from("/fast/cache")), home.clone()),
            Some(PathBuf::from("/fast/cache"))
        );
        assert_eq!(
            user_cache_dir(Some(PathBuf::from("relative")), home.clone()),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(
            user_cache_dir(None, home),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(user_cache_dir(None, None), None);
    }

    #[test]
    fn nonces_differ() {
        let a = nonce();
        assert_eq!(a.len(), 16);
        assert_ne!(a, nonce());
    }

    #[test]
    fn print_pages_are_private_files() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("print");
        let path = write_print_page(&pages, "<p>hi</p>", "abc").unwrap();
        assert_eq!(path, pages.join("message-abc.html"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<p>hi</p>");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&pages), 0o700);
        }
        // Never overwrites an existing page.
        assert!(write_print_page(&pages, "x", "abc").is_err());
    }

    #[test]
    fn stale_pages_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old = write_print_page(dir.path(), "old", "old").unwrap();
        let new = write_print_page(dir.path(), "new", "new").unwrap();
        let other = dir.path().join("keep.txt");
        std::fs::write(&other, "x").unwrap();
        let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();
        let removed = remove_stale_pages(dir.path(), SystemTime::now(), STALE_AFTER);
        assert_eq!(removed, 1);
        assert!(!old.exists() && new.exists() && other.exists());
    }
}
