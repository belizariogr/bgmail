# crates/auto_update/ — Updates from GitHub Releases

## Purpose

Updates an installed BGMail to the latest GitHub release: compares versions,
works out how this copy was installed, picks the release file that replaces
it, verifies its SHA-256 against GitHub's digest, and installs it per
platform. Ported from BGPaint's `update` module; named after Zed's
`auto_update`.

## Ownership

- Owns: `crates/auto_update/**` (`src/install.sh` is the embedded Linux/macOS
  installer).
- Does not own: HTTP (`protocols::release`), UI, settings and scheduling
  (`bgmail::auto_update_ui`), release asset names (published by
  `.github/workflows/release.yml`).

## Local Contracts

- **No GPUI, no network, no C dependencies** (only `sha2`): the app supplies a
  `ReleaseSource`, and the crate checks for every target with
  `cargo clippy -p auto_update --target <triple>`. `#![forbid(unsafe_code)]`.
- **Install → release file** (`asset_name`, `arch` = `std::env::consts::ARCH`):
  `~/.local/bgmail.app` and `/usr`/`/usr/local` prefixes →
  `bgmail-linux-<arch>.tar.gz`; packages owned by dpkg / pacman / rpm →
  `bgmail-linux-<arch>.{deb,pkg.tar.zst,rpm}`; AppImage →
  `BGMail_<v>_<arch>.AppImage`; macOS `.app` → `BGMail_<v>_<arch>.dmg`
  (not from `/Volumes` or AppTranslocation); Windows per-user NSIS
  (`%LOCALAPPDATA%`) → `BGMail_<v>_x64-setup.exe`, MSI (`%ProgramFiles%`) →
  `BGMail_<v>_x64_*.msi`; anything else (built from source) is
  `Unsupported` and only reported (`Status::Available`).
- **Root:** prefix and package installs always (unless already root); bundle,
  AppImage and `.app` only when their folder isn't writable. Asked through
  `pkexec` (126 = cancelled, no `bgmail-update: start` marker = no agent →
  BGMail asks for the password and runs `sudo -S`, checking it first with
  `sudo -v`), or the macOS administrator prompt (`osascript`, -128 =
  cancelled). Silent when no root is needed.
- **Install script** (`install.sh` modes `bundle`, `prefix`, `appimage`,
  `deb`, `rpm`, `pacman`, `macapp`): new files staged beside the old and
  swapped with a rename, so a failure keeps the working install; `prefix`
  writes absolute `Exec`/`Icon` like `script/install-linux-system`; `rpm`
  prefers dnf, then zypper (`--allow-unsigned-rpm`), yum, plain `rpm -U`;
  `pacman -U --noconfirm`; `apt-get install -y` (else `dpkg -i`); `macapp`
  accepts a signed app only with the same Team ID.
- **Windows** never installs while running: the download becomes
  `Updated { apply_on_quit }` and `on_quit` starts a hidden PowerShell that
  waits for the process, runs the NSIS installer with `/S` or `msiexec /i …
  /passive /norestart` (UAC), deletes it and, on restart, starts BGMail.
- **Relaunch** (`on_quit` with `relaunch`): a detached `sh` waits for this PID
  (`kill -0`) before exec'ing the installed copy: one BGMail runs per profile
  (`instance.rs`), so an early start would hand over to the quitting one.
- **Quiet automatic checks:** failures to reach or download a release report
  `Idle`; a manual check reports them. A damaged download (digest mismatch)
  is always an error. A complete download whose digest matches is reused;
  other files in the updates dir are deleted.
- `automatic_checks_allowed`: not in debug builds, not with
  `BGMAIL_NO_UPDATE`.

## Work Guidance

- Keep `asset_name` in step with the release workflow's file names, and the
  table in `src/lib.rs` docs with README "Updates".
- Command strings (script args, relaunch, PowerShell) are built by plain
  functions in `platform/mod.rs` so they are tested on every OS; `unix.rs` /
  `windows.rs` only spawn them.

## Verification

- `cargo test -p auto_update` (runs `install.sh` for real on temp dirs and with
  fake package managers on PATH)
- `cargo clippy -p auto_update --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
  and `--target x86_64-apple-darwin` (needs only `rustup target add`)

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/auto_update/src/` | [`.dox/crates/auto_update/src/AGENTS.md`](src/AGENTS.md) | Source API catalog |
