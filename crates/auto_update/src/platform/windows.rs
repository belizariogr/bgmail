//! Windows: a running program can't be replaced, so the downloaded installer
//! runs from a hidden PowerShell that first waits for BGMail to exit.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{launch_command, windows_helper_script};
use crate::{Elevation, Install, Outcome, Package, Prepared};

/// No console window for the helper.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Installers elevate themselves (UAC) when they need to.
pub(crate) fn is_root() -> bool {
    false
}

/// No Linux package managers here.
pub(crate) fn package_owner(_exe: &Path) -> Option<Package> {
    None
}

/// Windows updates are installed by [`apply_on_quit`] instead.
pub(crate) fn install(_prepared: &Prepared, _elevation: Elevation, _updates_dir: &Path) -> Outcome {
    Outcome::Failed("Windows updates are installed when BGMail quits".into())
}

fn powershell(script: &str) {
    let _ = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub(crate) fn apply_on_quit(prepared: &Prepared, relaunch: bool) {
    if let Some(script) = windows_helper_script(prepared, std::process::id(), relaunch) {
        powershell(&script);
    }
}

/// Starts BGMail again once this process has exited.
pub(crate) fn relaunch(install: &Install) {
    let Some((program, _)) = launch_command(install) else {
        return;
    };
    powershell(&format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath {}",
        std::process::id(),
        super::powershell_quote(&program.to_string_lossy())
    ));
}
