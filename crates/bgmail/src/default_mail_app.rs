//! Whether BGMail is the system's default mail app, and asking to make it so.
//! Each OS has its own mechanism behind this one API.

/// Whether BGMail handles `mailto:` links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    IsDefault,
    NotDefault,
    /// The system couldn't be asked.
    Unknown,
}

/// How far asking to become the default got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Only the user can finish it, in the system settings just opened.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    FinishInSystemSettings,
}

/// Why it couldn't be made the default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The system has no registration for this copy of the app (not installed:
    /// a development run or an AppImage).
    NotInstalled,
    Failed(String),
}

pub use platform::{set_default, status};

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::{Error, Outcome, Status};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const DESKTOP_ENTRY: &str = "BGMail.desktop";
    const MAILTO_MIME: &str = "x-scheme-handler/mailto";

    pub fn status() -> Status {
        status_with(&[])
    }

    pub fn set_default() -> Result<Outcome, Error> {
        set_default_with(&[], &application_dirs())
    }

    fn xdg_mime(envs: &[(&str, &Path)]) -> Command {
        let mut command = Command::new("xdg-mime");
        command.envs(envs.iter().map(|(key, value)| (*key, *value)));
        command
    }

    fn status_with(envs: &[(&str, &Path)]) -> Status {
        match xdg_mime(envs)
            .args(["query", "default", MAILTO_MIME])
            .output()
        {
            Ok(output) if output.status.success() => {
                status_from_query(&String::from_utf8_lossy(&output.stdout))
            }
            _ => Status::Unknown,
        }
    }

    pub(super) fn status_from_query(output: &str) -> Status {
        if output.trim() == DESKTOP_ENTRY {
            Status::IsDefault
        } else {
            Status::NotDefault
        }
    }

    fn set_default_with(envs: &[(&str, &Path)], dirs: &[PathBuf]) -> Result<Outcome, Error> {
        // `xdg-mime` would happily record an entry that doesn't exist, leaving
        // `mailto:` links with no handler at all.
        if !dirs.iter().any(|dir| dir.join(DESKTOP_ENTRY).is_file()) {
            return Err(Error::NotInstalled);
        }
        let output = xdg_mime(envs)
            .args(["default", DESKTOP_ENTRY, MAILTO_MIME])
            .output()
            .map_err(|error| Error::Failed(error.to_string()))?;
        if !output.status.success() {
            return Err(Error::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(Outcome::Done)
    }

    /// Where desktop entries are looked for (XDG Base Directory defaults).
    fn application_dirs() -> Vec<PathBuf> {
        let var = |name| std::env::var_os(name).filter(|value| !value.is_empty());
        let data_home = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".local/share")));
        let data_dirs =
            var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        data_home
            .into_iter()
            .chain(std::env::split_paths(&data_dirs))
            .map(|dir| dir.join("applications"))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn makes_it_the_default_only_when_installed() {
            if Command::new("xdg-mime").arg("--version").output().is_err() {
                return;
            }
            let home = tempfile::tempdir().unwrap();
            let config = home.path().join("config");
            let data = home.path().join("data");
            let applications = data.join("applications");
            std::fs::create_dir_all(&config).unwrap();
            std::fs::create_dir_all(&applications).unwrap();
            // Never the user's own mimeapps.list.
            let envs = [
                ("HOME", home.path()),
                ("XDG_CONFIG_HOME", config.as_path()),
                ("XDG_DATA_HOME", data.as_path()),
                ("XDG_DATA_DIRS", data.as_path()),
            ];
            let dirs = vec![applications.clone()];

            assert_eq!(status_with(&envs), Status::NotDefault);
            assert_eq!(set_default_with(&envs, &dirs), Err(Error::NotInstalled));
            assert!(!config.join("mimeapps.list").exists());

            std::fs::write(
                applications.join(DESKTOP_ENTRY),
                "[Desktop Entry]\nType=Application\nName=BGMail\nExec=bgmail %u\n\
                 MimeType=x-scheme-handler/mailto;\n",
            )
            .unwrap();
            assert_eq!(set_default_with(&envs, &dirs), Ok(Outcome::Done));
            assert_eq!(status_with(&envs), Status::IsDefault);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Error, Outcome, Status};
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    /// The packager's `identifier`; only a bundled app has it.
    const BUNDLE_ID: &str = "io.github.belizariogr.bgmail";

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
        fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, handler: CFStringRef) -> i32;
    }

    pub fn status() -> Status {
        let scheme = CFString::new("mailto");
        // SAFETY: `scheme` is a valid CFString for the call, and the result
        // follows the Copy rule: null, or owned by us (taken once below).
        let handler = unsafe { LSCopyDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef()) };
        if handler.is_null() {
            return Status::NotDefault;
        }
        // SAFETY: non-null and owned, per the above.
        let handler = unsafe { CFString::wrap_under_create_rule(handler) };
        if handler.to_string().eq_ignore_ascii_case(BUNDLE_ID) {
            Status::IsDefault
        } else {
            Status::NotDefault
        }
    }

    pub fn set_default() -> Result<Outcome, Error> {
        let scheme = CFString::new("mailto");
        let handler = CFString::new(BUNDLE_ID);
        // SAFETY: both are valid CFStrings for the duration of the call, which
        // doesn't keep them.
        let result = unsafe {
            LSSetDefaultHandlerForURLScheme(
                scheme.as_concrete_TypeRef(),
                handler.as_concrete_TypeRef(),
            )
        };
        match result {
            0 => Ok(Outcome::Done),
            code => Err(Error::Failed(format!("LaunchServices error {code}"))),
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Error, Outcome, Status};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    const PROG_ID: &str = "BGMail.Url.mailto";
    const CAPABILITIES: &str = r"Software\BGMail\Capabilities";
    /// No console window flashing for `reg.exe`.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg(args: &[&str]) -> std::io::Result<Output> {
        Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    pub fn status() -> Status {
        let key =
            r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\mailto\UserChoice";
        match reg(&["query", key, "/v", "ProgId"]) {
            Ok(output) if String::from_utf8_lossy(&output.stdout).contains(PROG_ID) => {
                Status::IsDefault
            }
            Ok(_) => Status::NotDefault,
            Err(_) => Status::Unknown,
        }
    }

    /// Windows lets only the user pick the default: this registers the app for
    /// this user (no elevation) and opens its page in the system settings.
    pub fn set_default() -> Result<Outcome, Error> {
        let exe = std::env::current_exe().map_err(|error| Error::Failed(error.to_string()))?;
        let command = format!("\"{}\" \"%1\"", exe.display());
        let class = format!(r"HKCU\Software\Classes\{PROG_ID}");
        let values: [(String, Option<&str>, &str); 6] = [
            (class.clone(), None, "URL:MailTo Protocol"),
            (class.clone(), Some("URL Protocol"), ""),
            (
                format!(r"{class}\shell\open\command"),
                None,
                command.as_str(),
            ),
            (
                format!(r"HKCU\{CAPABILITIES}"),
                Some("ApplicationName"),
                "BGMail",
            ),
            (
                format!(r"HKCU\{CAPABILITIES}\URLAssociations"),
                Some("mailto"),
                PROG_ID,
            ),
            (
                r"HKCU\Software\RegisteredApplications".to_string(),
                Some("BGMail"),
                CAPABILITIES,
            ),
        ];
        for (key, name, data) in &values {
            let mut args = vec!["add", key.as_str(), "/f", "/d", *data];
            match name {
                Some(name) => args.extend(["/v", *name]),
                None => args.push("/ve"),
            }
            let output = reg(&args).map_err(|error| Error::Failed(error.to_string()))?;
            if !output.status.success() {
                return Err(Error::Failed(
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
        }
        Command::new("explorer")
            .arg("ms-settings:defaultapps?registeredAppUser=BGMail")
            .spawn()
            .map_err(|error| Error::Failed(error.to_string()))?;
        Ok(Outcome::FinishInSystemSettings)
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn query_output_tells_who_is_default() {
        assert_eq!(
            platform::status_from_query("BGMail.desktop\n"),
            Status::IsDefault
        );
        assert_eq!(
            platform::status_from_query("HEY.desktop\n"),
            Status::NotDefault
        );
        assert_eq!(platform::status_from_query(""), Status::NotDefault);
    }
}
