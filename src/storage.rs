use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

const APP_ID: &str = "com.github.igris.InternetSpeedMonitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRecord {
    pub date: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    version: u32,
    #[serde(alias = "days")]
    records: Vec<DailyRecord>,
}

fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(format!(".local/share/{}", APP_ID))
}

fn data_file() -> PathBuf {
    data_dir().join("stats.json")
}

fn data_file_bak() -> PathBuf {
    data_dir().join("stats.json.bak")
}

fn today_key() -> String {
    let now = Utc::now();
    format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day())
}

fn key_to_date(key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()
}

pub async fn load_stats() -> Vec<DailyRecord> {
    tokio::task::spawn_blocking(load_stats_sync)
        .await
        .unwrap_or_default()
}

fn load_stats_sync() -> Vec<DailyRecord> {
    let path = data_file();

    match try_load_from(&path) {
        Ok(Some(records)) => return records,
        Ok(None) => return Vec::new(),
        Err(e) => eprintln!("speed-monitor: failed to load stats: {e}"),
    }

    let bak = data_file_bak();
    match try_load_from(&bak) {
        Ok(Some(records)) => {
            eprintln!("speed-monitor: restored {} records from backup", records.len());
            return records;
        }
        Ok(None) => {}
        Err(e) => eprintln!("speed-monitor: backup also failed: {e}"),
    }

    Vec::new()
}

fn try_load_from(path: &Path) -> Result<Option<Vec<DailyRecord>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    let state: PersistedState =
        serde_json::from_str(&json).map_err(|e| format!("invalid JSON: {e}"))?;
    Ok(Some(state.records))
}

pub async fn save_stats(records: &[DailyRecord]) {
    let records = records.to_vec();
    let _ = tokio::task::spawn_blocking(move || {
        save_stats_sync(&records);
    })
    .await;
}

fn save_stats_sync(records: &[DailyRecord]) {
    let dir = data_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    let path = data_file();
    let tmp_path = dir.join("stats.json.tmp");
    let bak = data_file_bak();

    if let Ok(Some(_)) = try_load_from(&path) {
        let _ = fs::copy(&path, &bak);
    } else if path.exists() && !bak.exists() {
        let _ = fs::copy(&path, &bak);
    }

    let state = PersistedState {
        version: 1,
        records: records.to_vec(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        if fs::write(&tmp_path, &json).is_ok() {
            let _ = fs::rename(&tmp_path, &path);
        }
    }
}

pub struct StatsAgg {
    pub today_rx: u64,
    pub today_tx: u64,
    pub this_month_rx: u64,
    pub this_month_tx: u64,
    pub last_month_rx: u64,
    pub last_month_tx: u64,
}

pub fn aggregate(records: &[DailyRecord]) -> StatsAgg {
    let mut today_rx = 0u64;
    let mut today_tx = 0u64;
    let mut this_month_rx = 0u64;
    let mut this_month_tx = 0u64;
    let mut last_month_rx = 0u64;
    let mut last_month_tx = 0u64;

    let today = today_key();
    let now = Utc::now();
    let this_month = (now.year(), now.month());

    let last_month = if now.month() == 1 {
        (now.year() - 1, 12)
    } else {
        (now.year(), now.month() - 1)
    };

    for record in records {
        if record.date == today {
            today_rx = record.rx_bytes;
            today_tx = record.tx_bytes;
        }

        if let Some(date) = key_to_date(&record.date) {
            if (date.year(), date.month()) == this_month {
                this_month_rx += record.rx_bytes;
                this_month_tx += record.tx_bytes;
            } else if (date.year(), date.month()) == last_month {
                last_month_rx += record.rx_bytes;
                last_month_tx += record.tx_bytes;
            }
        }
    }

    StatsAgg {
        today_rx,
        today_tx,
        this_month_rx,
        this_month_tx,
        last_month_rx,
        last_month_tx,
    }
}

pub fn update_today(mut records: Vec<DailyRecord>, rx_add: u64, tx_add: u64) -> Vec<DailyRecord> {
    let today = today_key();

    if let Some(record) = records.iter_mut().find(|r| r.date == today) {
        record.rx_bytes = record.rx_bytes.saturating_add(rx_add);
        record.tx_bytes = record.tx_bytes.saturating_add(tx_add);
    } else {
        records.push(DailyRecord {
            date: today,
            rx_bytes: rx_add,
            tx_bytes: tx_add,
        });
    }

    records.retain(|r| {
        key_to_date(&r.date)
            .is_some_and(|d| d >= NaiveDate::from_ymd_opt(2020, 1, 1).unwrap())
    });

    records
}
