use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::*;

const ABC_SHA256: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// Serves one release whose files all contain `body`.
struct FakeSource {
    tag: &'static str,
    assets: Vec<AssetInfo>,
    body: &'static str,
    fail_latest: bool,
    fail_download: bool,
    downloads: RefCell<u32>,
}

impl FakeSource {
    fn new(tag: &'static str, names: &[&str]) -> Self {
        Self {
            tag,
            assets: names
                .iter()
                .map(|name| AssetInfo {
                    name: (*name).into(),
                    url: format!("https://e/{name}"),
                    size: 3,
                    digest: Some(ABC_SHA256.into()),
                })
                .collect(),
            body: "abc",
            fail_latest: false,
            fail_download: false,
            downloads: RefCell::new(0),
        }
    }
}

impl ReleaseSource for FakeSource {
    fn latest(&self) -> Result<ReleaseInfo, String> {
        if self.fail_latest {
            return Err("offline".into());
        }
        Ok(ReleaseInfo {
            tag: self.tag.into(),
            assets: self.assets.clone(),
        })
    }

    fn download(
        &self,
        _url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), String> {
        *self.downloads.borrow_mut() += 1;
        if self.fail_download {
            return Err("connection reset".into());
        }
        std::fs::write(dest, self.body).map_err(|e| e.to_string())?;
        progress(self.body.len() as u64);
        Ok(())
    }
}

struct FakeInstaller {
    root: bool,
    outcome: Outcome,
    calls: RefCell<Vec<Elevation>>,
}

