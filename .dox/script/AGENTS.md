# script/ — Linux install scripts

## Purpose

Install BGMail on Linux without a package manager, following Zed's `script/`.
Packages (deb, AppImage, pacman, dmg, Windows) stay with cargo-packager.

## Ownership

- `bundle-linux`: builds `target/release/bgmail-linux-<arch>.tar.gz` holding
  `bgmail.app/{bin/bgmail, share/applications/BGMail.desktop, share/icons/hicolor}`.
  Renders the desktop template shared with cargo-packager
  (`crates/bgmail/packaging/linux/BGMail.desktop`). `BGMAIL_BINARY` packages an
  existing binary instead of building.
- `install.sh`: POSIX sh. Downloads the release tarball, or takes
  `BGMAIL_BUNDLE_PATH`, and unpacks it into `~/.local/bgmail.app`, with
  `~/.local/bin/bgmail` and a desktop entry using absolute paths. `BGMAIL_VERSION`
  picks a tag.
- `install-linux`: `bundle-linux` + `install.sh` for this checkout.
- `install-linux-system`: `bundle-linux` + install into `$PREFIX` (default
  `/usr`) through `$SUDO`; `--uninstall` removes those files.
- `uninstall.sh`: removes the `~/.local` install.
- `test-install`: exercises all of the above with a dummy binary, a temp `HOME`
  and a temp prefix.

## Local Contracts

- User data (`~/.config/BGMail`, always that path: the app ignores
  `XDG_CONFIG_HOME`) survives every uninstall path. Only `uninstall.sh` may delete
  it, after an explicit yes on a terminal.
- A bad tarball must not cost a working `~/.local` install: unpack aside, validate,
  then replace.
- The desktop file is named after the GPUI app id (`BGMail.desktop`).
- The tarball and `install.sh` are published by `.github/workflows/release.yml`;
  the download URL in `install.sh` depends on their names.

## Verification

- `script/test-install` (no build, no network; runs in the CI Linux job).
