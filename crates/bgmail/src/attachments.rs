//! Reader attachments: how a chip presents a file (icon family, human size)
//! and the file side of "Open" / "Save As…" / "Save to Downloads".
//!
//! Attachment names come from the sender, so they are never used as given:
//! [`sanitize_filename`] strips path separators, control and reserved
//! characters (a name can't escape the target directory or hide as a dotfile)
//! and keeps the extension, which the system needs to pick a viewer. Writers
//! never overwrite: a taken name gets a ` (n)` suffix.
//!
//! Opened attachments are written to a private temporary directory (Unix
//! 0700 dir / 0600 files, like [`crate::print`]); files older than a day are
//! removed when another attachment is opened.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ui::IconName;

use crate::locale::Language;

/// Opened attachments older than this are deleted on the next open (the
/// viewer has long loaded them by then).
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// Longest file stem kept (characters); the extension is kept whole.
const MAX_STEM_CHARS: usize = 120;
/// Name used when nothing usable is left of the sender's file name.
const FALLBACK_NAME: &str = "attachment";
/// How many ` (n)` suffixes are tried before giving up.
const MAX_DEDUPE: usize = 1000;

/// Broad kind of an attachment, for its chip icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFamily {
    Image,
    Pdf,
    Archive,
    Audio,
    Video,
    Text,
    Other,
}

impl FileFamily {
    /// The chip icon for this family.
    pub fn icon(self) -> IconName {
        match self {
            FileFamily::Image => IconName::FileImage,
            FileFamily::Pdf => IconName::FilePdf,
            FileFamily::Archive => IconName::FileArchive,
            FileFamily::Audio => IconName::FileAudio,
            FileFamily::Video => IconName::FileVideo,
            FileFamily::Text => IconName::FileText,
            FileFamily::Other => IconName::File,
        }
    }
}

/// Family of an attachment from its MIME type, falling back to the file
/// extension when the type is generic (`application/octet-stream`).
pub fn file_family(mime_type: &str, filename: &str) -> FileFamily {
    let mime = mime_type.trim().to_ascii_lowercase();
    let (top, sub) = mime.split_once('/').unwrap_or((mime.as_str(), ""));
    let by_mime = match top {
        "image" => Some(FileFamily::Image),
        "audio" => Some(FileFamily::Audio),
        "video" => Some(FileFamily::Video),
        "text" => Some(FileFamily::Text),
        "application" => match sub {
            "pdf" => Some(FileFamily::Pdf),
            "zip" | "gzip" | "x-gzip" | "x-tar" | "x-7z-compressed" | "x-rar-compressed"
            | "vnd.rar" | "x-bzip2" | "x-xz" | "zstd" => Some(FileFamily::Archive),
            "msword"
            | "rtf"
            | "vnd.oasis.opendocument.text"
            | "vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Some(FileFamily::Text)
            }
            _ => None,
        },
        _ => None,
    };
    by_mime.unwrap_or_else(|| family_from_extension(filename))
}

fn family_from_extension(filename: &str) -> FileFamily {
    let extension = Path::new(filename)
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "heic" | "tif" | "tiff" => {
            FileFamily::Image
        }
        "pdf" => FileFamily::Pdf,
        "zip" | "gz" | "tgz" | "tar" | "7z" | "rar" | "bz2" | "xz" | "zst" => FileFamily::Archive,
        "mp3" | "wav" | "ogg" | "m4a" | "flac" | "aac" | "opus" => FileFamily::Audio,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => FileFamily::Video,
        "txt" | "md" | "csv" | "rtf" | "doc" | "docx" | "odt" | "log" => FileFamily::Text,
        _ => FileFamily::Other,
    }
}

/// Human-readable size: bytes below 1 KB, then KB/MB/GB/TB (base 1024) with
/// one decimal under 10 ("1.2 MB", "15 KB"). Portuguese uses a decimal comma.
pub fn human_size(bytes: u64, language: Language) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let number = if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    let number = match language {
        Language::English => number,
        Language::Portuguese => number.replace('.', ","),
    };
    format!("{number} {}", UNITS[unit])
}

/// A file name safe to create on every platform: only the last path segment
/// of `name`, with control characters and `/ \ : * ? " < > |` replaced by
/// `_`, surrounding spaces and dots trimmed, Windows device names (`CON`,
/// `NUL`, …) prefixed with `_`, and the stem shortened to
/// [`MAX_STEM_CHARS`] while keeping the extension. Never empty.
pub fn sanitize_filename(name: &str) -> String {
    // Only the last segment: "../../etc/passwd" and "C:\x\y.pdf" lose their
    // directories instead of becoming long underscore names.
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if trimmed.is_empty() {
        return FALLBACK_NAME.to_string();
    }
    let (stem, extension) = split_extension(trimmed);
    let mut stem: String = stem.chars().take(MAX_STEM_CHARS).collect();
    let device = stem.to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((device.starts_with("COM") || device.starts_with("LPT"))
            && device.len() == 4
            && device.as_bytes()[3].is_ascii_digit());
    if reserved {
        stem.insert(0, '_');
    }
    match extension {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem,
    }
}

