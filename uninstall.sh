#!/usr/bin/env bash
# Remove Internet Speed Monitor from this machine.
set -euo pipefail

BIN_DEST=/usr/bin/internet-speed-monitor
DESKTOP_DEST=/usr/share/applications/internet-speed-monitor.desktop
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/com.github.AceMythos.InternetSpeedMonitor"

say() { printf '\n==> %s\n' "$1"; }

if ! command -v pkexec >/dev/null 2>&1; then
    printf 'error: pkexec was not found. Remove the files manually:\n'
    printf '  sudo rm -f %s %s\n' "$BIN_DEST" "$DESKTOP_DEST"
    exit 1
fi

say "Removing the applet binary and desktop entry"
pkexec rm -f "$BIN_DEST" "$DESKTOP_DEST"

if [ -d "$DATA_DIR" ]; then
    printf '\nYour speed history is stored in:\n  %s\n' "$DATA_DIR"
    printf 'Delete it too? This cannot be undone. [y/N] '
    reply=''
    read -r reply || true
    case "$reply" in
        [yY]*)
            rm -rf -- "$DATA_DIR"
            printf 'Deleted speed history.\n'
            ;;
        *)
            printf 'Kept speed history.\n'
            ;;
    esac
fi

say "Done."
printf 'Restart the panel (or log out and back in) to drop the applet from the panel:\n'
printf '  killall cosmic-panel\n'
