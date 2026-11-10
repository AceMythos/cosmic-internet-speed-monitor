<p align="center">
  <img src="screenshots/Screenshot_2026-10-24_22-38-14.png" alt="Internet Speed Monitor in COSMIC panel" width="720">
</p>

<h1 align="center">Internet Speed Monitor</h1>

<p align="center">
  A COSMIC desktop applet that monitors internet speed and data usage in real time.
</p>

<p align="center">
  <a href="https://github.com/AceMythos/cosmic-internet-speed-monitor/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-GPL--3.0-blue.svg" alt="License">
  </a>
  <a href="https://www.rust-lang.org/">
    <img src="https://img.shields.io/badge/rust-1.85%2B-dea584.svg?logo=rust" alt="Rust 1.85+">
  </a>
  <a href="https://github.com/pop-os/libcosmic">
    <img src="https://img.shields.io/badge/built%20with-libcosmic-48c9b0.svg" alt="Built with libcosmic">
  </a>
</p>

## Screenshots

| Main view | Monthly summary | Settings |
|---|---|---|
| ![Main applet popdown](screenshots/Screenshot_2026-10-24_22-29-16.png) | ![Popdown with Monthly Summary expanded](screenshots/Screenshot_2026-10-24_22-30-29.png) | ![Settings with the panel style menu open](screenshots/Screenshot_2026-10-24_22-29-45.png) |

---

## Install

Tested on Pop!_OS 24.04 with a COSMIC session. Other distros running COSMIC
should work, but are untested.

The first build downloads and compiles every dependency, so give it a few
minutes. Later builds are much faster.

### 1. Install the tools

