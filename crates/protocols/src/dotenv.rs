//! Minimal `.env` reader, shared by `build.rs` (through `#[path]`) and the
//! unit tests: build scripts can't have tests of their own.

use std::io;
use std::path::Path;

/// Contents of the `.env` at `path`, creating an empty one if it's missing.
pub fn read_or_create(path: &Path) -> io::Result<String> {
    // Append mode: an existing file keeps its contents and its mtime.
    std::fs::File::options()
        .append(true)
        .create(true)
        .open(path)?;
    std::fs::read_to_string(path)
}

/// Value of `key` in `.env` text: `KEY=value`, optionally `export KEY=value`
/// and wrapped in quotes. Blank values, comments and other keys are skipped;
/// the last assignment wins.
pub fn lookup(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let (name, value) = line.split_once('=')?;
            let name = name.trim();
            let name = name.strip_prefix("export ").map_or(name, str::trim);
            (name == key).then(|| unquote(value.trim()).to_string())
        })
        .rfind(|value| !value.is_empty())
}

fn unquote(value: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_or_create_creates_missing_and_keeps_existing() {
        let dir = std::env::temp_dir().join(format!("bgmail-dotenv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");

        assert_eq!(read_or_create(&path).unwrap(), "");
        assert!(path.exists());

        std::fs::write(&path, "KEY=v\n").unwrap();
        assert_eq!(read_or_create(&path).unwrap(), "KEY=v\n");

        // Unwritable location (no parent directory): the caller gets the error.
        assert!(read_or_create(&dir.join("missing/.env")).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lookup_reads_plain_exported_and_quoted_values() {
        let text = "# KEY=commented\nOTHER=x\nKEY=a=b\n";
        assert_eq!(lookup(text, "KEY").as_deref(), Some("a=b"));
        assert_eq!(lookup("export KEY=v", "KEY").as_deref(), Some("v"));
        assert_eq!(lookup("KEY = \"v w\"", "KEY").as_deref(), Some("v w"));
        assert_eq!(lookup("KEY='v'\r\n", "KEY").as_deref(), Some("v"));
    }

    #[test]
    fn lookup_skips_missing_and_blank_values() {
        assert_eq!(lookup("", "KEY"), None);
        assert_eq!(lookup("KEY=\nKEY2=x", "KEY"), None);
        assert_eq!(lookup("KEY=\"\"", "KEY"), None);
        assert_eq!(lookup("KEY", "KEY"), None);
        assert_eq!(lookup("KEY=a\nKEY=b", "KEY").as_deref(), Some("b"));
        assert_eq!(lookup("KEY=a\nKEY=", "KEY").as_deref(), Some("a"));
    }
}
