//! GitHub Releases for the app's own updates: the latest release of a repository
//! (with the SHA-256 digest GitHub computes for every uploaded file) and the
//! download of one of its files, with progress.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use mail_core::{RemoteError, RemoteErrorKind};
use serde::Deserialize;

const API_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound on the API answer (the JSON of one release).
const API_LIMIT: u64 = 4 * 1024 * 1024;
const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// A published release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// The tag, e.g. `v1.2.0`.
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// One file of a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    /// `sha256:<hex>`, computed by GitHub when the file was uploaded.
    #[serde(default)]
    pub digest: Option<String>,
}

/// The latest (non-draft, non-prerelease) release of `owner/name`.
pub fn latest_release(repo: &str) -> Result<Release, RemoteError> {
    latest_release_at(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))
}

/// [`latest_release`] from the GitHub API answer at `url`.
pub fn latest_release_at(url: &str) -> Result<Release, RemoteError> {
    let mut response = crate::http::agent(API_TIMEOUT)
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(crate::http::map_error)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(RemoteError::new(
            RemoteErrorKind::Network,
            format!("{url} answered HTTP {status}"),
        ));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(API_LIMIT)
        .read_to_vec()
        .map_err(crate::http::map_error)?;
    serde_json::from_slice(&body).map_err(|err| {
        RemoteError::new(
            RemoteErrorKind::Protocol,
            format!("unexpected release JSON: {err}"),
        )
    })
}

/// Downloads `url` into `dest`, reporting the bytes received so far to `progress`.
/// The bytes go to `<dest>.part` first, so `dest` only ever holds a whole file.
pub fn download(url: &str, dest: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), RemoteError> {
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let result = download_to(url, &part, progress)
        .and_then(|()| std::fs::rename(&part, dest).map_err(crate::tls::io_error));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn download_to(url: &str, part: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), RemoteError> {
    let mut response = crate::http::download_agent()
        .get(url)
        .call()
        .map_err(crate::http::map_error)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(RemoteError::new(
            RemoteErrorKind::Network,
            format!("{url} answered HTTP {status}"),
        ));
    }
    let mut file = std::fs::File::create(part).map_err(crate::tls::io_error)?;
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut buffer = vec![0; DOWNLOAD_CHUNK];
    let mut received = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(crate::tls::io_error)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(crate::tls::io_error)?;
        received += read as u64;
        progress(received);
    }
    file.sync_all().map_err(crate::tls::io_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server;

    const RELEASE_JSON: &str = r#"{"tag_name":"v0.3.0","html_url":"x","assets":[
        {"name":"bgmail-linux-x86_64.deb","browser_download_url":"https://e/a.deb","size":12,"digest":"sha256:ab"},
        {"name":"install.sh","browser_download_url":"https://e/install.sh"}]}"#;

    #[test]
    fn the_latest_release_is_parsed() {
        let (base, log) = test_server::serve(|_| (200, RELEASE_JSON.into()));
        let release = latest_release_at(&format!("{base}/releases/latest")).unwrap();
        assert_eq!(release.tag_name, "v0.3.0");
        assert_eq!(release.assets.len(), 2);
        assert_eq!(release.assets[0].digest.as_deref(), Some("sha256:ab"));
        assert_eq!(release.assets[0].size, 12);
        assert_eq!(release.assets[1].digest, None);
        assert_eq!(log.lock().unwrap()[0].target, "/releases/latest");
    }

    #[test]
    fn a_missing_release_or_bad_json_is_an_error() {
        let (base, _) = test_server::serve(|req| match req.target.as_str() {
            "/missing" => (404, "{}".into()),
            _ => (200, "not json".into()),
        });
        let missing = latest_release_at(&format!("{base}/missing")).unwrap_err();
        assert_eq!(missing.kind, RemoteErrorKind::Network);
        let bad = latest_release_at(&format!("{base}/bad")).unwrap_err();
        assert_eq!(bad.kind, RemoteErrorKind::Protocol);
    }

    #[test]
    fn downloads_report_progress_and_leave_no_part_file() {
        let body = "x".repeat(200_000);
        let served = body.clone();
        let (base, _) = test_server::serve(move |req| match req.target.as_str() {
            "/file" => (200, served.clone()),
            _ => (404, String::new()),
        });
        let dir = std::env::temp_dir().join(format!("bgmail-release-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("file");

        let mut seen = Vec::new();
        download(&format!("{base}/file"), &dest, &mut |n| seen.push(n)).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), body);
        assert_eq!(seen.last(), Some(&(body.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0] < w[1]));

        let missing = dir.join("missing");
        assert!(download(&format!("{base}/nope"), &missing, &mut |_| {}).is_err());
        assert!(!missing.exists());
        assert!(!dir.join("missing.part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
