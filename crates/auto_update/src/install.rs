//! How the running copy was installed, which decides the release file that
//! replaces it and whether replacing it needs root.

use std::path::{Path, PathBuf};

/// The operating system an install belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// A Linux package format, installed through the distro's package manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Package {
    Deb,
    Rpm,
    Pacman,
}

impl Package {
    /// Extension of the release file (`bgmail-linux-<arch>.<extension>`).
    pub fn extension(self) -> &'static str {
        match self {
            Package::Deb => "deb",
            Package::Rpm => "rpm",
            Package::Pacman => "pkg.tar.zst",
        }
    }

    /// The `install.sh` mode that installs it.
    #[cfg_attr(windows, allow(dead_code, reason = "Linux only"))]
    pub(crate) fn mode(self) -> &'static str {
        match self {
            Package::Deb => "deb",
            Package::Rpm => "rpm",
            Package::Pacman => "pacman",
        }
    }
}

/// How the running copy was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// A macOS app bundle, replaced with the one in the release's `.dmg`.
    MacApp(PathBuf),
    /// An AppImage file, replaced with the release's.
    AppImage(PathBuf),
    /// The `bgmail.app` folder `script/install.sh` unpacks into `~/.local`.
    Bundle(PathBuf),
    /// `<prefix>/bin/bgmail` and friends from `script/install-linux-system`.
    Prefix(PathBuf),
    /// `/usr/bin/bgmail` owned by a package (the release's `.deb`, `.rpm` or
    /// `.pkg.tar.zst`, or one built from the PKGBUILD).
    Package(Package),
    /// The per-user NSIS install (`…\AppData\Local\…\BGMail.exe`).
    WindowsSetup(PathBuf),
    /// The MSI install (`…\Program Files\BGMail\BGMail.exe`).
    WindowsMsi(PathBuf),
    /// Built from source (or a layout this crate doesn't know): it can only be
    /// told that a new version exists.
    Unsupported,
}

/// What [`classify`] looks at.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub exe: PathBuf,
    /// The AppImage file this executable runs from.
    pub appimage: Option<PathBuf>,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_app_data: Option<PathBuf>,
    /// `%ProgramFiles%` and `%ProgramW6432%` (Windows).
    pub program_files: Vec<PathBuf>,
}

impl Environment {
    /// The running executable and the variables that place it.
    pub fn current() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let exe = exe.canonicalize().unwrap_or(exe);
        // The AppImage runtime sets these for everything started from the
        // AppImage too, so they count only when this executable lives inside
        // the mounted image.
        let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|_| {
            appdir
                .as_ref()
                .is_some_and(|dir| !dir.as_os_str().is_empty() && exe.starts_with(dir))
        });
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Some(Self {
            exe,
            appimage,
            local_app_data: var("LOCALAPPDATA"),
            program_files: ["ProgramFiles", "ProgramW6432"]
                .into_iter()
                .filter_map(var)
                .collect(),
        })
    }
}

fn file_name(path: Option<&Path>) -> &str {
    path.and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("")
}

/// Works out the install from paths alone. A Linux system prefix is reported
/// as [`Install::Prefix`]; [`detect`] then asks the package managers.
pub fn classify(env: &Environment, os: Os) -> Install {
    match os {
        Os::MacOs => classify_mac(&env.exe),
        Os::Windows => classify_windows(env),
        Os::Linux => classify_linux(env),
    }
}

fn classify_mac(exe: &Path) -> Install {
    // …/BGMail.app/Contents/MacOS/BGMail
    let macos_dir = exe.parent();
    let contents = macos_dir.and_then(Path::parent);
    let Some(app) = contents.and_then(Path::parent) else {
        return Install::Unsupported;
    };
    // Run from the disk image, or moved there by Gatekeeper's translocation:
    // read-only, and not where the user keeps the app.
    let readonly = app.starts_with("/Volumes")
        || app
            .components()
            .any(|c| c.as_os_str() == "AppTranslocation");
    if file_name(macos_dir) == "MacOS"
        && file_name(contents) == "Contents"
        && file_name(Some(app)).ends_with(".app")
        && !readonly
    {
        Install::MacApp(app.to_path_buf())
    } else {
        Install::Unsupported
    }
}

fn classify_linux(env: &Environment) -> Install {
    if let Some(file) = &env.appimage {
        return Install::AppImage(file.clone());
    }
    let bin = env.exe.parent();
    let Some(root) = bin.and_then(Path::parent) else {
        return Install::Unsupported;
    };
    if file_name(bin) != "bin" || file_name(Some(&env.exe)) != "bgmail" {
        return Install::Unsupported;
    }
    if file_name(Some(root)) == "bgmail.app" {
        Install::Bundle(root.to_path_buf())
    } else if root == Path::new("/usr") || root == Path::new("/usr/local") {
        Install::Prefix(root.to_path_buf())
    } else {
        Install::Unsupported
    }
}

