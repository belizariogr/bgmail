//! Linux and macOS: the embedded `install.sh`, run as this user or as root
//! (pkexec, sudo, or the macOS administrator prompt), and the relaunch helper.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use super::{
    applescript_string, launch_command, script_args, shell_quote, SCRIPT, STARTED, WAIT_THEN_EXEC,
};
use crate::{Elevation, Install, Outcome, Package, Prepared};

/// Whether `program` is on PATH.
fn has(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Runs `program` quietly; its stdout when it succeeded.
fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn is_root() -> bool {
    output("id", &["-u"]).is_some_and(|uid| uid.trim() == "0")
}

/// The package manager that owns `exe`, if any.
pub(crate) fn package_owner(exe: &Path) -> Option<Package> {
    let path = exe.to_str()?;
    let owns = |program: &str, args: &[&str]| has(program) && output(program, args).is_some();
    if owns("dpkg-query", &["-S", path]) {
        Some(Package::Deb)
    } else if owns("pacman", &["-Qqo", path]) {
        Some(Package::Pacman)
    } else if owns("rpm", &["-qf", path]) {
        Some(Package::Rpm)
    } else {
        None
    }
}

fn run_with_input(cmd: &mut Command, input: Option<&str>) -> std::io::Result<Output> {
    let mut child = cmd
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = std::io::Write::write_all(&mut stdin, format!("{input}\n").as_bytes());
    }
    child.wait_with_output()
}

fn failure(out: &Output) -> Outcome {
    let error = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Outcome::Failed(if error.is_empty() {
        format!("the installer exited with {}", out.status)
    } else {
        error
    })
}

