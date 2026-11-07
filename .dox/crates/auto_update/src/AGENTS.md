# crates/auto_update/src/ — Source API

## Purpose

Source of the `auto_update` crate: versions, install detection, the check
pipeline and the platform installers.

## Ownership

- Owns: `lib.rs`, `version.rs`, `install.rs`, `platform/{mod,unix,windows}.rs`,
  `platform/install.sh`, `tests.rs`.
- Does not own: crate contracts (parent `.dox/crates/auto_update/AGENTS.md`).

## Local Contracts

- Every function/method is cataloged below; update entries with signature or
  behavior changes.

## Verification

- `cargo test -p auto_update`

## Child DOX Index

_(none)_

## API Catalog

### `src/lib.rs`

#### Types / constants

- **`REPO`** = `belizariogr/bgmail`; **`RELEASES_URL`** — latest release page.
- **struct `ReleaseInfo { tag, assets: Vec<AssetInfo> }`**, **struct `AssetInfo { name, url, size, digest: Option<"sha256:…"> }`**.
- **trait `ReleaseSource`** — `latest() -> Result<ReleaseInfo, String>`; `download(url, dest, progress: &mut dyn FnMut(u64)) -> Result<(), String>` (whole file or nothing). Blocking.
- **trait `Installer`** — `needs_root(&Install) -> bool`; `install(&Prepared, Elevation) -> Outcome`.
- **struct `System { updates_dir }`** — the real `Installer`: `needs_root` → `install::needs_root`; `install` → `platform::install`.
- **struct `Prepared { version, install, file }`** — a downloaded, verified update.
- **enum `Elevation`** — `None`, `Polkit`, `Sudo(password)`, `MacAdmin(prompt)`.
- **enum `Outcome`** — `Done`, `Cancelled`, `NoAgent`, `WrongPassword`, `Failed(String)`.
- **enum `Status`** — `Idle`, `Checking`, `UpToDate`, `CheckFailed(e)`, `Downloading { version, received, total }`, `ReadyToInstall(Prepared)`, `NeedsPassword { prepared, wrong }`, `Installing { version }`, `Updated { version, install, apply_on_quit: Option<Prepared> }`, `Available { version }`, `Errored(e)`.
- **struct `Check<'a> { manual, current_version, install, arch, updates_dir }`**.

#### Functions

- **`Status::busy`** — true for `Checking`, `Downloading`, `Installing`.
- **`automatic_checks_allowed`** — false in debug builds or with `BGMAIL_NO_UPDATE`.
- **`check(source, installer, check, report)`** — reports `Checking`, then: latest not newer (or unparsable tag, or unreachable) → `UpToDate`/`CheckFailed` when manual, else `Idle`; no matching asset → `Available`; creates `updates_dir`, deletes every other file there, reuses the file when its digest matches, else reports `Downloading` (0, then ~100 steps) and downloads (failure → `Errored` manual / `Idle` automatic), verifies the digest (mismatch → deletes, `Errored("… is damaged …")`); Windows installs → `Updated { apply_on_quit: Some }`; root needed → `ReadyToInstall`; else `Installing` then the install result (`after_install`).
- **`install_with_root(installer, prepared, elevation) -> Status`** — installs after the user agreed; result through `after_install`.
- **`elevation_for(os, password, mac_prompt) -> Elevation`** — macOS → `MacAdmin`; a typed password → `Sudo`; else `Polkit`.
- **`after_install`** (private) — `Done` → `Updated { apply_on_quit: None }`; `Cancelled` → `ReadyToInstall`; `NoAgent` → `NeedsPassword { wrong: false }`; `WrongPassword` → `NeedsPassword { wrong: true }`; `Failed` → `Errored`.
- **`on_quit(status, relaunch)`** — `Updated` with `apply_on_quit` → `platform::apply_on_quit`; without, when `relaunch` → `platform::relaunch`; anything else: nothing.
- **`sha256(file) -> io::Result<String>`** — lowercase hex, streamed in 64 KiB chunks.
- **`digest_matches(file, digest) -> Result<bool, String>`** — compares to `sha256:<hex>` (case-insensitive); true without a digest; error when the file can't be read.

### `src/version.rs`

- **struct `Version`** — `parse(s) -> Option<Self>`: `[v]X.Y.Z[-pre][+build]`, build ignored; rejects missing/extra parts and empty prerelease identifiers. `Ord`: core numbers, then a prerelease sorts before its release; identifiers numeric < alphanumeric, numeric by value, more identifiers sort later (SemVer).

### `src/install.rs`