Debian, Ubuntu, and Pop!_OS:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev iw
```

`iw` is only needed for the Wi-Fi name, signal strength, and link speed.

Now install Rust 1.85 or newer. Skip this if `cargo --version` already reports
1.85 or newer:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 2. Download the code

```sh
git clone https://github.com/AceMythos/cosmic-internet-speed-monitor
cd cosmic-internet-speed-monitor
```

No `git`? Download the ZIP with the green **Code** button on GitHub, extract it,
then `cd` into the extracted folder.

### 3. Build and install

```sh
./install.sh
```

The script builds the release binary, copies it to `/usr/bin`, registers the
applet, and asks whether to restart the panel. `pkexec` asks for your admin
password.

<details>
<summary>Prefer to do it by hand?</summary>

```sh
cargo build --release --locked
pkexec install -m 755 "$PWD/target/release/internet-speed-monitor" /usr/bin/internet-speed-monitor
pkexec install -m 644 "$PWD/internet-speed-monitor.desktop" /usr/share/applications/internet-speed-monitor.desktop
```

</details>

### 4. Add it to the panel

Open **COSMIC Settings → Desktop → Panel → Add applet** and pick
**Internet Speed Monitor**.

If it is not in the list, restart the panel:

```sh
killall cosmic-panel
```

The panel starts again on its own. If it does not, log out and back in.

### If something does not work

| Problem | Fix |
|---|---|
| "No such file or directory" when running `./install.sh` | You are not inside the project folder. Run `cd cosmic-internet-speed-monitor` first. |
| `cargo: command not found` | Rust is not installed, or the shell needs a restart. Run `source "$HOME/.cargo/env"`. |
| Build fails with a `pkg-config` error | Install the packages from step 1, then run `./install.sh` again. |
| Applet is missing from **Add applet** | Restart the panel (step 4) or log out and back in. |
| Wi-Fi name, signal, or link speed is empty | Install `iw`: `sudo apt install iw`. |
| Speed stays at zero | Open the applet and check that **Interface** picked the right adapter. |
| Quota light stays ⚪ | The light needs a patched KDE Connect to show green. Without it, 5G stays hollow and LTE shows red. See [Quota light](#quota-light-paired-phone-only). |

### Uninstall

```sh
./uninstall.sh
```

It removes the binary and the applet entry, and asks before deleting your speed
history.

<details>
<summary>Manual uninstall, or if you deleted the folder</summary>

```sh
sudo rm -f /usr/bin/internet-speed-monitor /usr/share/applications/internet-speed-monitor.desktop
rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/com.github.AceMythos.InternetSpeedMonitor"
```

</details>

---

## Features

- **Live speeds** — Download/upload rate in the panel, updated once a second by default
- **Quota light** — Green circle: a live reading matches the Jio unlimited rule · red diamond: burning daily quota (blinks) · hollow circle: unknown, offline, or stale
- **Panel styles** — Compact text, Standard with a rolling sparkline, Detailed with today's total
- **Hourly sparkline** — Activity graph for the current day with a live "now" marker
- **Monthly bar chart** — Per-day totals with highest, average, and lowest active day. Click any bar to see its value
- **Connection details** — IPv4, IPv6, gateway, DNS, interface, signal, link speed, and session duration. Copy the interface and address fields
- **Wi-Fi info** — SSID, signal strength, link speed
- **Configurable** — Refresh interval, speed units (bps/bytes), panel preset, interface, data retention (7, 15, 30, 60, or 90 days)
- **Reset** — Clear today's or this month's stats, with confirmation

## Configuration

| Setting | Default | Description |
|---|---|---|
| Refresh | 1 s | How often to poll the network interface |
| Units | bps | Bits per second or bytes per second |
| Panel style | Compact | Compact text, Standard adds a sparkline, Detailed adds today's total |
| Interface | auto | Which interface to monitor |
| Keep data | 30 days | Days to keep, including today: 7, 15, 30, 60, or 90 |
| Quota notifications | On | Notify when the verdict becomes unlimited or burning |

## Quota light (paired phone only)

The applet draws one symbol next to the speeds in the panel. Open the applet for
detail.

| Panel symbol | Color | Means |
|---|---|---|
| `●` | Green | Unlimited data — a live `5G SA n78` reading matches the Jio rule |
| `◆` | Red | Burning your daily quota — the diamond blinks |
| `○` | Amber | Unknown, offline, or stale — see below |

The hero screenshot at the top shows the `○` state.

**The green rule is tuned for Jio.** Jio treats 5G SA on n78 as unlimited, so
that is the only reading that turns the dot green. On another carrier the same
reading may not mean unlimited, and the applet cannot tell. Treat green as
"matches the Jio rule", not as a billing fact.

The blink is on the red diamond only. Green and hollow stay steady, so a
blinking shape always means the same thing. It pulses about every 0.8 seconds
and dims rather than disappearing, so it never reads as a broken applet.

**The hollow circle is normal on some phones.** Green needs `5G`, `SA`, and
`n78` in the same live reading. A reading that leaves the band out, or names a
different band, stays hollow instead of guessing. The applet never shows green
unless a live reading carried all three tokens.

If the phone stops answering — it sleeps, drops off the network, or KDE Connect
quits — the applet keeps the last reading but marks it **stale**. A stale
reading drops to the hollow circle, so a quiet phone cannot leave a false green
dot in your panel. Stale is not the same as live.

Needs [a patched KDE Connect app](https://github.com/AceMythos/kdeconnect-android)
on the phone. Without the patch the reading carries no band: the light stays
hollow on 5G and turns red on LTE, but it can never turn green.

**If the light is wrong:** your carrier decides what to bill. This reads the
radio, not your account.

<details>
<summary>The light follows the radio mode, not the band</summary>

The applet reads three tokens: the generation (`5G` or `LTE`), `SA`/`NSA`, and
the band. Green requires all of `5G`, `SA`, and `n78` in a live reading.

| Phone reports | Verdict |
|---|---|
| `5G SA n78` | Green — matches the Jio rule |
| `LTE B5` | Red — burning |
| `5G SA n28` | Hollow — unknown |
| `5G NSA n78` | Hollow — unknown |

Any reading the applet parses as LTE is red, whatever band it names. A 5G
reading that is not SA on n78 is hollow, including 5G NSA.

So the light flips green ⇄ red when the phone drops from 5G standalone to LTE,
not when the band changes by itself. On the Jio handset observed here, that
fallback happened roughly every 15 seconds and also changed band
(`n78` → `B5`); the band change was a side effect of the mode change, not the
cause of the flip.

With notifications on, the applet announces every transition that lands on
unlimited or burning: unlimited becoming available, and unlimited stopping.
That is deliberate — losing unlimited is what starts eating your daily
allowance. Transitions to unknown or offline are silent, because they are not
actionable and flap with the radio. The first reading after the applet starts
is also silent, since there is no transition to report. Disable notifications
in Settings if you only want the panel light.

If the flapping gets noisy, increase the **refresh interval**. The applet
samples the phone less often and reports fewer transitions. The higher interval
does not stop the radio from switching. It only stops the applet from narrating
every switch.

</details>

## Build from source

Already covered in [Install](#install). For the full build and update workflow,
see [BUILD.md](BUILD.md).

```sh
cargo build --release --locked
```

## Logs

```sh
journalctl -p 3 -xb --user _EXE=/usr/bin/internet-speed-monitor
```

This shows errors only, so no output means the applet has logged no errors.

## Data and privacy

- Stats stay on this machine in
  `$XDG_DATA_HOME/com.github.AceMythos.InternetSpeedMonitor/stats.json`
  (usually `~/.local/share/...`).
- The applet sends no data to the network. To find your local IP address it asks
  the kernel which local address would reach `1.1.1.1:53`; a UDP `connect`
  sends no packets. Phone state is read locally from KDE Connect over D-Bus.
- Delete that directory. This deletes all history.

## Contributing

Bug reports, feature requests, and pull requests are welcome.

## License

[GPL-3.0-or-later](LICENSE)
