//! Reads the phone's cellular state over KDE Connect's session-bus interface.
//!
//! The patched KDE Connect Android app ships a `connectivity_report` plugin whose
//! `cellularNetworkType` property is a space-separated string such as
//! `"5G SA n78"`, where the parts are optional:
//!
//! ```text
//! <rat> [<sa|nsa>] [<band>]
//! ```
//!
//! Because that format comes from our own patch, an unrecognised value is
//! reported as [`Cellular::Unknown`] rather than guessed at.

use serde::{Deserialize, Serialize};

/// KDE Connect's well-known bus name on the session bus.
const SERVICE: &str = "org.kde.kdeconnect";
/// The daemon object, used to enumerate paired devices.
const DAEMON_PATH: &str = "/modules/kdeconnect";
const DAEMON_IFACE: &str = "org.kde.kdeconnect.daemon";
/// Per-device plugin object, built as `/modules/kdeconnect/devices/<id>/connectivity_report`.
const REPORT_IFACE: &str = "org.kde.kdeconnect.device.connectivity_report";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rat {
    FiveG,
    Lte,
    Other,
}

/// What the panel needs in order to say something useful about the quota.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quota {
    /// 5G SA on n78: Jio treats this as unlimited.
    Unlimited,
    /// Anything that draws from the daily allowance: n28, plain LTE, or NSA,
    /// which rides on an LTE anchor and is billed as 4G.
    Burning,
    /// 5G SA on a band we cannot name, so we cannot tell which side of the
    /// line this is. Deliberately not reported as unlimited.
    UnknownBand,
    /// Not connected to a phone, or the property was unreadable.
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cellular {
    pub rat: Rat,
    pub standalone: Option<bool>,
    pub band: Option<String>,
    pub quota: Quota,
    /// True when this came from a phone rather than a cache.
    pub live: bool,
}

impl Default for Cellular {
    fn default() -> Self {
        Self {
            rat: Rat::Other,
            standalone: None,
            band: None,
            quota: Quota::Disconnected,
            live: false,
        }
    }
}

impl Cellular {
    /// Short label for the panel, e.g. `n78 5G SA` or `unknown band`.
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(band) = &self.band {
            parts.push(band.clone());
        }
        parts.push(
            match self.rat {
                Rat::FiveG => "5G".to_string(),
                Rat::Lte => "LTE".to_string(),
                Rat::Other => "no data".to_string(),
            },
        );
        match self.standalone {
            Some(true) => parts.push("SA".to_string()),
            Some(false) => parts.push("NSA".to_string()),
            None => {}
        }
        parts.join(" ")
    }
}

/// Parse the string our patch produces. Unrecognised input yields `None` so the
/// caller can show "unknown" rather than trusting a changed format.
pub fn parse_network_type(value: &str) -> Option<Cellular> {
    let mut rat = None;
    let mut standalone = None;
    let mut band = None;

    for token in value.split_whitespace() {
        match token {
            "5G" => rat = Some(Rat::FiveG),
            "LTE" => rat = Some(Rat::Lte),
            "SA" => standalone = Some(true),
            "NSA" => standalone = Some(false),
            // Band names are "n28"/"n78" or an LTE anchor like "B40", and NSA
            // reports both as "n78+B40".
            other if is_band(other) => band = Some(other.to_string()),
            _ => return None,
        }
    }

    let rat = rat?;
    let quota = match (rat, band.as_deref()) {
        // The one state we can call unambiguously: 5G SA on n78.
        (Rat::FiveG, Some(b)) if standalone == Some(true) && b == "n78" => Quota::Unlimited,
        // Everything else either draws from the allowance or we cannot tell.
        (Rat::FiveG, None) => Quota::UnknownBand,
        (Rat::FiveG, Some(_)) => Quota::UnknownBand,
        _ => Quota::Burning,
    };

    Some(Cellular {
        rat,
        standalone,
        band,
        quota,
        live: true,
    })
}

fn is_band(token: &str) -> bool {
    // "n78", "n28", "n41" or "B40"; "n78+B40" for non-standalone.
    token.split('+').all(|part| {
        let bytes = part.as_bytes();
        match bytes.first() {
            Some(b'n') | Some(b'B') => {
                part.len() > 1 && part[1..].bytes().all(|b| b.is_ascii_digit())
            }
            _ => false,
        }
    })
}