- **enum `Os { Linux, MacOs, Windows }`** — `current()` by `cfg`.
- **enum `Package { Deb, Rpm, Pacman }`** — `extension()` (`deb`/`rpm`/`pkg.tar.zst`); `mode()` (pub(crate)): the `install.sh` mode.
- **enum `Install`** — `MacApp(app)`, `AppImage(file)`, `Bundle(app)`, `Prefix(prefix)`, `Package(Package)`, `WindowsSetup(exe)`, `WindowsMsi(exe)`, `Unsupported`.
- **struct `Environment { exe, appimage, local_app_data, program_files }`** — `current()`: canonical `current_exe`; `APPIMAGE` only when the exe lives inside `APPDIR`; `LOCALAPPDATA`, `ProgramFiles`/`ProgramW6432`.
- **`classify(env, os) -> Install`** — macOS: `…/X.app/Contents/MacOS/*` not under `/Volumes` or `AppTranslocation`; Linux: AppImage first, then `bin/bgmail` under `bgmail.app` (Bundle) or `/usr`, `/usr/local` (Prefix); Windows: `BGMail.exe` (case-insensitive, either separator) under `LOCALAPPDATA` (Setup) or a Program Files dir (Msi); else `Unsupported`.
- **`detect() -> Install`** — `classify` of the running exe; a Prefix owned by a package manager becomes `Package` (`platform::package_owner`). Runs commands: background thread.
- **`asset_name(install, names, arch) -> Option<&str>`** — the release file for the install (see crate contract); Windows arch `x64`/`arm64`.
- **`needs_root(install) -> bool`** — Prefix/Package: not root; MacApp/Bundle: folder or its parent not writable; AppImage: parent not writable; Windows/Unsupported: false.
- **`writable(dir)`** (private) — creates and deletes a probe file.

### `src/platform/mod.rs`

- **`SCRIPT`** (`install.sh`), **`STARTED`** (`bgmail-update: start`), **`WAIT_THEN_EXEC`** (`sh -c` waiting on `$0` then `exec "$@"`).
- **`script_args(prepared)`** — `[mode, file, target]` per install (`Package`: `[mode, file]`); `None` for Windows/Unsupported.
- **`launch_command(install)`** — program + args that start the installed copy: `open -n <app>`, the AppImage, `<app>/bin/bgmail`, `<prefix>/bin/bgmail`, `/usr/bin/bgmail` for packages, the Windows exe; `None` when unsupported.
- **`shell_quote`**, **`applescript_string`**, **`powershell_quote`** — quoting for sh, AppleScript and PowerShell single quotes.
- **`windows_helper_script(prepared, pid, relaunch)`** — `Wait-Process`, then `Start-Process … -Wait` of the setup (`/S`) or `msiexec /i "<msi>" /passive /norestart`, `Remove-Item` of the installer, then `Start-Process <exe>` when relaunching; `None` for non-Windows installs.

### `src/platform/unix.rs` (`cfg(unix)`)

- **`has(program)`**, **`output(program, args)`** (private) — PATH lookup; stdout of a successful quiet run.
- **`is_root`** — `id -u` is 0.
- **`package_owner(exe)`** — `dpkg-query -S` → Deb, `pacman -Qqo` → Pacman, `rpm -qf` → Rpm (each only when installed).
- **`run_with_input`**, **`failure`** (private) — run with an optional stdin line; stderr (or exit status) as the failure.
- **`install(prepared, elevation, updates_dir) -> Outcome`** — writes the script into `updates_dir`, runs it per `Elevation` (see crate contract), deletes `updates_dir` on success.
- **`relaunch(install)`** — detached (`process_group(0)`) `sh -c WAIT_THEN_EXEC <pid> <launch_command>`, without `APPIMAGE`/`APPDIR`.
- **`apply_on_quit`** — no-op (Windows only).

### `src/platform/windows.rs` (`cfg(windows)`)

- **`is_root`** → false; **`package_owner`** → None; **`install`** → `Failed` (Windows installs on quit).
- **`powershell(script)`** (private) — hidden `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command`, `CREATE_NO_WINDOW`.
- **`apply_on_quit(prepared, relaunch)`** — runs `windows_helper_script` for this PID.
- **`relaunch(install)`** — waits for this PID, then starts the exe.

### `src/platform/install.sh`

- Modes `bundle`, `prefix`, `appimage`, `deb`, `rpm`, `pacman`, `macapp` (crate contract); prints `bgmail-update: start` first; exits 3 with a message on failure.

### Tests (`src/tests.rs`, module tests)

- Pipeline with `FakeSource`/`FakeInstaller`: quiet vs manual results, source builds only told, silent install, packages wait and pick their format, Windows installs on quit, damaged and failed downloads, download reuse/cleanup, root outcomes, elevation choice.
- `platform::unix::tests` run the real script: bundle replaced in place (no leftovers), prefix with absolute desktop paths, a bad tarball keeps the install, package modes call the right manager (fakes on PATH), the relaunch waits for the old PID.
