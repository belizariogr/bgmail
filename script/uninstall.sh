#!/usr/bin/env sh
set -eu

# Removes what script/install.sh installed. Your mail database and settings
# stay unless you ask to delete them, and that question is only asked on a
# terminal. Account passwords stay in the system keyring; removing an account
# inside BGMail first deletes them.

main() {
    data="$HOME/.config/BGMail"

    rm -rf "$HOME/.local/bgmail.app"
    rm -f "$HOME/.local/bin/bgmail"
    rm -f "$HOME/.local/share/applications/BGMail.desktop"
    echo "BGMail has been uninstalled."

    if [ -d "$data" ]; then
        response=""
        if [ -t 0 ]; then
            printf "Also delete your local mail and settings in %s? [y/N] " "$data"
            read -r response
        fi
        case "$response" in
            [yY] | [yY][eE][sS])
                rm -rf "$data"
                echo "Deleted $data"
                ;;
            *)
                echo "Your local mail and settings are still in $data"
                ;;
        esac
    fi
}

main "$@"