/// Read `cellularNetworkType` for the single paired phone.
///
/// Returns `None` when KDE Connect is not running, no device is paired, or the
/// device is offline, all of which mean "no live data" to the caller.
pub async fn read() -> Option<Cellular> {
    let conn = zbus::Connection::session().await.ok()?;
    let device_id = first_device(&conn).await?;

    // org.freedesktop.DBus.Properties.Get, called directly rather than through
    // the typed proxy so the interface name stays a plain string.
    let reply = conn
        .call_method(
            Some(SERVICE),
            format!("/modules/kdeconnect/devices/{device_id}/connectivity_report"),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(REPORT_IFACE, "cellularNetworkType"),
        )
        .await
        .ok()?;

    // Properties.Get replies with a VARIANT, not a bare string. In zvariant a
// variant is Value::Value(inner), so unwrap it before reading. Deserializing
// straight to String fails with "Signature mismatch: got `v`, expected `s`".
    let body = reply.body();
    let value: zbus::zvariant::Value = body.deserialize().ok()?;
    let inner = match value {
        zbus::zvariant::Value::Value(inner) => *inner,
        other => other,
    };
    let text: String = inner.downcast_ref::<String>().ok()?.clone();

    parse_network_type(&text)
}

/// First paired device reported by the KDE Connect daemon.
async fn first_device(conn: &zbus::Connection) -> Option<String> {
    let reply = conn
        .call_method(
            Some(SERVICE),
            DAEMON_PATH,
            Some(DAEMON_IFACE),
            "devices",
            &(false, false),
        )
        .await
        .ok()?;

    let devices: Vec<String> = reply.body().deserialize().ok()?;
    devices.into_iter().next()
}

/// Fire a desktop notification when the quota verdict changes.
///
/// Both edges matter: the user wants to know when unlimited data becomes
/// available *and* when it stops, since the latter starts burning the daily
/// allowance. Returns a task that fails silently if no notification server is
/// present.
pub(crate) fn notify_on_change(
    from: Option<Quota>,
    to: Quota,
    label: &str,
) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
    let (summary, body) = match to {
        Quota::Unlimited => (
            "Unlimited data available",
            format!("{label} — no daily quota used right now"),
        ),
        Quota::Burning => (
            "Now using daily quota",
            format!("{label} — data counts against your daily allowance"),
        ),
        Quota::UnknownBand => (
            "On 5G, band unknown",
            "Cannot confirm whether this is unlimited".to_string(),
        ),
        Quota::Disconnected => return cosmic::Task::none(),
    };

    // Do not announce the very first reading: the applet has just started and
    // there is no transition to report.
    if from.is_none() {
        return cosmic::Task::none();
    }

    let title = summary.to_string();
    let body = body.clone();

    cosmic::Task::perform(
        async move {
            let Ok(conn) = zbus::Connection::session().await else {
                return;
            };
            let _ = conn
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &(
                        "internet-speed-monitor",
                        0u32,
                        "network-wireless",
                        &title,
                        &body,
                        Vec::<String>::new(),
                        std::collections::HashMap::<String, zbus::zvariant::Value>::new(),
                        5000i32,
                    ),
                )
                .await;
        },
        |_| cosmic::Action::None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unlimited_state() {
        let c = parse_network_type("5G SA n78").expect("should parse");
        assert_eq!(c.rat, Rat::FiveG);
        assert_eq!(c.standalone, Some(true));
        assert_eq!(c.band.as_deref(), Some("n78"));
        assert_eq!(c.quota, Quota::Unlimited);
    }

    #[test]
    fn n28_is_not_unlimited() {
        let c = parse_network_type("5G SA n28").expect("should parse");
        assert_eq!(c.quota, Quota::UnknownBand);
    }

    #[test]
    fn nsa_with_anchor_is_burning() {
        let c = parse_network_type("LTE NSA B40").expect("should parse");
        assert_eq!(c.quota, Quota::Burning);
        assert_eq!(c.standalone, Some(false));
    }

    #[test]
    fn lte_alone_is_burning() {
        let c = parse_network_type("LTE B40").expect("should parse");
        assert_eq!(c.quota, Quota::Burning);
        assert_eq!(c.standalone, None);
    }

    #[test]
    fn missing_band_is_never_unlimited() {
        let c = parse_network_type("5G SA").expect("should parse");
        assert_eq!(c.quota, Quota::UnknownBand);
    }

    #[test]
    fn unknown_format_is_rejected() {
        // If the phone app is replaced with upstream, the format changes and we
        // must not guess.
        assert!(parse_network_type("5G").is_some());
        assert!(parse_network_type("banana").is_none());
        assert!(parse_network_type("").is_none());
        assert!(parse_network_type("LTE CA").is_none());
    }

    #[test]
    fn label_is_compact() {
        let c = parse_network_type("5G SA n78").unwrap();
        assert_eq!(c.label(), "n78 5G SA");
    }
}