/// Windows paths compare without case, whichever separator they use; done on
/// strings so that it behaves the same when tested on any OS.
fn windows_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn classify_windows(env: &Environment) -> Install {
    let exe = windows_key(&env.exe);
    if exe.rsplit('\\').next() != Some("bgmail.exe") {
        return Install::Unsupported;
    }
    let inside = |dir: &Path| exe.starts_with(&format!("{}\\", windows_key(dir)));
    if env.local_app_data.as_deref().is_some_and(inside) {
        Install::WindowsSetup(env.exe.clone())
    } else if env.program_files.iter().any(|dir| inside(dir)) {
        Install::WindowsMsi(env.exe.clone())
    } else {
        Install::Unsupported
    }
}

/// The install of the running copy. Asks the package managers about system
/// paths (commands), so it belongs on a background thread.
pub fn detect() -> Install {
    let Some(env) = Environment::current() else {
        return Install::Unsupported;
    };
    match classify(&env, Os::current()) {
        Install::Prefix(prefix) => match crate::platform::package_owner(&env.exe) {
            Some(package) => Install::Package(package),
            None => Install::Prefix(prefix),
        },
        other => other,
    }
}

/// The name of the release file that updates `install` on this CPU (`arch` as
/// in `std::env::consts::ARCH`), among `names`.
pub fn asset_name<'a>(
    install: &Install,
    names: impl IntoIterator<Item = &'a str>,
    arch: &str,
) -> Option<&'a str> {
    let windows_arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let linux = |extension: &str| format!("bgmail-linux-{arch}.{extension}");
    let wanted = |name: &str| match install {
        Install::MacApp(_) => {
            name.starts_with("BGMail_") && name.ends_with(&format!("_{arch}.dmg"))
        }
        Install::AppImage(_) => {
            name.starts_with("BGMail_") && name.ends_with(&format!("_{arch}.AppImage"))
        }
        Install::Bundle(_) | Install::Prefix(_) => name == linux("tar.gz"),
        Install::Package(package) => name == linux(package.extension()),
        Install::WindowsSetup(_) => {
            name.starts_with("BGMail_") && name.ends_with(&format!("_{windows_arch}-setup.exe"))
        }
        Install::WindowsMsi(_) => {
            name.starts_with("BGMail_")
                && name.contains(&format!("_{windows_arch}_"))
                && name.ends_with(".msi")
        }
        Install::Unsupported => false,
    };
    names.into_iter().find(|name| wanted(name))
}

/// Whether installing over `install` needs root (Linux/macOS). Windows
/// installers decide for themselves: the MSI asks through UAC.
pub fn needs_root(install: &Install) -> bool {
    match install {
        Install::Prefix(_) | Install::Package(_) => !crate::platform::is_root(),
        // The folder is renamed and later deleted, so its contents must be ours too.
        Install::MacApp(path) | Install::Bundle(path) => {
            !(path.parent().is_some_and(writable) && writable(path))
        }
        Install::AppImage(path) => !path.parent().is_some_and(writable),
        Install::WindowsSetup(_) | Install::WindowsMsi(_) | Install::Unsupported => false,
    }
}

