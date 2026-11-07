# script/ — Install, packaging and asset scripts

## Purpose

Install BGMail on Linux without a package manager, following Zed's `script/`;
build the macOS installer DMG; regenerate icon rasters; build/install the
native rpm and Arch packages. Release deb, AppImage, .app and Windows
packages stay with cargo-packager.

## Ownership

- `bundle-linux`: builds `target/release/bgmail-linux-<arch>.tar.gz` holding
  `bgmail.app/{bin/bgmail, share/applications/BGMail.desktop, share/icons/hicolor}`.
  Renders the desktop template shared with cargo-packager
  (`crates/bgmail/packaging/linux/BGMail.desktop`). `BGMAIL_BINARY` packages an
  existing binary instead of building.
- `install.sh`: POSIX sh (dash-safe). Picks the distro's native package from
  `/etc/os-release` (`BGMAIL_OS_RELEASE` overrides), trying `ID` then each
  `ID_LIKE`: `arch` → `.pkg.tar.zst` (pacman), `debian`/`ubuntu` → `.deb`
  (apt-get), `fedora`/`rhel`/`centos`/`suse`/`opensuse*` → `.rpm` (dnf, else
  zypper with `--allow-unsigned-rpm`, else yum); an id counts only when its
  package manager is on PATH. Downloads `bgmail-linux-<arch>.<format>` and
  installs it unattended (`--noconfirm`/`-y`: a piped script has no stdin)
  through root, `sudo` or `doas`; then removes an earlier `~/.local` install
  (app, link, desktop entry; never `~/.config/BGMail`), which would shadow it,
  and prints the uninstall command. Falls back to the tarball in
  `~/.local/bgmail.app` (with `~/.local/bin/bgmail` and a desktop entry using
  absolute paths) when no distro matches, no root helper exists, the release
  lacks that package (says so), `BGMAIL_INSTALL=local`, or
  `BGMAIL_BUNDLE_PATH` is set. `BGMAIL_VERSION` picks a tag.
- `install-linux`: `bundle-linux` + `install.sh` for this checkout.
- `install-linux-system`: `bundle-linux` + install into `$PREFIX` (default
  `/usr`) through `$SUDO`; `--uninstall` removes those files.
- `uninstall.sh`: removes the `~/.local` install.
- `bundle-linux-rpm`: `bundle-linux` tree → `rpmbuild -bb`
  (`crates/bgmail/packaging/linux/bgmail.spec`, version `-` → `~`) →
  `target/release/bgmail-linux-<arch>.rpm`; prints the path (rpmbuild output
  goes to stderr). Needs rpmbuild. Honors `BGMAIL_BINARY`, `CARGO_TARGET_DIR`.
- `bundle-linux-pacman`: the PKGBUILD in a temp copy (`makepkg --nodeps`,
  `PKGEXT=.pkg.tar.zst`) → `target/release/bgmail-linux-<arch>.pkg.tar.zst`;
  prints the path, makepkg output to stderr. Needs makepkg (CI: archlinux
  container, non-root user). Honors `BGMAIL_BINARY`, `CARGO_TARGET_DIR`.
- `install-arch [makepkg options]`: copies
  `crates/bgmail/packaging/linux/PKGBUILD` to a temp dir and runs
  `makepkg --syncdeps --install --force` with `BGMAIL_SRC` = this checkout and
  `PKGDEST=target/pacman`, so the checkout stays clean and pacman owns the
  install (`pacman -R bgmail` removes it). Sources `~/.cargo/env` when cargo is
  off PATH. Refuses up front when `/usr/bin/bgmail` exists unowned (an
  `install-linux-system` install pacman would refuse to overwrite).
- `test-rpm-package`: `bundle-linux-rpm` with a dummy binary; checks name,
  version (`~`), arch, soname `Requires`, file list (under `/usr`, no
  directories owned) and the extracted desktop entry. `SKIP` without rpmbuild.
- `test-arch-package`: `bundle-linux-pacman` with a dummy binary; checks `.PKGINFO` (name, Cargo version, deps, conflict), the
  file list (all under `/usr`), the desktop entry and a clean checkout. Prints
  `SKIP` without makepkg.
