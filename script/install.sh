#!/usr/bin/env sh
set -eu

# Downloads a tarball from the BGMail GitHub releases and unpacks it into
# ~/.local/. Environment:
#   BGMAIL_VERSION      release tag, e.g. v0.1.0 (default: latest)
#   BGMAIL_BUNDLE_PATH  install this local tarball instead of downloading

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

    bundle="$temp/bgmail-linux-$arch.tar.gz"
    if [ -n "${BGMAIL_BUNDLE_PATH:-}" ]; then
        cp "$BGMAIL_BUNDLE_PATH" "$bundle"
    else
        download "$bundle"
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

download() {
    version="${BGMAIL_VERSION:-latest}"
    if [ "$version" = "latest" ]; then
        url="https://github.com/$REPO/releases/latest/download/bgmail-linux-$arch.tar.gz"
    else
        url="https://github.com/$REPO/releases/download/$version/bgmail-linux-$arch.tar.gz"
    fi
    echo "Downloading BGMail version: $version"
    if command -v curl >/dev/null 2>&1; then
        fetch="curl -fL -o"
    elif command -v wget >/dev/null 2>&1; then
        fetch="wget -O"
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi
    if ! $fetch "$1" "$url"; then
        echo "Could not download $url"
        echo "There may be no release for $arch yet. To build from source: script/install-linux"
        exit 1
    fi
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
