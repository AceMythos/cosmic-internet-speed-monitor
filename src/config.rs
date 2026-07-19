use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 2]
pub struct Config {
    pub refresh_interval: u64,
    pub panel_preset: String,
    pub network_interface: String,
    pub speed_units: String,
    pub data_retention_days: u64,
    pub notifications_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval: 1000,
            panel_preset: "compact".to_string(),
            network_interface: "auto".to_string(),
            speed_units: "auto".to_string(),
            data_retention_days: 30,
            notifications_enabled: false,
        }
    }
}