- `test-install`: exercises the tarball/`~/.local` and system scripts with a
  dummy binary, a temp `HOME` and a temp prefix, and `install.sh`'s package
  choice with fake `curl`/`sudo`/package managers per distro (Arch, Mint,
  openSUSE, Rocky; fallbacks for a missing package, no package manager, an
  unknown distro, `BGMAIL_INSTALL=local`; replacing a `~/.local` install).
- `bundle-macos-dmg APP OUT.dmg`: installer image with BGDesk's window
  (`crates/bgmail/packaging/macos/`: background art, White Drive volume icon,
  app at (200,190), Applications at (600,190), hidden files at (2000,2000);
  no `hide_extensions`, whose FinderInfo xattr breaks `codesign --strict`).
  Layout via `dmgbuild` (`DMGBUILD` or PATH; pinned 1.6.7), command line only:
  never Finder/AppleScript (fails on CI). With `APPLE_SIGNING_IDENTITY` (+
  optional `APPLE_KEYCHAIN`) signs the image. With notarization credentials
  (`APPLE_KEYCHAIN_PROFILE`, or `APPLE_ID`+`APPLE_PASSWORD`+`APPLE_TEAM_ID`, or
  `APPLE_API_KEY`+`APPLE_API_ISSUER`+`APPLE_API_KEY_PATH`, first complete set
  wins) it first notarizes (zip via `ditto`) and staples the signed `.app` in
  place, then the image; each must be `Accepted` (else prints the log), and
  `spctl` must accept each. Stapling: `xcrun stapler` exits 68 when it loses
  `api.apple-cloudkit.com` (always on GitHub's macOS runners, as BGDesk found):
  3 tries (IPv6 off after the first on CI, `sudo -n networksetup`), then the
  same ticket via `rcodesign staple` (apple-codesign 0.22.0, downloaded and
  SHA-256 checked); a 68 from `stapler validate` is then tolerated. Other
  stapler codes fail at once. Writes the output only after every step passed.
- `test-macos-dmg`: builds an unsigned DMG from a dummy app and checks its
  contents, volume icon, HiDPI background, no FinderInfo/ResourceFork xattr on
  the app, and `.DS_Store` layout (window 800x480, icon view, positions). Runs
  in the CI macOS job.
- `generate-macos-dmg-assets`: rasterizes `background.svg` (800x400 + @2x) and
  `volume-icon.svg` (ICNS via `iconutil`; the app icon `BGMail.svg` is copied
  beside it first, librsvg refuses parent paths). Needs `rsvg-convert`; only
  run after editing the art, CI uses the committed rasters.
- `generate-app-icons`: `assets/appIcon/BGMail.svg` → `BGMail-{16..1024}.png`,
  `BGMail.ico`, `BGMail.icns` (`rsvg-convert` + `python3`).

## Local Contracts

- User data (`~/.config/BGMail`, always that path: the app ignores
  `XDG_CONFIG_HOME`) survives every uninstall path. Only `uninstall.sh` may delete
  it, after an explicit yes on a terminal.
- A bad tarball must not cost a working `~/.local` install: unpack aside, validate,
  then replace.
- The desktop file is named after the GPUI app id (`BGMail.desktop`).
- The tarball, `bgmail-linux-<arch>.{deb,rpm,pkg.tar.zst}` and `install.sh` are
  published by `.github/workflows/release.yml` under those stable names;
  `install.sh` downloads them by name from `releases/latest/download`.
- Native packages are all named `bgmail` and install `/usr/bin/bgmail`, the
  desktop entry and the icons from the same `bundle-linux` tree.

## Verification

- `script/test-install` (no build, no network; runs in the CI Linux job).
- `script/test-rpm-package` (needs rpmbuild; CI Linux job).
- `script/test-arch-package` (needs makepkg; CI Linux job in an archlinux
  container).
- `script/test-macos-dmg` (macOS, needs dmgbuild; runs in the CI macOS job).
