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
        let parts: Vec<&str> = line.split(':').collect();
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
