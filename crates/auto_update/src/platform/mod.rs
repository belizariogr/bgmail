//! The OS side of updating, behind one API: who owns `/usr/bin/bgmail`,
//! whether we are root, running the installer, and starting the new version.
//! The commands are built by plain functions here (tested on every OS); the
//! `unix` / `windows` modules only run them.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{Install, Prepared};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{apply_on_quit, install, is_root, package_owner, relaunch};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{apply_on_quit, install, is_root, package_owner, relaunch};

/// The installer steps for Linux and macOS, run with `/bin/sh` (as root when
/// the install needs it).
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) const SCRIPT: &str = include_str!("install.sh");
/// Printed by the script once it runs: authorization succeeded.
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) const STARTED: &str = "bgmail-update: start";

/// `install.sh` arguments for `prepared` (None: nothing it installs).
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) fn script_args(prepared: &Prepared) -> Option<Vec<OsString>> {
    let package = prepared.file.clone().into_os_string();
    Some(match &prepared.install {
        Install::MacApp(app) => vec!["macapp".into(), package, app.into()],
        Install::AppImage(file) => vec!["appimage".into(), package, file.into()],
        Install::Bundle(app) => vec!["bundle".into(), package, app.into()],
        Install::Prefix(prefix) => vec!["prefix".into(), package, prefix.into()],
        Install::Package(kind) => vec![kind.mode().into(), package],
        Install::WindowsSetup(_) | Install::WindowsMsi(_) | Install::Unsupported => return None,
    })
}

/// The program (and arguments) that starts the installed version.
pub(crate) fn launch_command(install: &Install) -> Option<(PathBuf, Vec<OsString>)> {
    Some(match install {
        Install::MacApp(app) => ("/usr/bin/open".into(), vec!["-n".into(), app.into()]),
        Install::AppImage(file) => (file.clone(), Vec::new()),
        Install::Bundle(app) => (app.join("bin/bgmail"), Vec::new()),
        Install::Prefix(prefix) => (prefix.join("bin/bgmail"), Vec::new()),
        // Package managers put it there; the running binary's own path reads
        // "(deleted)" once the package replaced it.
        Install::Package(_) => ("/usr/bin/bgmail".into(), Vec::new()),
        Install::WindowsSetup(exe) | Install::WindowsMsi(exe) => (exe.clone(), Vec::new()),
        Install::Unsupported => return None,
    })
}

/// `sh -c` script that waits for process `$0` to exit, then runs `"$@"`.
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) const WAIT_THEN_EXEC: &str =
    "while kill -0 \"$0\" 2>/dev/null; do sleep 0.2; done; exec \"$@\"";