impl FakeInstaller {
    fn new(root: bool, outcome: Outcome) -> Self {
        Self {
            root,
            outcome,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl Installer for FakeInstaller {
    fn needs_root(&self, _install: &Install) -> bool {
        self.root
    }

    fn install(&self, _prepared: &Prepared, elevation: Elevation) -> Outcome {
        self.calls.borrow_mut().push(elevation);
        self.outcome.clone()
    }
}

fn updates_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bgmail-au-check-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Runs a check of a `0.2.0` build; every status it reported.
fn run(
    source: &FakeSource,
    installer: &FakeInstaller,
    install: Install,
    manual: bool,
    dir: &Path,
) -> Vec<Status> {
    let mut seen = Vec::new();
    check(
        source,
        installer,
        Check {
            manual,
            current_version: "0.2.0",
            install,
            arch: "x86_64",
            updates_dir: dir,
        },
        &mut |status| seen.push(status),
    );
    seen
}

const LINUX: &[&str] = &[
    "bgmail-linux-x86_64.tar.gz",
    "bgmail-linux-x86_64.deb",
    "bgmail-linux-x86_64.rpm",
    "bgmail-linux-x86_64.pkg.tar.zst",
    "BGMail_0.3.0_x64-setup.exe",
];

#[test]
fn nothing_newer_is_quiet_unless_asked() {
    let source = FakeSource::new("v0.2.0", LINUX);
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("same");
    let bundle = Install::Bundle("/h/bgmail.app".into());
    assert_eq!(
        run(&source, &installer, bundle.clone(), false, &dir),
        [Status::Checking, Status::Idle]
    );
    assert_eq!(
        run(&source, &installer, bundle, true, &dir).last(),
        Some(&Status::UpToDate)
    );
    assert_eq!(*source.downloads.borrow(), 0);
}

#[test]
fn an_unreachable_release_is_quiet_unless_asked() {
    let mut source = FakeSource::new("v0.3.0", LINUX);
    source.fail_latest = true;
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("offline");
    let bundle = Install::Bundle("/h/bgmail.app".into());
    assert_eq!(
        run(&source, &installer, bundle.clone(), false, &dir).last(),
        Some(&Status::Idle)
    );
    assert_eq!(
        run(&source, &installer, bundle, true, &dir).last(),
        Some(&Status::CheckFailed("offline".into()))
    );
}

#[test]
fn a_build_from_source_is_only_told() {
    let source = FakeSource::new("v0.3.0", LINUX);
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("source");
    let seen = run(&source, &installer, Install::Unsupported, false, &dir);
    assert_eq!(
        seen.last(),
        Some(&Status::Available {
            version: "0.3.0".into()
        })
    );
    assert_eq!(*source.downloads.borrow(), 0);
    assert!(installer.calls.borrow().is_empty());
}

#[test]
fn a_writable_install_is_updated_silently() {
    let source = FakeSource::new("v0.3.0", LINUX);
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("silent");
    let bundle = Install::Bundle("/h/bgmail.app".into());
    let seen = run(&source, &installer, bundle.clone(), false, &dir);
    assert!(matches!(
        seen[1],
        Status::Downloading {
            received: 0,
            total: 3,
            ..
        }
    ));
    assert!(seen.contains(&Status::Downloading {
        version: "0.3.0".into(),
        received: 3,
        total: 3
    }));
    assert_eq!(
        seen[seen.len() - 2],
        Status::Installing {
            version: "0.3.0".into()
        }
    );
    assert_eq!(
        seen.last(),
        Some(&Status::Updated {
            version: "0.3.0".into(),
            install: bundle,
            apply_on_quit: None,
        })
    );
    assert_eq!(*installer.calls.borrow(), [Elevation::None]);
    assert!(dir.join("bgmail-linux-x86_64.tar.gz").is_file());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn packages_wait_for_the_user_and_pick_their_format() {
    let installer = FakeInstaller::new(true, Outcome::Done);
    for (package, file) in [
        (Package::Pacman, "bgmail-linux-x86_64.pkg.tar.zst"),
        (Package::Rpm, "bgmail-linux-x86_64.rpm"),
        (Package::Deb, "bgmail-linux-x86_64.deb"),
    ] {
        let source = FakeSource::new("v0.3.0", LINUX);
        let dir = updates_dir(package.extension());
        let seen = run(&source, &installer, Install::Package(package), false, &dir);
        let Some(Status::ReadyToInstall(prepared)) = seen.last() else {
            panic!("{package:?}: {seen:?}");
        };
        assert_eq!(prepared.file, dir.join(file));
        let _ = std::fs::remove_dir_all(dir);
    }
    assert!(
        installer.calls.borrow().is_empty(),
        "installed without asking"
    );
}

#[test]
fn windows_installs_when_the_app_quits() {
    let source = FakeSource::new("v0.3.0", LINUX);
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("windows");
    let setup = Install::WindowsSetup(r"C:\u\BGMail.exe".into());
    let seen = run(&source, &installer, setup.clone(), false, &dir);
    let Some(Status::Updated {
        apply_on_quit: Some(prepared),
        install,
        ..
    }) = seen.last()
    else {
        panic!("{seen:?}");
    };
    assert_eq!(install, &setup);
    assert_eq!(prepared.file, dir.join("BGMail_0.3.0_x64-setup.exe"));
    assert!(installer.calls.borrow().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_download_is_deleted_and_reported() {
    let mut source = FakeSource::new("v0.3.0", LINUX);
    source.body = "tampered";
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("damaged");
    let seen = run(
        &source,
        &installer,
        Install::Bundle("/h/bgmail.app".into()),
        false,
        &dir,
    );
    assert!(
        matches!(seen.last(), Some(Status::Errored(e)) if e.contains("damaged")),
        "{seen:?}"
    );
    assert!(!dir.join("bgmail-linux-x86_64.tar.gz").exists());
    assert!(installer.calls.borrow().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_download_is_quiet_unless_asked() {
    let mut source = FakeSource::new("v0.3.0", LINUX);
    source.fail_download = true;
    let installer = FakeInstaller::new(false, Outcome::Done);
    let dir = updates_dir("dlfail");
    let bundle = Install::Bundle("/h/bgmail.app".into());
    assert_eq!(
        run(&source, &installer, bundle.clone(), false, &dir).last(),
        Some(&Status::Idle)
    );
    assert_eq!(
        run(&source, &installer, bundle, true, &dir).last(),
        Some(&Status::Errored("connection reset".into()))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_complete_download_is_reused_and_old_ones_dropped() {
    let source = FakeSource::new("v0.3.0", LINUX);
    let installer = FakeInstaller::new(true, Outcome::Done);
    let dir = updates_dir("reuse");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bgmail-linux-x86_64.deb"), "abc").unwrap();
    std::fs::write(dir.join("bgmail-linux-x86_64.deb.part"), "ab").unwrap();
    std::fs::write(dir.join("BGMail_0.2.9_x64-setup.exe"), "old").unwrap();
    let seen = run(
        &source,
        &installer,
        Install::Package(Package::Deb),
        false,
        &dir,
    );
    assert!(
        matches!(seen.last(), Some(Status::ReadyToInstall(_))),
        "{seen:?}"
    );
    assert_eq!(*source.downloads.borrow(), 0);
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, ["bgmail-linux-x86_64.deb"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn installing_with_root_follows_the_outcome() {
    let prepared = Prepared {
        version: "0.3.0".into(),
        install: Install::Package(Package::Rpm),
        file: "/u/f.rpm".into(),
    };
    let status = |outcome| {
        install_with_root(
            &FakeInstaller::new(true, outcome),
            prepared.clone(),
            Elevation::Polkit,
        )
    };
    assert_eq!(
        status(Outcome::Done),
        Status::Updated {
            version: "0.3.0".into(),
            install: Install::Package(Package::Rpm),
            apply_on_quit: None,
        }
    );
    assert_eq!(
        status(Outcome::Cancelled),
        Status::ReadyToInstall(prepared.clone())
    );
    assert_eq!(
        status(Outcome::NoAgent),
        Status::NeedsPassword {
            prepared: prepared.clone(),
            wrong: false
        }
    );
    assert_eq!(
        status(Outcome::WrongPassword),
        Status::NeedsPassword {
            prepared: prepared.clone(),
            wrong: true
        }
    );
    assert_eq!(
        status(Outcome::Failed("x".into())),
        Status::Errored("x".into())
    );
}

#[test]
fn elevation_depends_on_the_os_and_a_typed_password() {
    assert_eq!(
        elevation_for(Os::MacOs, None, "why"),
        Elevation::MacAdmin("why".into())
    );
    assert_eq!(elevation_for(Os::Linux, None, "why"), Elevation::Polkit);
    assert_eq!(
        elevation_for(Os::Linux, Some("pw".into()), "why"),
        Elevation::Sudo("pw".into())
    );
}

#[test]
fn busy_statuses() {
    assert!(Status::Checking.busy());
    assert!(Status::Installing {
        version: "1".into()
    }
    .busy());
    assert!(!Status::Idle.busy());
    assert!(!Status::Available {
        version: "1".into()
    }
    .busy());
}

#[test]
fn quitting_without_an_update_does_nothing() {
    // Nothing to run for these; must not spawn anything or panic.
    on_quit(&Status::Idle, true);
    on_quit(
        &Status::Updated {
            version: "1".into(),
            install: Install::Unsupported,
            apply_on_quit: None,
        },
        true,
    );
}

#[test]
fn digests_are_checked() {
    let dir = updates_dir("digest");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(digest_matches(&file, Some(ABC_SHA256)), Ok(true));
    assert_eq!(digest_matches(&file, Some("sha256:00")), Ok(false));
    assert_eq!(digest_matches(&file, None), Ok(true));
    assert!(digest_matches(&dir.join("missing"), Some(ABC_SHA256)).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
