# script/ — Install, packaging and asset scripts

## Purpose

Install BGMail on Linux without a package manager, following Zed's `script/`;
build the macOS installer DMG; regenerate icon rasters. Other packages (deb,
AppImage, pacman, .app, Windows) stay with cargo-packager.

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
- `bundle-macos-dmg APP OUT.dmg`: installer image with BGDesk's window
  (`crates/bgmail/packaging/macos/`: background art, White Drive volume icon,
  app at (200,190), Applications at (600,190), hidden files at (2000,2000)).
  Layout via `dmgbuild` (`DMGBUILD` or PATH; pinned 1.6.7), command line only:
  never Finder/AppleScript (fails on CI). With `APPLE_SIGNING_IDENTITY` (+
  optional `APPLE_KEYCHAIN`) signs the image; with notarization credentials
  (`APPLE_KEYCHAIN_PROFILE`, or `APPLE_ID`+`APPLE_PASSWORD`+`APPLE_TEAM_ID`, or
  `APPLE_API_KEY`+`APPLE_API_ISSUER`+`APPLE_API_KEY_PATH`, first complete set
  wins) notarizes (fails unless `Accepted`, prints the log), staples (5 tries),
  validates and runs `spctl`. Writes the output only after every step passed.
- `test-macos-dmg`: builds an unsigned DMG from a dummy app and checks its
  contents, volume icon, HiDPI background and `.DS_Store` layout (window
  800x480, icon view, positions). Runs in the CI macOS job.
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
- The tarball and `install.sh` are published by `.github/workflows/release.yml`;
  the download URL in `install.sh` depends on their names.

## Verification

- `script/test-install` (no build, no network; runs in the CI Linux job).
- `script/test-macos-dmg` (macOS, needs dmgbuild; runs in the CI macOS job).