/// `s` quoted for a POSIX shell.
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` as an AppleScript string literal.
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Linux/macOS only; tested everywhere")
)]
pub(crate) fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\""))
}

/// `s` as a PowerShell single-quoted string.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "Windows only; tested everywhere")
)]
pub(crate) fn powershell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// PowerShell that waits for process `pid` to exit, runs the downloaded
/// Windows installer silently, deletes it and, when `relaunch`, starts BGMail.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "Windows only; tested everywhere")
)]
pub(crate) fn windows_helper_script(
    prepared: &Prepared,
    pid: u32,
    relaunch: bool,
) -> Option<String> {
    let file = prepared.file.to_string_lossy();
    let (install, exe) = match &prepared.install {
        // NSIS: /S is silent; it keeps the folder of the earlier install.
        Install::WindowsSetup(exe) => (
            format!(
                "Start-Process -FilePath {} -ArgumentList '/S' -Wait",
                powershell_quote(&file)
            ),
            exe,
        ),
        // A per-machine MSI: msiexec asks for consent (UAC) and shows its progress.
        Install::WindowsMsi(exe) => (
            format!(
                "Start-Process -FilePath 'msiexec.exe' -ArgumentList {} -Wait",
                powershell_quote(&format!("/i \"{file}\" /passive /norestart"))
            ),
            exe,
        ),
        _ => return None,
    };
    let mut script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; Wait-Process -Id {pid}; {install}; Remove-Item -LiteralPath {}",
        powershell_quote(&file)
    );
    if relaunch {
        script.push_str(&format!(
            "; Start-Process -FilePath {}",
            powershell_quote(&exe.to_string_lossy())
        ));
    }
    Some(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package;

    fn prepared(install: Install) -> Prepared {
        Prepared {
            version: "1.0.0".into(),
            install,
            file: "/u/updates/f".into(),
        }
    }

    #[test]
    fn script_modes_follow_the_install() {
        let args = |install| {
            script_args(&prepared(install)).map(|args| {
                args.iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            args(Install::Bundle("/h/.local/bgmail.app".into())).unwrap(),
            ["bundle", "/u/updates/f", "/h/.local/bgmail.app"]
        );
        assert_eq!(
            args(Install::Prefix("/usr/local".into())).unwrap(),
            ["prefix", "/u/updates/f", "/usr/local"]
        );
        assert_eq!(
            args(Install::Package(Package::Pacman)).unwrap(),
            ["pacman", "/u/updates/f"]
        );
        assert_eq!(
            args(Install::Package(Package::Rpm)).unwrap(),
            ["rpm", "/u/updates/f"]
        );
        assert_eq!(
            args(Install::Package(Package::Deb)).unwrap(),
            ["deb", "/u/updates/f"]
        );
        assert_eq!(
            args(Install::MacApp("/Applications/BGMail.app".into())).unwrap()[0],
            "macapp"
        );
        assert_eq!(args(Install::AppImage("/a".into())).unwrap()[0], "appimage");
        assert_eq!(args(Install::WindowsSetup("C:\\x".into())), None);
        assert_eq!(args(Install::Unsupported), None);
    }

    #[test]
    fn the_new_version_starts_from_the_installed_path() {
        let launch = |install| launch_command(&install).map(|(program, _)| program);
        assert_eq!(
            launch(Install::Bundle("/h/.local/bgmail.app".into())),
            Some("/h/.local/bgmail.app/bin/bgmail".into())
        );
        assert_eq!(
            launch(Install::Package(Package::Deb)),
            Some("/usr/bin/bgmail".into())
        );
        assert_eq!(
            launch(Install::Prefix("/usr/local".into())),
            Some("/usr/local/bin/bgmail".into())
        );
        let (program, args) =
            launch_command(&Install::MacApp("/Applications/BGMail.app".into())).unwrap();
        assert_eq!(program, PathBuf::from("/usr/bin/open"));
        assert_eq!(args, ["-n", "/Applications/BGMail.app"]);
        assert_eq!(launch(Install::Unsupported), None);
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(
            applescript_string(r#"say "hi" \o/"#),
            r#""say \"hi\" \\o/""#
        );
        assert_eq!(powershell_quote(r"C:\Ana's\x"), r"'C:\Ana''s\x'");
    }

    #[test]
    fn windows_installers_run_after_the_app_exits() {
        let setup = Prepared {
            version: "1.0.0".into(),
            install: Install::WindowsSetup(r"C:\Users\A B\AppData\Local\BGMail\BGMail.exe".into()),
            file: r"C:\Users\A B\.config\BGMail\updates\BGMail_1.0.0_x64-setup.exe".into(),
        };
        let script = windows_helper_script(&setup, 42, true).unwrap();
        assert!(script.contains("Wait-Process -Id 42;"), "{script}");
        assert!(script.contains(
            r"Start-Process -FilePath 'C:\Users\A B\.config\BGMail\updates\BGMail_1.0.0_x64-setup.exe' -ArgumentList '/S' -Wait"
        ), "{script}");
        assert!(
            script.ends_with(
                r"Start-Process -FilePath 'C:\Users\A B\AppData\Local\BGMail\BGMail.exe'"
            ),
            "{script}"
        );
        // Wait, then install, then start: in that order.
        let wait = script.find("Wait-Process").unwrap();
        let install = script.find("-ArgumentList").unwrap();
        let start = script.rfind("Start-Process").unwrap();
        assert!(wait < install && install < start);

        let msi = Prepared {
            install: Install::WindowsMsi(r"C:\Program Files\BGMail\BGMail.exe".into()),
            file: r"C:\u\BGMail_1.0.0_x64_en-US.msi".into(),
            ..setup
        };
        let script = windows_helper_script(&msi, 7, false).unwrap();
        assert!(script.contains(
            r#"'msiexec.exe' -ArgumentList '/i "C:\u\BGMail_1.0.0_x64_en-US.msi" /passive /norestart'"#
        ), "{script}");
        assert!(!script.contains("BGMail.exe'"), "no relaunch: {script}");
        assert_eq!(
            windows_helper_script(&prepared(Install::Package(Package::Deb)), 1, true),
            None
        );
    }
}
