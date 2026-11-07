#!/bin/sh
# Installs a downloaded BGMail update. Embedded in the app and run by
# crates/auto_update, as root (pkexec, sudo or the macOS administrator prompt)
# when the install needs it.
#
#   install.sh bundle   <tarball>    <bgmail.app folder>  (script/install.sh, in ~/.local)
#   install.sh prefix   <tarball>    <prefix>             (script/install-linux-system)
#   install.sh appimage <AppImage>   <AppImage to replace>
#   install.sh deb      <package.deb>
#   install.sh rpm      <package.rpm>
#   install.sh pacman   <package.pkg.tar.zst>
#   install.sh macapp   <dmg>        <BGMail.app>
#
# New files are put next to the old ones first and swapped in with a rename, so
# a failure leaves the working install alone.

set -eu
# Tells the app that authorization succeeded and this script ran.
echo "bgmail-update: start"

mode=$1
package=$2
target=${3:-}
umask 022

fail() {
    echo "$1" >&2
    exit 3
}

# Unpacks the release tarball (bgmail.app/{bin,share}) into $1.
unpack() {
    tar -xzf "$package" -C "$1" || fail "Could not unpack $package."
    [ -x "$1/bgmail.app/bin/bgmail" ] || fail "Invalid package: bin/bgmail is missing."
}

case "$mode" in
bundle)
    stage=$(mktemp -d "$(dirname "$target")/.bgmail-update.XXXXXX")
    trap 'rm -rf "$stage"' EXIT
    unpack "$stage"
    mv "$target" "$stage/old"
    if ! mv "$stage/bgmail.app" "$target"; then
        mv "$stage/old" "$target"
        fail "Could not replace $target."
    fi
    ;;

prefix)
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    unpack "$stage"
    app=$stage/bgmail.app
    desktop=BGMail.desktop
    # Absolute paths, as script/install-linux-system writes them.
    sed -i -e "s|^Exec=bgmail|Exec=$target/bin/bgmail|" \
        -e "s|^Icon=bgmail|Icon=$target/share/icons/hicolor/512x512/apps/bgmail.png|" \
        "$app/share/applications/$desktop"
    # A rename, not a copy over the running binary.
    install -Dm755 "$app/bin/bgmail" "$target/bin/.bgmail.new"
    mv -f "$target/bin/.bgmail.new" "$target/bin/bgmail"
    install -Dm644 "$app/share/applications/$desktop" "$target/share/applications/$desktop"
    for dir in "$app"/share/icons/hicolor/*/apps; do
        for file in "$dir"/*; do
            install -Dm644 "$file" "$target/${dir#"$app"/}/$(basename "$file")"
        done
    done
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f "$target/share/icons/hicolor" >/dev/null 2>&1 || true
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$target/share/applications" >/dev/null 2>&1 || true
    fi
    ;;

appimage)
    new="$(dirname "$target")/.bgmail-update.$$.AppImage"
    trap 'rm -f "$new"' EXIT
    cp "$package" "$new"
    chmod 755 "$new"
    mv -f "$new" "$target"
    ;;

deb)
    export DEBIAN_FRONTEND=noninteractive
    if command -v apt-get >/dev/null 2>&1; then
        apt-get install -y "$package"
    else
        dpkg -i "$package"
    fi
    ;;

rpm)
    # The package manager fetches new dependencies; plain rpm only as a last resort.
    if command -v dnf >/dev/null 2>&1; then
        dnf install -y "$package"
    elif command -v zypper >/dev/null 2>&1; then
        zypper --non-interactive install --allow-unsigned-rpm "$package"
    elif command -v yum >/dev/null 2>&1; then
        yum install -y "$package"
    else
        rpm -U "$package"
    fi
    ;;

pacman)
    pacman -U --noconfirm "$package"
    ;;

macapp)
    parent=$(dirname "$target")
    mnt=$(mktemp -d "${TMPDIR:-/tmp}/bgmail-dmg.XXXXXX")
    new="$parent/.bgmail-update.$$.app"
    old="$parent/.bgmail-old.$$.app"
    cleanup() {
        hdiutil detach -quiet "$mnt" 2>/dev/null || hdiutil detach -force -quiet "$mnt" 2>/dev/null || true
        rmdir "$mnt" 2>/dev/null || true
        rm -rf "$new" "$old"
    }
    trap cleanup EXIT
    hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$package" >/dev/null
    src=""
    for app in "$mnt"/*.app; do
        if [ -d "$app" ]; then
            src=$app
            break
        fi
    done
    [ -n "$src" ] || fail "The disk image has no app."
    ditto "$src" "$new"
    # A signed copy only accepts an update signed by the same developer.
    team() { codesign -dv "$1" 2>&1 | sed -n 's/^TeamIdentifier=//p'; }
    current=$(team "$target")
    if [ -n "$current" ] && [ "$current" != "not set" ]; then
        codesign --verify --deep --strict "$new" || fail "The new version's signature is not valid."
        [ "$(team "$new")" = "$current" ] || fail "The new version is signed by a different developer."
    fi
    mv "$target" "$old"
    if ! mv "$new" "$target"; then
        mv "$old" "$target"
        fail "Could not replace $target."
    fi
    ;;

*)
    fail "Unknown install type: $mode"
    ;;
esac
