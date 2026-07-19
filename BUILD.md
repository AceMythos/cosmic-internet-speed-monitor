# Internet Speed Monitor

**Architecture scan**: https://foglamp.dev/scan/internet-speed-monitor-to4zhp

# Build & Update

## Build release

```bash
cargo build --release
```

## Install / Update

Uses `pkexec` with absolute paths to avoid working directory issues.

```bash
pkexec install -m 755 "$PWD/target/release/internet-speed-monitor" /usr/bin/internet-speed-monitor
pkexec cp "$PWD/internet-speed-monitor.desktop" /usr/share/applications/
```

## Quick rebuild + update

```bash
cargo build --release && \
pkexec install -m 755 "$PWD/target/release/internet-speed-monitor" /usr/bin/internet-speed-monitor && \
pkexec cp "$PWD/internet-speed-monitor.desktop" /usr/share/applications/
```

## Add to panel

COSMIC Settings → Desktop → Panel → Add applet.

Panel may need a restart (`killall cosmic-panel`) or a log out/in to pick up the new applet.
