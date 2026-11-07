//! Updates BGMail from its GitHub Releases.
//!
//! [`check`] asks a [`ReleaseSource`] for the latest release. When it is newer
//! than this build, the file that matches how this copy was installed
//! ([`detect`]) is downloaded, checked against the SHA-256 digest GitHub
//! publishes for it, and installed:
//!
//! | Install                                   | Release file                   | Root            |
//! |-------------------------------------------|--------------------------------|-----------------|
//! | `~/.local/bgmail.app` (`install.sh`)      | `bgmail-linux-<arch>.tar.gz`   | no              |
//! | `/usr`, `/usr/local` (`install-linux-system`) | the same tarball          | yes             |
//! | `.deb` / `.rpm` / pacman package          | `bgmail-linux-<arch>.<format>` | yes             |
//! | AppImage                                  | `BGMail_<v>_<arch>.AppImage`   | if not writable |
//! | macOS `BGMail.app`                        | `BGMail_<v>_<arch>.dmg`        | if not writable |
//! | Windows per-user setup / MSI              | `-setup.exe` / `.msi`          | MSI: UAC        |
//!
//! Installs that need no root happen right away and silently; the others wait
//! for the user, then ask for the password through polkit (`pkexec`), falling
//! back to `sudo` with a password typed into BGMail, or through the macOS
//! administrator prompt. Windows can't replace a running program, so its
//! installer runs once BGMail quits ([`on_quit`]). Copies built from source are
//! only told that a new version exists ([`Status::Available`]). The new version
//! runs after a restart.
//!
//! No GPUI and no network code: the app supplies the [`ReleaseSource`].

#![forbid(unsafe_code)]

mod install;
mod platform;
mod version;

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub use install::{asset_name, classify, detect, needs_root, Environment, Install, Os, Package};
pub use version::Version;

/// The GitHub repository releases come from.
pub const REPO: &str = "belizariogr/bgmail";
/// The page of the latest release (notes and manual downloads).
pub const RELEASES_URL: &str = "https://github.com/belizariogr/bgmail/releases/latest";

/// A release, as the [`ReleaseSource`] found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub assets: Vec<AssetInfo>,
}

/// One file of a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetInfo {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// `sha256:<hex>`.
    pub digest: Option<String>,
}

/// Where releases come from (GitHub in the app, a fake in tests). Blocking.
pub trait ReleaseSource {
    fn latest(&self) -> Result<ReleaseInfo, String>;
    /// Downloads `url` into `dest` (whole or not at all), reporting the bytes
    /// received so far.
    fn download(&self, url: &str, dest: &Path, progress: &mut dyn FnMut(u64))
        -> Result<(), String>;
}

/// Installs a downloaded update ([`System`] in the app, a fake in tests).
pub trait Installer {
    fn needs_root(&self, install: &Install) -> bool;
    fn install(&self, prepared: &Prepared, elevation: Elevation) -> Outcome;
}

/// The real installer; `updates_dir` holds downloads and the install script.
pub struct System {
    pub updates_dir: PathBuf,
}

impl Installer for System {
    fn needs_root(&self, install: &Install) -> bool {
        needs_root(install)
    }

    fn install(&self, prepared: &Prepared, elevation: Elevation) -> Outcome {
        platform::install(prepared, elevation, &self.updates_dir)
    }
}

/// A downloaded, verified update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub version: String,
    pub install: Install,
    pub file: PathBuf,
}

/// How the installer gets root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Elevation {
    None,
    /// `pkexec`: the desktop's polkit agent asks for the password.
    Polkit,
    /// `sudo`, with the password typed into BGMail.
    Sudo(String),
    /// The macOS administrator prompt, with this reason.
    MacAdmin(String),
}

/// How an install went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// The user dismissed the password prompt.
    Cancelled,
    /// polkit could not ask (no `pkexec`, or no agent running).
    NoAgent,
    WrongPassword,
    Failed(String),
}

/// Where the updater is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Checking,
    /// A manual check found nothing newer.
    UpToDate,
    /// A manual check could not reach the releases.
    CheckFailed(String),
    Downloading {
        version: String,
        received: u64,
        total: u64,
    },
    /// Downloaded; installing it needs root, so it waits for the user.
    ReadyToInstall(Prepared),
    /// polkit could not ask for the password, so BGMail does (`wrong`: the last
    /// one was refused).
    NeedsPassword {
        prepared: Prepared,
        wrong: bool,
    },
    Installing {
        version: String,
    },
    /// Installed: runs after a restart. On Windows, `apply_on_quit` is the
    /// installer that runs once BGMail quits.
    Updated {
        version: String,
        install: Install,
        apply_on_quit: Option<Prepared>,
    },
    /// A newer version this copy can't install (built from source).
    Available {
        version: String,
    },
    Errored(String),
}

impl Status {
    /// A check, download or install is running.
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Status::Checking | Status::Downloading { .. } | Status::Installing { .. }
        )
    }
}

/// What [`check`] needs to know about this copy.
pub struct Check<'a> {
    /// Asked for by the user: every result is reported, failures included.
    pub manual: bool,
    pub current_version: &'a str,
    pub install: Install,
    /// As in `std::env::consts::ARCH`.
    pub arch: &'a str,
    pub updates_dir: &'a Path,
}

/// Whether to check without being asked: not in debug builds (run from the
/// source tree) or with `BGMAIL_NO_UPDATE` set.
pub fn automatic_checks_allowed() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("BGMAIL_NO_UPDATE").is_none()
}