/// `(stem, extension)` of a name; a leading dot is not an extension and an
/// overly long "extension" is treated as part of the stem.
fn split_extension(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot - 1 <= 16 => {
            (&name[..dot], Some(&name[dot + 1..]))
        }
        _ => (name, None),
    }
}

/// The `n`-th candidate for `name`: the name itself, then `stem (n).ext`.
fn numbered_name(name: &str, n: usize) -> String {
    if n == 0 {
        return name.to_string();
    }
    match split_extension(name) {
        (stem, Some(extension)) => format!("{stem} ({n}).{extension}"),
        (stem, None) => format!("{stem} ({n})"),
    }
}

/// Decoded bytes of part `part_index` of the raw message `raw` (the
/// `part_index` stored with the attachment row).
pub fn extract(raw: &[u8], part_index: u32) -> io::Result<Vec<u8>> {
    mail_core::attachment_content(raw, part_index as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "attachment part not found"))
}

/// Directory holding attachments opened from the reader.
pub fn open_dir() -> PathBuf {
    crate::print::handoff_dir("attachments")
}

/// Where "Save" dialogs start and "Save to Downloads" writes: the user's
/// `Downloads` folder when it exists, else the home folder, else the
/// temporary directory.
pub fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    downloads_dir_in(home.as_deref())
}

fn downloads_dir_in(home: Option<&Path>) -> PathBuf {
    match home {
        Some(home) if home.join("Downloads").is_dir() => home.join("Downloads"),
        Some(home) if home.is_dir() => home.to_path_buf(),
        _ => std::env::temp_dir(),
    }
}

/// Writes `data` as a new file named after `filename` (sanitized, ` (n)`
/// added while the name is taken) in `dir`; returns its path. With
/// `private`, the directory is made owner-only and the file 0600 (Unix).
pub fn write_unique(dir: &Path, filename: &str, data: &[u8], private: bool) -> io::Result<PathBuf> {
    if private {
        crate::print::create_private_dir(dir)?;
    } else {
        std::fs::create_dir_all(dir)?;
    }
    let name = sanitize_filename(filename);
    for n in 0..MAX_DEDUPE {
        let path = dir.join(numbered_name(&name, n));
        let file = if private {
            crate::print::create_private_file(&path)
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
        };
        match file {
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free file name",
    ))
}

/// Writes an attachment the user is opening into the private [`open_dir`]
/// (after removing stale ones); returns the file to hand to the system.
pub fn write_for_open(filename: &str, data: &[u8]) -> io::Result<PathBuf> {
    let dir = open_dir();
    // First: cleaning up must not run in a directory that turns out not to be ours.
    crate::print::create_private_dir(&dir)?;
    remove_stale_files(&dir, SystemTime::now(), STALE_AFTER);
    write_unique(&dir, filename, data, true)
}

