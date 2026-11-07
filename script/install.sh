#!/usr/bin/env sh
set -eu

# Installs BGMail from the GitHub releases. On Debian/Ubuntu, Fedora/RHEL/
# openSUSE and Arch (and their derivatives) it installs the native package
# (.deb, .rpm, .pkg.tar.zst) through the package manager, as root; elsewhere,
# without root, or when the release has no such package, it unpacks the
# standalone tarball into ~/.local/. Environment:
#   BGMAIL_VERSION      release tag, e.g. v0.1.0 (default: latest)
#   BGMAIL_INSTALL      "local" skips the package manager and installs into ~/.local
#   BGMAIL_BUNDLE_PATH  install this local tarball into ~/.local instead of downloading
#   BGMAIL_OS_RELEASE   os-release file to detect the distro from (default: /etc/os-release)

REPO="belizariogr/bgmail"

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    if [ "$platform" != "Linux" ]; then
        echo "This script installs BGMail on Linux. Unsupported platform: $platform"
        echo "Other systems: https://github.com/$REPO/releases"
        exit 1
    fi
    case "$arch" in
        x86_64 | aarch64) ;;
        arm64) arch="aarch64" ;;
        *)
            echo "Unsupported architecture: $arch"
            exit 1
            ;;
    esac

    # Use TMPDIR if available (for environments with non-standard temp directories)
    temp="$(mktemp -d "${TMPDIR:-/tmp}/bgmail-XXXXXX")"
    trap 'rm -rf "$temp"' EXIT
    # The package manager may read the download as another user (apt's _apt).
    chmod 755 "$temp"

    if [ -z "${BGMAIL_BUNDLE_PATH:-}" ] && [ "${BGMAIL_INSTALL:-}" != "local" ] \
        && format="$(package_format)"; then
        if root_runner; then
            package="$temp/bgmail-linux-$arch.$format"
            if download "bgmail-linux-$arch.$format" "$package"; then
                chmod 644 "$package"
                install_package "$format" "$package"
                return
            fi
            echo "This release has no .$format package for $arch; installing into ~/.local instead."
        else
            echo "Neither sudo nor doas found; installing into ~/.local instead of a .$format package."
        fi
    fi

    bundle="$temp/bgmail-linux-$arch.tar.gz"
    if [ -n "${BGMAIL_BUNDLE_PATH:-}" ]; then
        cp "$BGMAIL_BUNDLE_PATH" "$bundle"
    elif ! download "bgmail-linux-$arch.tar.gz" "$bundle"; then
        echo "There may be no release for $arch yet. To build from source: script/install-linux"
        exit 1
    fi

    install_bundle "$bundle"

    if [ "$(command -v bgmail)" = "$HOME/.local/bin/bgmail" ]; then
        echo "BGMail has been installed. Run with 'bgmail'"
    else
        echo "BGMail has been installed. To run it from your terminal, add ~/.local/bin to your PATH"
        echo "Run:"
        case "${SHELL:-}" in
            *zsh)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.zshrc"
                echo "   source ~/.zshrc"
                ;;
            *fish)
                echo "   fish_add_path -U $HOME/.local/bin"
                ;;
            *)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.bashrc"
                echo "   source ~/.bashrc"
                ;;
        esac
        echo "To run BGMail now, '~/.local/bin/bgmail'"
    fi
}

# Prints the package format of this distro (deb, rpm, pkg.tar.zst) when its
# package manager is installed. The distro and the distros it derives from
# (ID_LIKE) are tried in order, so Mint, Manjaro or Rocky get their parent's.
package_format() {
    os_release="${BGMAIL_OS_RELEASE:-/etc/os-release}"
    [ -r "$os_release" ] || return 1
    ids="$(. "$os_release" && echo "${ID:-} ${ID_LIKE:-}")"
    for id in $ids; do
        case "$id" in
            arch)
                if command -v pacman >/dev/null 2>&1; then echo pkg.tar.zst && return 0; fi
                ;;
            debian | ubuntu)
                if command -v apt-get >/dev/null 2>&1; then echo deb && return 0; fi
                ;;
            fedora | rhel | centos | suse | opensuse*)
                for tool in dnf zypper yum; do
                    if command -v "$tool" >/dev/null 2>&1; then echo rpm && return 0; fi
                done
                ;;
        esac
    done
    return 1
}