pub(crate) fn install(prepared: &Prepared, elevation: Elevation, updates_dir: &Path) -> Outcome {
    let Some(args) = script_args(prepared) else {
        return Outcome::Failed("this copy cannot be updated".into());
    };
    let script = updates_dir.join("install-update.sh");
    if let Err(error) =
        std::fs::create_dir_all(updates_dir).and_then(|()| std::fs::write(&script, SCRIPT))
    {
        return Outcome::Failed(error.to_string());
    }
    let mut sh_args = vec![script.into_os_string()];
    sh_args.extend(args);
    let result = match elevation {
        Elevation::None => run_with_input(Command::new("/bin/sh").args(&sh_args), None),
        Elevation::Polkit => {
            if !has("pkexec") {
                return Outcome::NoAgent;
            }
            match run_with_input(Command::new("pkexec").arg("/bin/sh").args(&sh_args), None) {
                Ok(out) if !String::from_utf8_lossy(&out.stdout).contains(STARTED) => {
                    // pkexec exits with 126 when the user dismisses the prompt
                    // and 127 when it cannot authorize (no agent, for one).
                    return if out.status.code() == Some(126) {
                        Outcome::Cancelled
                    } else {
                        Outcome::NoAgent
                    };
                }
                other => other,
            }
        }
        Elevation::Sudo(password) => {
            if !has("sudo") {
                return Outcome::Failed("sudo is not installed".into());
            }
            // Check the password first, so a wrong one isn't reported as a failed install.
            let valid = run_with_input(
                Command::new("sudo").args(["-S", "-k", "-v", "-p", ""]),
                Some(&password),
            );
            if !valid.is_ok_and(|out| out.status.success()) {
                return Outcome::WrongPassword;
            }
            run_with_input(
                Command::new("sudo")
                    .args(["-S", "-k", "-p", "", "--", "/bin/sh"])
                    .args(&sh_args),
                Some(&password),
            )
        }
        Elevation::MacAdmin(prompt) => {
            let command = std::iter::once("/bin/sh".to_owned())
                .chain(
                    sh_args
                        .iter()
                        .map(|arg| shell_quote(&arg.to_string_lossy())),
                )
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "do shell script {} with prompt {} with administrator privileges",
                applescript_string(&command),
                applescript_string(&prompt)
            );
            match run_with_input(Command::new("osascript").args(["-e", &source]), None) {
                // "User canceled. (-128)"
                Ok(out)
                    if !out.status.success()
                        && String::from_utf8_lossy(&out.stderr).contains("-128") =>
                {
                    return Outcome::Cancelled;
                }
                other => other,
            }
        }
    };
    match result {
        Ok(out) if out.status.success() => {
            let _ = std::fs::remove_dir_all(updates_dir);
            Outcome::Done
        }
        Ok(out) => failure(&out),
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Starts the installed version once this process has exited.
pub(crate) fn relaunch(install: &Install) {
    let Some((program, args)) = launch_command(install) else {
        return;
    };
    let _ = Command::new("/bin/sh")
        .args(["-c", WAIT_THEN_EXEC])
        .arg(std::process::id().to_string())
        .arg(program)
        .args(args)
        // Not this AppImage's environment for the new copy.
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Outlives this process (and a terminal it was started from).
        .process_group(0)
        .spawn();
}

/// Windows installers only.
pub(crate) fn apply_on_quit(_prepared: &Prepared, _relaunch: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bgmail-au-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A release tarball (`bgmail.app/{bin/bgmail, share/…}`) whose binary prints `text`.
    fn tarball(dir: &Path, text: &str) -> std::path::PathBuf {
        let app = dir.join("src/bgmail.app");
        std::fs::create_dir_all(app.join("bin")).unwrap();
        std::fs::create_dir_all(app.join("share/applications")).unwrap();
        std::fs::create_dir_all(app.join("share/icons/hicolor/512x512/apps")).unwrap();
        std::fs::write(app.join("bin/bgmail"), format!("#!/bin/sh\necho {text}\n")).unwrap();
        let status = Command::new("chmod")
            .arg("755")
            .arg(app.join("bin/bgmail"))
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(
            app.join("share/applications/BGMail.desktop"),
            "[Desktop Entry]\nExec=bgmail %u\nIcon=bgmail\n",
        )
        .unwrap();
        std::fs::write(
            app.join("share/icons/hicolor/512x512/apps/bgmail.png"),
            "png",
        )
        .unwrap();
        let file = dir.join("bgmail-linux-x86_64.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&file)
            .arg("-C")
            .arg(dir.join("src"))
            .arg("bgmail.app")
            .status()
            .unwrap();
        assert!(status.success());
        file
    }

    fn run(path: &Path) -> String {
        String::from_utf8(Command::new(path).output().unwrap().stdout).unwrap()
    }

    #[test]
    fn a_bundle_is_replaced_in_place() {
        let dir = temp("bundle");
        let app = dir.join("home/.local/bgmail.app");
        std::fs::create_dir_all(app.join("bin")).unwrap();
        std::fs::write(app.join("bin/bgmail"), "old").unwrap();
        std::fs::write(app.join("leftover"), "").unwrap();
        let prepared = Prepared {
            version: "9.0.0".into(),
            install: Install::Bundle(app.clone()),
            file: tarball(&dir, "new-bgmail"),
        };
        let updates = dir.join("updates");
        assert_eq!(install(&prepared, Elevation::None, &updates), Outcome::Done);
        assert_eq!(run(&app.join("bin/bgmail")), "new-bgmail\n");
        assert!(!app.join("leftover").exists(), "old files kept");
        assert!(!updates.exists(), "updates folder kept");
        // Nothing staged is left beside the app.
        let siblings: Vec<_> = std::fs::read_dir(app.parent().unwrap())
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(siblings.len(), 1, "{siblings:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_prefix_gets_absolute_desktop_paths() {
        let dir = temp("prefix");
        let prefix = dir.join("usr");
        let prepared = Prepared {
            version: "9.0.0".into(),
            install: Install::Prefix(prefix.clone()),
            file: tarball(&dir, "new-bgmail"),
        };
        assert_eq!(
            install(&prepared, Elevation::None, &dir.join("updates")),
            Outcome::Done
        );
        assert_eq!(run(&prefix.join("bin/bgmail")), "new-bgmail\n");
        let desktop =
            std::fs::read_to_string(prefix.join("share/applications/BGMail.desktop")).unwrap();
        assert!(
            desktop.contains(&format!("Exec={}/bin/bgmail %u", prefix.display())),
            "{desktop}"
        );
        assert!(prefix
            .join("share/icons/hicolor/512x512/apps/bgmail.png")
            .is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_tarball_leaves_the_install_alone() {
        let dir = temp("bad");
        let app = dir.join("bgmail.app");
        std::fs::create_dir_all(app.join("bin")).unwrap();
        std::fs::write(app.join("bin/bgmail"), "old").unwrap();
        std::fs::write(dir.join("bad.tar.gz"), "not a tarball").unwrap();
        let prepared = Prepared {
            version: "9.0.0".into(),
            install: Install::Bundle(app.clone()),
            file: dir.join("bad.tar.gz"),
        };
        assert!(matches!(
            install(&prepared, Elevation::None, &dir.join("updates")),
            Outcome::Failed(_)
        ));
        assert_eq!(
            std::fs::read_to_string(app.join("bin/bgmail")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The package modes call the distro's package manager (faked on PATH).
    #[test]
    fn packages_go_through_the_package_manager() {
        let dir = temp("packages");
        let script = dir.join("install.sh");
        std::fs::write(&script, SCRIPT).unwrap();
        let log = dir.join("log");
        let fake = |name: &str| {
            let bin = dir.join(format!("bin-{name}"));
            std::fs::create_dir_all(&bin).unwrap();
            let path = bin.join(name);
            std::fs::write(
                &path,
                format!("#!/bin/sh\necho \"{name} $*\" >> {}\n", log.display()),
            )
            .unwrap();
            Command::new("chmod")
                .arg("755")
                .arg(&path)
                .status()
                .unwrap();
            bin
        };
        let cases = [
            ("pacman", "pacman", "pacman -U --noconfirm /p/f"),
            ("rpm", "dnf", "dnf install -y /p/f"),
            (
                "rpm",
                "zypper",
                "zypper --non-interactive install --allow-unsigned-rpm /p/f",
            ),
            ("rpm", "yum", "yum install -y /p/f"),
            ("deb", "apt-get", "apt-get install -y /p/f"),
        ];
        for (mode, manager, expected) in cases {
            let _ = std::fs::remove_file(&log);
            let bin = fake(manager);
            // Only the fake and the basics (sh, echo) on PATH.
            let out = Command::new("/bin/sh")
                .arg(&script)
                .args([mode, "/p/f"])
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{mode}/{manager}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let ran = std::fs::read_to_string(&log).unwrap_or_default();
            assert_eq!(ran.trim(), expected, "{mode}/{manager}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_relaunch_waits_for_the_old_process() {
        let dir = temp("wait");
        let mut sleeper = Command::new("sleep").arg("1").spawn().unwrap();
        let marker = dir.join("started");
        let started = std::time::Instant::now();
        let out = Command::new("/bin/sh")
            .args(["-c", WAIT_THEN_EXEC])
            .arg(sleeper.id().to_string())
            .args(["touch"])
            .arg(&marker)
            .spawn()
            .unwrap();
        // Reap the sleeper as soon as it exits, as the system does for a quitting app.
        sleeper.wait().unwrap();
        let mut out = out;
        out.wait().unwrap();
        assert!(marker.exists());
        assert!(started.elapsed() >= std::time::Duration::from_millis(900));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn processes_are_queried_quietly() {
        assert!(has("sh"));
        assert!(!has("bgmail-no-such-program"));
        assert_eq!(output("sh", &["-c", "echo hi"]).as_deref(), Some("hi\n"));
        assert_eq!(output("sh", &["-c", "exit 3"]), None);
        assert_eq!(package_owner(Path::new("/nonexistent/bgmail")), None);
    }
}
