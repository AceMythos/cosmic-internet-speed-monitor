# Internet Speed Monitor

A COSMIC desktop applet that monitors internet speed and data usage in real time.

![Applet in panel](screenshots/Screenshot_2026-07-19_21-54-58.png)

## Features

- Live download/upload speed displayed in the panel
- Hourly activity sparkline for the current day
- Monthly bar chart with per-day totals, highest/average/lowest
- Click any bar to see the exact value
- Connection details: IPv4, IPv6, gateway, DNS, interface, connection duration
- Wi-Fi info: SSID, signal strength, link speed (requires `iw`)
- Configurable: refresh interval, speed units (bps/bytes), panel preset, network interface, data retention (7-90 days)
- Reset today's or this month's stats

## Screenshots

![Popdown detail view](screenshots/Screenshot_2026-07-19_21-54-16.png)

## Installation

### Build from source

Requires Rust 1.80+ and a working COSMIC/Wayland session.

```sh
git clone https://github.com/AceMythos/cosmic-internet-speed-monitor
cd cosmic-internet-speed-monitor
cargo build --release
sudo install -m 755 target/release/internet-speed-monitor /usr/bin/
sudo cp internet-speed-monitor.desktop /usr/share/applications/
```

See [BUILD.md](BUILD.md) for detailed instructions and quick rebuild commands.

### Wi-Fi support

Signal strength works out of the box. For SSID and link speed, install `iw`:

```sh
sudo apt install iw
```

## Adding to panel

COSMIC Settings → Desktop → Panel → Add applet.

If it doesn't appear, restart the panel: `killall cosmic-panel`

## Logs

```sh
journalctl -p 3 -xb --user _EXE=/usr/bin/internet-speed-monitor
```

## License

GPL-3.0-or-later
