//! Updates in the app (the work is the `auto_update` crate's): one
//! [`AutoUpdater`] entity, held by a global, checks 10 s after startup and
//! every 12 hours (Thunderbird's `app.update.interval`), and by hand from
//! Settings ▸ General or BGMail ▸ Check for Updates… on macOS. Its status shows
//! in the status bar ([`indicator`]) when it needs the user, and in full in
//! Settings ([`status_text`]).

use std::path::PathBuf;
use std::time::Duration;

use auto_update::{AssetInfo, Check, Os, ReleaseInfo, ReleaseSource, Status, System};
use gpui::{App, AppContext, Context, Entity, Global};

use crate::locale::{ActiveLanguage, Key, Language};

/// After startup, so the check never competes with opening the window.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
/// Thunderbird's and Firefox's default `app.update.interval` (43200 s).
const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// The update state of the app.
pub(crate) struct AutoUpdater {
    status: Status,
    /// Settings ▸ General ▸ "Check for updates automatically".
    enabled: bool,
    /// Start the new version once BGMail has quit.
    relaunch: bool,
}

struct GlobalAutoUpdater(Entity<AutoUpdater>);

impl Global for GlobalAutoUpdater {}

/// GitHub Releases, through `protocols::release`.
struct GitHub;

impl ReleaseSource for GitHub {
    fn latest(&self) -> Result<ReleaseInfo, String> {
        // A staging or test release instead of GitHub's (same JSON).
        let release = match std::env::var("BGMAIL_UPDATE_API") {
            Ok(url) => protocols::release::latest_release_at(&url),
            Err(_) => protocols::release::latest_release(auto_update::REPO),
        }
        .map_err(|e| e.message)?;
        Ok(ReleaseInfo {
            tag: release.tag_name,
            assets: release
                .assets
                .into_iter()
                .map(|asset| AssetInfo {
                    name: asset.name,
                    url: asset.browser_download_url,
                    size: asset.size,
                    digest: asset.digest,
                })
                .collect(),
        })
    }

    fn download(
        &self,
        url: &str,
        dest: &std::path::Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), String> {
        protocols::release::download(url, dest, progress).map_err(|e| e.message)
    }
}

/// Downloads and the install script: `~/.config/BGMail/updates`.
fn updates_dir() -> PathBuf {
    crate::config::config_path()
        .map(|config| config.with_file_name("updates"))
        .unwrap_or_else(|| std::env::temp_dir().join("bgmail-updates"))
}

/// Creates the updater and schedules its checks (`enabled`: the saved setting).
pub(crate) fn init(enabled: bool, cx: &mut App) {
    let updater = cx.new(|cx| {
        // Held by a global, so it outlives the windows and sees the quit.
        cx.on_app_quit(|this: &mut AutoUpdater, _| {
            auto_update::on_quit(&this.status, this.relaunch);
            async {}
        })
        .detach();
        AutoUpdater {
            status: Status::Idle,
            enabled,
            relaunch: false,
        }
    });
    cx.set_global(GlobalAutoUpdater(updater.clone()));
    if !auto_update::automatic_checks_allowed() {
        return;
    }
    let updater = updater.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_CHECK_DELAY).await;
        loop {
            let alive = updater.update(cx, |this, cx| {
                if this.enabled {
                    this.check(false, cx);
                }
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor().timer(CHECK_INTERVAL).await;
        }
    })
    .detach();
}

/// Turns the automatic checks on or off (they resume at the next interval).
pub(crate) fn set_enabled(enabled: bool, cx: &mut App) {
    if let Some(updater) = AutoUpdater::get(cx) {
        updater.update(cx, |this, _| this.enabled = enabled);
    }
}

/// A check asked for by the user.
pub(crate) fn check_now(cx: &mut App) {
    if let Some(updater) = AutoUpdater::get(cx) {
        updater.update(cx, |this, cx| this.check(true, cx));
    }
}

/// Quits (unless a draft keeps BGMail open) and starts the installed version.
pub(crate) fn restart(cx: &mut App) {
    crate::schedule_quit_then(cx, |cx| {
        if let Some(updater) = AutoUpdater::get(cx) {
            updater.update(cx, |this, _| this.relaunch = true);
        }
    });
}