# Sets $as_root to the command prefix that runs a command as root.
root_runner() {
    if [ "$(id -u)" -eq 0 ]; then
        as_root=""
    elif command -v sudo >/dev/null 2>&1; then
        as_root="sudo"
    elif command -v doas >/dev/null 2>&1; then
        as_root="doas"
    else
        return 1
    fi
}

download() {
    version="${BGMAIL_VERSION:-latest}"
    if [ "$version" = "latest" ]; then
        url="https://github.com/$REPO/releases/latest/download/$1"
    else
        url="https://github.com/$REPO/releases/download/$version/$1"
    fi
    echo "Downloading BGMail ($version): $1"
    if command -v curl >/dev/null 2>&1; then
        fetch="curl -fL -o"
    elif command -v wget >/dev/null 2>&1; then
        fetch="wget -O"
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi
    if ! $fetch "$2" "$url"; then
        echo "Could not download $url"
        return 1
    fi
}

install_package() {
    # Unattended: a piped script has no terminal on stdin to answer prompts
    # (sudo asks on the terminal itself).
    case "$1" in
        pkg.tar.zst)
            manager="pacman"
            $as_root pacman -U --noconfirm "$2"
            ;;
        deb)
            manager="apt"
            $as_root apt-get install -y "$2"
            ;;
        rpm)
            if command -v dnf >/dev/null 2>&1; then
                manager="dnf"
                $as_root dnf install -y "$2"
            elif command -v zypper >/dev/null 2>&1; then
                manager="zypper"
                $as_root zypper --non-interactive install --allow-unsigned-rpm "$2"
            else
                manager="yum"
                $as_root yum install -y "$2"
            fi
            ;;
    esac

    # An earlier ~/.local install would shadow the package on PATH and in the
    # launcher. Only its program files go; ~/.config/BGMail stays.
    if [ -d "$HOME/.local/bgmail.app" ]; then
        rm -rf "$HOME/.local/bgmail.app" "$HOME/.local/share/applications/BGMail.desktop"
        if [ -L "$HOME/.local/bin/bgmail" ]; then rm -f "$HOME/.local/bin/bgmail"; fi
        echo "Removed the previous BGMail install from ~/.local (your mail and settings are kept)."
    fi

    remove="$manager remove bgmail"
    if [ "$manager" = "pacman" ]; then remove="pacman -R bgmail"; fi
    echo "BGMail has been installed with $manager. Run with 'bgmail'"
    echo "To uninstall: ${as_root:+$as_root }$remove"
}

install_bundle() {
    # Unpack aside first: a bad archive must not cost a working install.
    tar -xzf "$1" -C "$temp"
    if [ ! -x "$temp/bgmail.app/bin/bgmail" ]; then
        echo "Invalid BGMail bundle: bin/bgmail is missing"
        exit 1
    fi

    app="$HOME/.local/bgmail.app"
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
    rm -rf "$app"
    mv "$temp/bgmail.app" "$app"

    if command -v ldd >/dev/null 2>&1; then
        missing="$(ldd "$app/bin/bgmail" 2>/dev/null | sed -n 's/^[[:space:]]*\(.*\) => not found$/\1/p')"
        if [ -n "$missing" ]; then
            echo "Warning: your system is missing libraries that BGMail needs:"
            echo "$missing" | sed 's/^/    /'
            echo "Install them with your package manager, or BGMail will fail to start."
        fi
    fi

    ln -sf "$app/bin/bgmail" "$HOME/.local/bin/bgmail"

    # Absolute paths: ~/.local/bin is often not in the PATH of desktop sessions.
    sed -e "s|^Exec=bgmail|Exec=$app/bin/bgmail|" \
        -e "s|^Icon=bgmail|Icon=$app/share/icons/hicolor/512x512/apps/bgmail.png|" \
        "$app/share/applications/BGMail.desktop" \
        > "$HOME/.local/share/applications/BGMail.desktop"
}

main "$@"