/// Whether this user can create files in `dir`.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".bgmail-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux(exe: &str, appimage: Option<&str>) -> Install {
        let env = Environment {
            exe: exe.into(),
            appimage: appimage.map(PathBuf::from),
            ..Default::default()
        };
        classify(&env, Os::Linux)
    }

    fn mac(exe: &str) -> Install {
        let env = Environment {
            exe: exe.into(),
            ..Default::default()
        };
        classify(&env, Os::MacOs)
    }

    fn windows(exe: &str) -> Install {
        let env = Environment {
            exe: exe.into(),
            local_app_data: Some(r"C:\Users\Ana Maria\AppData\Local".into()),
            program_files: vec![r"C:\Program Files".into()],
            ..Default::default()
        };
        classify(&env, Os::Windows)
    }

    #[test]
    fn linux_installs_are_recognized() {
        let p = PathBuf::from;
        assert_eq!(
            linux("/home/u/.local/bgmail.app/bin/bgmail", None),
            Install::Bundle(p("/home/u/.local/bgmail.app"))
        );
        assert_eq!(linux("/usr/bin/bgmail", None), Install::Prefix(p("/usr")));
        assert_eq!(
            linux("/usr/local/bin/bgmail", None),
            Install::Prefix(p("/usr/local"))
        );
        assert_eq!(
            linux("/tmp/.mount_x/usr/bin/BGMail", Some("/opt/BGMail.AppImage")),
            Install::AppImage(p("/opt/BGMail.AppImage"))
        );
        for source in [
            "/home/u/src/bgmail/target/release/BGMail",
            "/home/u/.cargo/bin/bgmail",
            "/opt/bin/bgmail",
            "/usr/bin/BGMail",
        ] {
            assert_eq!(linux(source, None), Install::Unsupported, "{source}");
        }
    }

    #[test]
    fn mac_installs_are_recognized() {
        assert_eq!(
            mac("/Applications/BGMail.app/Contents/MacOS/BGMail"),
            Install::MacApp("/Applications/BGMail.app".into())
        );
        for unsupported in [
            "/Volumes/BGMail/BGMail.app/Contents/MacOS/BGMail",
            "/private/var/folders/x/AppTranslocation/y/d/BGMail.app/Contents/MacOS/BGMail",
            "/Users/u/src/bgmail/target/release/BGMail",
        ] {
            assert_eq!(mac(unsupported), Install::Unsupported, "{unsupported}");
        }
    }

    #[test]
    fn windows_installs_are_recognized() {
        let setup = r"C:\Users\Ana Maria\AppData\Local\BGMail\BGMail.exe";
        assert_eq!(windows(setup), Install::WindowsSetup(setup.into()));
        // Case and separators don't matter on Windows.
        let odd = "c:/users/ana maria/appdata/local/Programs/BGMail/bgmail.EXE";
        assert_eq!(windows(odd), Install::WindowsSetup(odd.into()));
        let msi = r"C:\Program Files\BGMail\BGMail.exe";
        assert_eq!(windows(msi), Install::WindowsMsi(msi.into()));
        for unsupported in [
            r"C:\src\bgmail\target\release\BGMail.exe",
            r"C:\Program Files (x86)\BGMail\BGMail.exe",
            r"C:\Users\Ana Maria\AppData\Local\BGMail\other.exe",
        ] {
            assert_eq!(windows(unsupported), Install::Unsupported, "{unsupported}");
        }
    }

    #[test]
    fn assets_match_the_install() {
        let names = [
            "bgmail-linux-x86_64.tar.gz",
            "bgmail-linux-x86_64.deb",
            "bgmail-linux-x86_64.rpm",
            "bgmail-linux-x86_64.pkg.tar.zst",
            "BGMail_0.3.0_aarch64.dmg",
            "BGMail_0.3.0_x86_64.dmg",
            "BGMail_0.3.0_x86_64.AppImage",
            "BGMail_0.3.0_x64-setup.exe",
            "BGMail_0.3.0_x64_en-US.msi",
            "install.sh",
        ];
        let pick = |install: Install, arch: &str| asset_name(&install, names, arch);
        let any = PathBuf::from("/x");
        assert_eq!(
            pick(Install::Bundle(any.clone()), "x86_64"),
            Some("bgmail-linux-x86_64.tar.gz")
        );
        assert_eq!(
            pick(Install::Prefix(any.clone()), "x86_64"),
            Some("bgmail-linux-x86_64.tar.gz")
        );
        assert_eq!(pick(Install::Bundle(any.clone()), "aarch64"), None);
        assert_eq!(
            pick(Install::Package(Package::Deb), "x86_64"),
            Some("bgmail-linux-x86_64.deb")
        );
        assert_eq!(
            pick(Install::Package(Package::Rpm), "x86_64"),
            Some("bgmail-linux-x86_64.rpm")
        );
        assert_eq!(
            pick(Install::Package(Package::Pacman), "x86_64"),
            Some("bgmail-linux-x86_64.pkg.tar.zst")
        );
        assert_eq!(
            pick(Install::AppImage(any.clone()), "x86_64"),
            Some("BGMail_0.3.0_x86_64.AppImage")
        );
        assert_eq!(
            pick(Install::MacApp(any.clone()), "aarch64"),
            Some("BGMail_0.3.0_aarch64.dmg")
        );
        assert_eq!(
            pick(Install::MacApp(any.clone()), "x86_64"),
            Some("BGMail_0.3.0_x86_64.dmg")
        );
        assert_eq!(
            pick(Install::WindowsSetup(any.clone()), "x86_64"),
            Some("BGMail_0.3.0_x64-setup.exe")
        );
        assert_eq!(
            pick(Install::WindowsMsi(any.clone()), "x86_64"),
            Some("BGMail_0.3.0_x64_en-US.msi")
        );
        assert_eq!(pick(Install::WindowsSetup(any), "aarch64"), None);
        assert_eq!(pick(Install::Unsupported, "x86_64"), None);
    }

    #[test]
    fn writable_folders_need_no_root() {
        let dir = std::env::temp_dir().join(format!("bgmail-au-root-{}", std::process::id()));
        let app = dir.join("bgmail.app");
        std::fs::create_dir_all(&app).unwrap();
        assert!(!needs_root(&Install::Bundle(app.clone())));
        assert!(!needs_root(&Install::AppImage(dir.join("BGMail.AppImage"))));
        assert!(!needs_root(&Install::WindowsSetup(dir.join("BGMail.exe"))));
        assert!(!needs_root(&Install::Unsupported));
        let _ = std::fs::remove_dir_all(dir);
    }
}
