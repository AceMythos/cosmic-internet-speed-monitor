#!/usr/bin/env bash
# Build and install Internet Speed Monitor as a COSMIC panel applet.
set -euo pipefail

cd -- "$(dirname -- "$0")"

BINARY=target/release/internet-speed-monitor
BIN_DEST=/usr/bin/internet-speed-monitor
DESKTOP_SRC=internet-speed-monitor.desktop
DESKTOP_DEST=/usr/share/applications/internet-speed-monitor.desktop

say() { printf '\n==> %s\n' "$1"; }

if ! command -v cargo >/dev/null 2>&1; then
    printf 'error: cargo was not found.\n\n'
    printf 'Install Rust 1.85 or newer first:\n'
    printf '  curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh\n'
    printf '  source "$HOME/.cargo/env"\n'
    exit 1
fi

if ! command -v pkexec >/dev/null 2>&1; then
    printf 'error: pkexec was not found. Install polkit, or run the manual steps in README.md.\n'
    exit 1
fi

say "Building release binary (this can take a few minutes the first time)"
cargo build --release --locked

if [ ! -x "$BINARY" ]; then
    printf 'error: build finished but %s is missing.\n' "$BINARY"
    exit 1
fi

say "Installing $BIN_DEST (you will be asked for your password)"
pkexec install -m 755 "$PWD/$BINARY" "$BIN_DEST"

say "Registering the applet in $DESKTOP_DEST"
pkexec install -m 644 "$PWD/$DESKTOP_SRC" "$DESKTOP_DEST"

if ! command -v iw >/dev/null 2>&1; then
    printf '\nnote: iw is not installed, so Wi-Fi name, signal and link speed will be blank.\n'
    printf '      Install it with: sudo apt install iw\n'
fi

say "Done. The applet is installed."
printf '    Add it from: COSMIC Settings -> Desktop -> Panel -> Add applet\n'

if pgrep -x cosmic-panel >/dev/null 2>&1; then
    printf '\nThe panel needs a restart before the applet shows up.\n'
    printf 'Restart it now? [Y/n] '
    reply=''
    read -r reply || true
    case "$reply" in
        [nN]*) ;;
        *)
            say "Restarting the panel"
            killall cosmic-panel
            sleep 3
            if ! pgrep -x cosmic-panel >/dev/null 2>&1; then
                printf 'The panel did not come back on its own. Log out and back in to restore it.\n'
            fi
            ;;
    esac
fi

printf '\nIf the applet is not listed in Add applet yet, log out and back in once.\n'