/// Looks for a newer release, downloads it and, when that needs no root,
/// installs it. Every step is `report`ed; the last report is the result.
/// Blocking: run it on a background thread.
pub fn check(
    source: &dyn ReleaseSource,
    installer: &dyn Installer,
    check: Check,
    report: &mut dyn FnMut(Status),
) {
    let manual = check.manual;
    // An automatic check stays quiet about failures: the next one retries.
    let quiet = |error: String| {
        if manual {
            Status::CheckFailed(error)
        } else {
            Status::Idle
        }
    };
    report(Status::Checking);
    let release = match source.latest() {
        Ok(release) => release,
        Err(error) => return report(quiet(error)),
    };
    let (Some(current), Some(latest)) = (
        Version::parse(check.current_version),
        Version::parse(&release.tag),
    ) else {
        return report(quiet(format!("unexpected version: {}", release.tag)));
    };
    if latest <= current {
        return report(if manual {
            Status::UpToDate
        } else {
            Status::Idle
        });
    }
    let version = release.tag.trim_start_matches(['v', 'V']).to_owned();
    let names = release.assets.iter().map(|asset| asset.name.as_str());
    let Some(asset) = asset_name(&check.install, names, check.arch)
        .and_then(|name| release.assets.iter().find(|asset| asset.name == name))
    else {
        return report(Status::Available { version });
    };

    let file = check.updates_dir.join(&asset.name);
    if let Err(error) = std::fs::create_dir_all(check.updates_dir) {
        return report(Status::Errored(error.to_string()));
    }
    // Earlier downloads go, except this very file, which may be complete.
    if let Ok(entries) = std::fs::read_dir(check.updates_dir) {
        for entry in entries.flatten().filter(|entry| entry.path() != file) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let complete = asset.digest.is_some()
        && file.is_file()
        && digest_matches(&file, asset.digest.as_deref()) == Ok(true);
    if !complete {
        let total = asset.size;
        report(Status::Downloading {
            version: version.clone(),
            received: 0,
            total,
        });
        // About a hundred progress reports, however big the file.
        let step = (total / 100).max(1);
        let mut reported = 0;
        let mut progress = |received: u64| {
            if received >= reported + step || received == total {
                reported = received;
                report(Status::Downloading {
                    version: version.clone(),
                    received,
                    total,
                });
            }
        };
        if let Err(error) = source.download(&asset.url, &file, &mut progress) {
            return report(if manual {
                Status::Errored(error)
            } else {
                Status::Idle
            });
        }
        match digest_matches(&file, asset.digest.as_deref()) {
            Ok(true) => {}
            Ok(false) => {
                let _ = std::fs::remove_file(&file);
                return report(Status::Errored(format!(
                    "{} is damaged (its SHA-256 does not match the release)",
                    asset.name
                )));
            }
            Err(error) => return report(Status::Errored(error)),
        }
    }

    let prepared = Prepared {
        version: version.clone(),
        install: check.install.clone(),
        file,
    };
    if matches!(
        check.install,
        Install::WindowsSetup(_) | Install::WindowsMsi(_)
    ) {
        return report(Status::Updated {
            version,
            install: check.install,
            apply_on_quit: Some(prepared),
        });
    }
    if installer.needs_root(&check.install) {
        return report(Status::ReadyToInstall(prepared));
    }
    report(Status::Installing { version });
    report(after_install(
        installer.install(&prepared, Elevation::None),
        prepared,
    ))
}

/// Installs an update that needs root, once the user agreed (`elevation`:
/// [`elevation_for`]). Blocking.
pub fn install_with_root(
    installer: &dyn Installer,
    prepared: Prepared,
    elevation: Elevation,
) -> Status {
    after_install(installer.install(&prepared, elevation), prepared)
}

/// The elevation to try: the macOS prompt, else polkit, or `sudo` once the user
/// typed a password into BGMail.
pub fn elevation_for(os: Os, password: Option<String>, mac_prompt: &str) -> Elevation {
    match (os, password) {
        (Os::MacOs, _) => Elevation::MacAdmin(mac_prompt.to_owned()),
        (_, Some(password)) => Elevation::Sudo(password),
        (_, None) => Elevation::Polkit,
    }
}

fn after_install(outcome: Outcome, prepared: Prepared) -> Status {
    match outcome {
        Outcome::Done => Status::Updated {
            version: prepared.version,
            install: prepared.install,
            apply_on_quit: None,
        },
        // Asked again only when the user wants.
        Outcome::Cancelled => Status::ReadyToInstall(prepared),
        Outcome::NoAgent => Status::NeedsPassword {
            prepared,
            wrong: false,
        },
        Outcome::WrongPassword => Status::NeedsPassword {
            prepared,
            wrong: true,
        },
        Outcome::Failed(error) => Status::Errored(error),
    }
}

/// Called as BGMail quits: on Windows runs a pending installer (and then the
/// new version when `relaunch`); elsewhere starts the installed version when
/// `relaunch`. The new process waits for this one to exit, since one BGMail
/// runs per profile.
pub fn on_quit(status: &Status, relaunch: bool) {
    let Status::Updated {
        install,
        apply_on_quit,
        ..
    } = status
    else {
        return;
    };
    match apply_on_quit {
        Some(prepared) => platform::apply_on_quit(prepared, relaunch),
        None if relaunch => platform::relaunch(install),
        None => {}
    }
}

/// Lowercase hex SHA-256 of `file`.
pub fn sha256(file: &Path) -> std::io::Result<String> {
    let mut reader = std::fs::File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Whether `file` matches the digest GitHub lists (true when it lists none).
pub fn digest_matches(file: &Path, digest: Option<&str>) -> Result<bool, String> {
    let Some(expected) = digest.and_then(|d| d.strip_prefix("sha256:")) else {
        return Ok(true);
    };
    let actual = sha256(file).map_err(|error| error.to_string())?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests;
