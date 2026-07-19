use std::collections::HashMap;
use std::fs;
use std::net::UdpSocket;

#[derive(Debug, Clone)]
pub struct InterfaceStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

pub fn read_proc_net_dev() -> Result<HashMap<String, InterfaceStats>, String> {
    let content = fs::read_to_string("/proc/net/dev")
        .map_err(|e| format!("Failed to read /proc/net/dev: {e}"))?;

    let mut interfaces = HashMap::new();

    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[0].trim().to_string();
        let values: Vec<u64> = parts[1]
            .split_whitespace()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();

        if values.len() >= 10 {
            interfaces.insert(
                name,
                InterfaceStats {
                    rx_bytes: values[0],
                    tx_bytes: values[8],
                },
            );
        }
    }

    Ok(interfaces)
}

pub fn default_interface() -> Option<String> {
    let content = fs::read_to_string("/proc/net/route").ok()?;
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "00000000" {
            return Some(parts[0].to_string());
        }
    }
    None
}

pub fn local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:53").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[allow(dead_code)]
pub fn vpn_active() -> bool {
    if let Ok(interfaces) = read_proc_net_dev() {
        interfaces.keys().any(|name| {
            name.starts_with("tun")
                || name.starts_with("tap")
                || name.starts_with("wg")
                || name.contains("vpn")
        })
    } else {
        false
    }
}

pub fn compute_speed(
    prev: &HashMap<String, InterfaceStats>,
    curr: &HashMap<String, InterfaceStats>,
    elapsed: std::time::Duration,
    interface: &str,
) -> Option<(f64, f64)> {
    let prev_stats = prev.get(interface)?;
    let curr_stats = curr.get(interface)?;

    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= 0.0 {
        return None;
    }

    let rx_speed = (curr_stats.rx_bytes.saturating_sub(prev_stats.rx_bytes)) as f64 * 8.0
        / elapsed_secs;
    let tx_speed = (curr_stats.tx_bytes.saturating_sub(prev_stats.tx_bytes)) as f64 * 8.0
        / elapsed_secs;

    Some((rx_speed, tx_speed))
}

pub fn filter_interfaces(stats: &HashMap<String, InterfaceStats>, preferred: &str) -> String {
    if preferred != "auto" && stats.contains_key(preferred) {
        return preferred.to_string();
    }

    if let Some(def) = default_interface() {
        if stats.contains_key(&def) {
            return def;
        }
    }

    for name in ["wlan0", "wlp3s0", "eth0", "enp0s3", "enp2s0"] {
        if stats.contains_key(name) {
            return name.to_string();
        }
    }

    stats.keys().next().cloned().unwrap_or_default()
}


/// Parses `/proc/net/route` and returns the gateway IP in dotted-decimal format
/// for the default route (destination `00000000`).
pub fn default_gateway() -> Option<String> {
    let content = fs::read_to_string("/proc/net/route").ok()?;
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "00000000" {
            let val = u32::from_str_radix(parts[2], 16).ok()?;
            let [b0, b1, b2, b3] = val.to_le_bytes();
            return Some(format!("{}.{}.{}.{}", b0, b1, b2, b3));
        }
    }
    None
}

/// Reads `/etc/resolv.conf` and returns all configured DNS server IP addresses.
pub fn dns_servers() -> Vec<String> {
    let content = fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("nameserver") {
                trimmed.split_whitespace().nth(1).map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Retrieves Wi-Fi information for a given interface by running `iw dev {interface} link`.
/// Returns `(SSID, signal_dbm, bitrate_mbps)` on success, or `None` on failure.
pub fn wifi_info(interface: &str) -> Option<(String, i32, u32)> {
    let output = std::process::Command::new("iw")
        .args(["dev", interface, "link"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;

    let mut ssid = None;
    let mut signal = None;
    let mut bitrate = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("SSID:") {
            ssid = Some(val.trim().to_string());
        } else if let Some(val) = trimmed.strip_prefix("signal:") {
            if let Some(dbm_str) = val.trim().split_whitespace().next() {
                signal = dbm_str.parse::<i32>().ok();
            }
        } else if let Some(val) = trimmed.strip_prefix("tx bitrate:") {
            if let Some(rate_str) = val.trim().split_whitespace().next() {
                if let Ok(rate) = rate_str.parse::<f64>() {
                    bitrate = Some(rate as u32);
                }
            }
        }
    }

    Some((ssid?, signal?, bitrate?))
}

/// Reads `/proc/net/if_inet6` and returns the IPv6 address for the given interface
/// in standard colon-separated notation.
pub fn ipv6_address(interface: &str) -> Option<String> {
    let content = fs::read_to_string("/proc/net/if_inet6").ok()?;
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts.last() == Some(&interface) {
            let hex = parts[0];
            if hex.len() != 32 {
                continue;
            }
            let groups: Vec<String> = hex
                .as_bytes()
                .chunks(4)
                .map(|chunk| {
                    let s = std::str::from_utf8(chunk).unwrap_or("0000");
                    s.trim_start_matches('0')
                })
                .map(|s| if s.is_empty() { "0".to_string() } else { s.to_string() })
                .collect();
            return Some(groups.join(":"));
        }
    }
    None
}

/// Parses `/proc/net/wireless` and returns the signal level in dBm for the
/// given interface, or `None` if unavailable.
pub fn wifi_signal(interface: &str) -> Option<i32> {
    let content = fs::read_to_string("/proc/net/wireless").ok()?;
    for line in content.lines().skip(2) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&format!("{}:", interface)) {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() >= 3 {
                // fields[1] is signal level, e.g. "-50."
                let sig_str = fields[1].trim_end_matches('.');
                return sig_str.parse::<i32>().ok();
            }
        }
    }
    None
}

/// Checks whether the given network interface is wireless by testing if
/// `/sys/class/net/{interface}/wireless` exists as a directory.
pub fn is_wireless(interface: &str) -> bool {
    let path = format!("/sys/class/net/{}/wireless", interface);
    std::path::Path::new(&path).is_dir()
}