/// Deletes the files in `dir` last modified more than `age` before `now`;
/// returns how many were removed. Best effort.
pub fn remove_stale_files(dir: &Path, now: SystemTime, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > age)
        })
        .filter(|entry| std::fs::remove_file(entry.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_come_from_mime_then_extension() {
        assert_eq!(file_family("image/png", "x"), FileFamily::Image);
        assert_eq!(file_family("APPLICATION/PDF", "x"), FileFamily::Pdf);
        assert_eq!(file_family("application/zip", "x"), FileFamily::Archive);
        assert_eq!(file_family("audio/mpeg", "x"), FileFamily::Audio);
        assert_eq!(file_family("video/mp4", "x"), FileFamily::Video);
        assert_eq!(file_family("text/plain", "x"), FileFamily::Text);
        assert_eq!(
            file_family("application/octet-stream", "Scan.PDF"),
            FileFamily::Pdf
        );
        assert_eq!(file_family("", "photo.jpeg"), FileFamily::Image);
        assert_eq!(
            file_family("application/octet-stream", "data.bin"),
            FileFamily::Other
        );
        assert_eq!(FileFamily::Other.icon(), IconName::File);
        assert_eq!(FileFamily::Pdf.icon(), IconName::FilePdf);
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(0, Language::English), "0 B");
        assert_eq!(human_size(1023, Language::English), "1023 B");
        assert_eq!(human_size(1024, Language::English), "1.0 KB");
        assert_eq!(human_size(15 * 1024, Language::English), "15 KB");
        assert_eq!(human_size(1_258_291, Language::English), "1.2 MB");
        assert_eq!(human_size(1_258_291, Language::Portuguese), "1,2 MB");
        assert_eq!(
            human_size(3 * 1024 * 1024 * 1024, Language::English),
            "3.0 GB"
        );
    }

    #[test]
    fn sanitized_names_cannot_escape_or_hide() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Users\\x\\a.doc"), "a.doc");
        assert_eq!(sanitize_filename("a\u{0}b\nc.txt"), "a_b_c.txt");
        assert_eq!(sanitize_filename("what?<>|*.png"), "what_____.png");
        assert_eq!(sanitize_filename(".bashrc"), "bashrc");
        assert_eq!(sanitize_filename("  trailing. . "), "trailing");
        assert_eq!(sanitize_filename(""), FALLBACK_NAME);
        assert_eq!(sanitize_filename("../.."), FALLBACK_NAME);
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("LPT1"), "_LPT1");
        assert_eq!(sanitize_filename("Console.txt"), "Console.txt");
    }

    #[test]
    fn long_names_keep_their_extension() {
        let long = format!("{}.pdf", "a".repeat(400));
        let clean = sanitize_filename(&long);
        assert!(clean.ends_with(".pdf"));
        assert_eq!(clean.chars().count(), MAX_STEM_CHARS + 4);
    }

    #[test]
    fn numbered_names_go_before_the_extension() {
        assert_eq!(numbered_name("a.pdf", 0), "a.pdf");
        assert_eq!(numbered_name("a.pdf", 2), "a (2).pdf");
        assert_eq!(numbered_name("README", 1), "README (1)");
        assert_eq!(numbered_name("a.tar.gz", 1), "a.tar (1).gz");
    }

    #[test]
    fn unique_writes_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let first = write_unique(dir.path(), "a/b.txt", b"one", false).unwrap();
        let second = write_unique(dir.path(), "b.txt", b"two", false).unwrap();
        assert_eq!(first, dir.path().join("b.txt"));
        assert_eq!(second, dir.path().join("b (1).txt"));
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
    }

    #[test]
    fn private_writes_are_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("open");
        let path = write_unique(&target, "x.pdf", b"%PDF", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&target), 0o700);
        }
    }

    #[test]
    fn stale_open_files_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old = write_unique(dir.path(), "old.txt", b"o", false).unwrap();
        let new = write_unique(dir.path(), "new.txt", b"n", false).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 3600))
            .unwrap();
        assert_eq!(
            remove_stale_files(dir.path(), SystemTime::now(), STALE_AFTER),
            1
        );
        assert!(!old.exists() && new.exists());
    }

    #[test]
    fn downloads_prefers_the_downloads_folder() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(downloads_dir_in(Some(home.path())), home.path());
        std::fs::create_dir(home.path().join("Downloads")).unwrap();
        assert_eq!(
            downloads_dir_in(Some(home.path())),
            home.path().join("Downloads")
        );
        assert_eq!(downloads_dir_in(None), std::env::temp_dir());
        assert!(!downloads_dir().as_os_str().is_empty());
    }

    const RAW_WITH_ATTACHMENT: &str = "From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: Files\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"XX\"\r\n\
\r\n\
--XX\r\n\
Content-Type: text/plain\r\n\
\r\n\
Hello\r\n\
--XX\r\n\
Content-Type: application/pdf; name=\"a.pdf\"\r\n\
Content-Disposition: attachment; filename=\"a.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQ=\r\n\
--XX--\r\n";

    #[test]
    fn extract_decodes_the_stored_part() {
        let raw = RAW_WITH_ATTACHMENT.as_bytes();
        let parsed = mail_core::parse_message(raw).expect("parses");
        let info = parsed.attachments.first().expect("one attachment");
        let data = extract(raw, info.index as u32).unwrap();
        assert_eq!(data, b"%PDF-1.4");
        assert!(extract(raw, 999).is_err());
    }

    #[test]
    fn write_for_open_creates_a_private_copy() {
        let path = write_for_open("../x/report.txt", b"hi").unwrap();
        assert!(path.starts_with(open_dir()));
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("report"));
        assert_eq!(std::fs::read(&path).unwrap(), b"hi");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn open_dir_is_named_for_attachments() {
        assert!(open_dir().ends_with("attachments") || open_dir().ends_with("bgmail-attachments"));
    }
}