/// Whether a check may start now. One already found waits for the user (or a
/// restart) instead of being looked for again.
pub(crate) fn should_check(status: &Status) -> bool {
    !status.busy()
        && !matches!(
            status,
            Status::ReadyToInstall(_) | Status::NeedsPassword { .. } | Status::Updated { .. }
        )
}

impl AutoUpdater {
    pub(crate) fn get(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalAutoUpdater>()
            .map(|global| global.0.clone())
    }

    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    /// Looks for a newer release in the background; downloads and, when no
    /// password is needed, installs it. `manual`: report every result.
    pub(crate) fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !should_check(&self.status) {
            return;
        }
        self.status = Status::Checking;
        cx.notify();
        let (tx, rx) = async_channel::unbounded();
        cx.background_spawn(async move {
            let dir = updates_dir();
            let installer = System {
                updates_dir: dir.clone(),
            };
            let check = Check {
                manual,
                current_version: env!("CARGO_PKG_VERSION"),
                install: auto_update::detect(),
                arch: std::env::consts::ARCH,
                updates_dir: &dir,
            };
            auto_update::check(&GitHub, &installer, check, &mut |status| {
                let _ = tx.send_blocking(status);
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(status) = rx.recv().await {
                let applied = this.update(cx, |this, cx| {
                    this.status = status;
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Installs a downloaded update that needs root. `password`: typed into
    /// BGMail after polkit could not ask (used only then).
    pub(crate) fn install(&mut self, password: Option<String>, cx: &mut Context<Self>) {
        let (prepared, password) = match &self.status {
            Status::ReadyToInstall(prepared) => (prepared.clone(), None),
            Status::NeedsPassword { prepared, .. } => (prepared.clone(), password),
            _ => return,
        };
        let elevation = auto_update::elevation_for(
            Os::current(),
            password,
            Key::UpdatesAdminPrompt.tr(cx.language()),
        );
        self.status = Status::Installing {
            version: prepared.version.clone(),
        };
        cx.notify();
        let installed = cx.background_spawn(async move {
            let installer = System {
                updates_dir: updates_dir(),
            };
            auto_update::install_with_root(&installer, prepared, elevation)
        });
        cx.spawn(async move |this, cx| {
            let status = installed.await;
            this.update(cx, |this, cx| {
                this.status = status;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// What the status bar offers when the updater needs the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Indicator {
    /// Installed: restart to run it.
    Restart,
    /// Downloaded; installing it needs the password.
    Install,
    /// A new version this copy can't install.
    Available,
    Failed,
}

impl Indicator {
    pub(crate) fn label(self) -> Key {
        match self {
            Indicator::Restart => Key::StatusUpdateRestart,
            Indicator::Install => Key::StatusUpdateInstall,
            Indicator::Available => Key::StatusUpdateAvailable,
            Indicator::Failed => Key::StatusUpdateFailed,
        }
    }
}

pub(crate) fn indicator(status: &Status) -> Option<Indicator> {
    match status {
        Status::Updated { .. } => Some(Indicator::Restart),
        Status::ReadyToInstall(_) | Status::NeedsPassword { .. } => Some(Indicator::Install),
        Status::Available { .. } => Some(Indicator::Available),
        Status::Errored(_) => Some(Indicator::Failed),
        _ => None,
    }
}

/// The updater's status for Settings, and the error detail below it.
pub(crate) fn status_text(status: &Status, language: Language) -> Option<(String, Option<String>)> {
    let with = |key: Key, version: &str| key.tr(language).replace("{version}", version);
    Some(match status {
        Status::Idle => return None,
        Status::Checking => (Key::UpdatesChecking.tr(language).into(), None),
        Status::UpToDate => (Key::UpdatesUpToDate.tr(language).into(), None),
        Status::CheckFailed(error) => (
            Key::UpdatesCheckFailed.tr(language).into(),
            Some(error.clone()),
        ),
        Status::Downloading {
            version,
            received,
            total,
        } => {
            let text = with(Key::UpdatesDownloading, version);
            if *total > 0 {
                let percent = (received.saturating_mul(100) / total).min(100);
                (format!("{text} {percent}%"), None)
            } else {
                (text, None)
            }
        }
        Status::ReadyToInstall(prepared) => (with(Key::UpdatesReady, &prepared.version), None),
        Status::NeedsPassword { prepared, .. } => {
            (with(Key::UpdatesPassword, &prepared.version), None)
        }
        Status::Installing { version } => (with(Key::UpdatesInstalling, version), None),
        Status::Updated {
            version,
            apply_on_quit,
            ..
        } => {
            let key = if apply_on_quit.is_some() {
                Key::UpdatesInstallOnRestart
            } else {
                Key::UpdatesInstalled
            };
            (with(key, version), None)
        }
        Status::Available { version } => (with(Key::UpdatesAvailable, version), None),
        Status::Errored(error) => (Key::UpdatesFailed.tr(language).into(), Some(error.clone())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use auto_update::{Install, Package, Prepared};

    fn prepared() -> Prepared {
        Prepared {
            version: "0.3.0".into(),
            install: Install::Package(Package::Pacman),
            file: "/u/f".into(),
        }
    }

    fn updated(apply_on_quit: Option<Prepared>) -> Status {
        Status::Updated {
            version: "0.3.0".into(),
            install: Install::Package(Package::Pacman),
            apply_on_quit,
        }
    }

    #[test]
    fn found_updates_are_not_looked_for_again() {
        assert!(should_check(&Status::Idle));
        assert!(should_check(&Status::UpToDate));
        assert!(should_check(&Status::Errored("x".into())));
        assert!(should_check(&Status::Available {
            version: "1".into()
        }));
        assert!(!should_check(&Status::Checking));
        assert!(!should_check(&Status::ReadyToInstall(prepared())));
        assert!(!should_check(&updated(None)));
    }

    #[test]
    fn the_status_bar_shows_only_what_needs_the_user() {
        assert_eq!(indicator(&updated(None)), Some(Indicator::Restart));
        assert_eq!(
            indicator(&Status::NeedsPassword {
                prepared: prepared(),
                wrong: true
            }),
            Some(Indicator::Install)
        );
        assert_eq!(
            indicator(&Status::Available {
                version: "1".into()
            }),
            Some(Indicator::Available)
        );
        assert_eq!(
            indicator(&Status::Errored("x".into())),
            Some(Indicator::Failed)
        );
        for quiet in [
            Status::Idle,
            Status::Checking,
            Status::UpToDate,
            Status::CheckFailed("x".into()),
            Status::Downloading {
                version: "1".into(),
                received: 1,
                total: 2,
            },
        ] {
            assert_eq!(indicator(&quiet), None, "{quiet:?}");
        }
        for indicator in [
            Indicator::Restart,
            Indicator::Install,
            Indicator::Available,
            Indicator::Failed,
        ] {
            assert!(!indicator.label().tr(Language::English).is_empty());
        }
    }

    #[test]
    fn settings_describe_every_status() {
        let en = Language::English;
        assert_eq!(status_text(&Status::Idle, en), None);
        let (text, _) = status_text(
            &Status::Downloading {
                version: "0.3.0".into(),
                received: 50,
                total: 200,
            },
            en,
        )
        .unwrap();
        assert_eq!(text, "Downloading BGMail 0.3.0… 25%");
        let (text, detail) = status_text(&Status::Errored("disk full".into()), en).unwrap();
        assert_eq!(text, "The update could not be installed.");
        assert_eq!(detail.as_deref(), Some("disk full"));
        let (restart, _) = status_text(&updated(None), Language::Portuguese).unwrap();
        assert!(
            restart.contains("0.3.0") && restart.contains("Reinicie"),
            "{restart}"
        );
        let (windows, _) = status_text(&updated(Some(prepared())), en).unwrap();
        assert!(windows.contains("when BGMail restarts"), "{windows}");
        let (ready, _) = status_text(&Status::ReadyToInstall(prepared()), en).unwrap();
        assert!(ready.contains("0.3.0") && !ready.contains("{version}"));
    }
}
