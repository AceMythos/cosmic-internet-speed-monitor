use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};

const APP_ID: &str = "com.github.AceMythos.InternetSpeedMonitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRecord {
    pub date: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    #[serde(default)]
    pub hourly: Vec<HourlySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HourlySnapshot {
    pub hour: u8,
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
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join(APP_ID);
    }

    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home).join(".local/share").join(APP_ID);
    }

    std::env::temp_dir().join(APP_ID)
}

fn data_file() -> PathBuf {
    data_dir().join("stats.json")
}

fn data_file_bak() -> PathBuf {
    data_dir().join("stats.json.bak")
}

fn today_key() -> String {
    let now = Local::now();
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
            eprintln!(
                "speed-monitor: restored {} records from backup",
                records.len()
            );
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
    let mut state: PersistedState =
        serde_json::from_str(&json).map_err(|e| format!("invalid JSON: {e}"))?;

    // Backward compatibility: version 1 data doesn't have hourly snapshots
    if state.version == 1 {
        for record in &mut state.records {
            record.hourly = Vec::new();
        }
    }

    Ok(Some(state.records))
}

pub async fn save_stats(records: &[DailyRecord]) {
    let records = records.to_vec();
    let _ = tokio::task::spawn_blocking(move || {
        save_stats_sync(&records);
    })
    .await;
}

const BACKUP_INTERVAL_SECS: u64 = 300;

fn backup_due(path: &Path) -> bool {
    match fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(modified) => modified
            .elapsed()
            .map(|elapsed| elapsed.as_secs() >= BACKUP_INTERVAL_SECS)
            .unwrap_or(true),
        Err(_) => true,
    }
}

fn save_stats_sync(records: &[DailyRecord]) {
    let dir = data_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("speed-monitor: cannot create data dir: {e}");
        return;
    }

    let path = data_file();
    let tmp_path = dir.join("stats.json.tmp");
    let bak = data_file_bak();

    // Only promote a file that parses. A corrupt current file must never
    // overwrite the last known-good backup.
    if backup_due(&bak)
        && matches!(try_load_from(&path), Ok(Some(_)))
        && let Err(e) = fs::copy(&path, &bak)
    {
        eprintln!("speed-monitor: cannot update backup: {e}");
    }

    let state = PersistedState {
        version: 2,
        records: records.to_vec(),
    };

    let json = match serde_json::to_string_pretty(&state) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("speed-monitor: cannot serialize stats: {e}");
            return;
        }
    };

    if let Err(e) = fs::write(&tmp_path, &json) {
        eprintln!("speed-monitor: cannot write stats: {e}");
        return;
    }
    if let Err(e) = fs::rename(&tmp_path, &path) {
        eprintln!("speed-monitor: cannot replace stats: {e}");
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
    let now = Local::now();
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
            hourly: Vec::new(),
        });
    }

    records
}

pub fn update_hourly(mut records: Vec<DailyRecord>, rx_add: u64, tx_add: u64) -> Vec<DailyRecord> {
    let today = today_key();
    let hour = Local::now().hour() as u8;

    if let Some(record) = records.iter_mut().find(|r| r.date == today) {
        if let Some(snapshot) = record.hourly.iter_mut().find(|s| s.hour == hour) {
            snapshot.rx_bytes = snapshot.rx_bytes.saturating_add(rx_add);
            snapshot.tx_bytes = snapshot.tx_bytes.saturating_add(tx_add);
        } else {
            record.hourly.push(HourlySnapshot {
                hour,
                rx_bytes: rx_add,
                tx_bytes: tx_add,
            });
        }
    }
    // If no today's record exists yet, nothing to update hourly for

    records
}

pub fn apply_retention(records: Vec<DailyRecord>, days: u64) -> Vec<DailyRecord> {
    let today = Local::now().date_naive();
    // "30 days" means today plus the previous 29 days. The old cutoff used
    // `today - 30` inclusively, which kept 31 calendar days.
    let keep_days = days.max(1) as i64;
    let cutoff = today - chrono::Duration::days(keep_days - 1);

    records
        .into_iter()
        .filter(|r| key_to_date(&r.date).is_some_and(|d| d >= cutoff))
        .collect()
}

pub fn clear_today(mut records: Vec<DailyRecord>) -> Vec<DailyRecord> {
    let today = today_key();
    records.retain(|r| r.date != today);
    records
}

pub fn clear_month(mut records: Vec<DailyRecord>) -> Vec<DailyRecord> {
    let now = Local::now();
    let target_month = (now.year(), now.month());
    records.retain(|r| key_to_date(&r.date).is_some_and(|d| (d.year(), d.month()) != target_month));
    records
}

#[allow(dead_code)]
pub fn clear_all() -> Vec<DailyRecord> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(date: NaiveDate) -> String {
        date.format("%Y-%m-%d").to_string()
    }

    fn record(date: NaiveDate, rx: u64, tx: u64) -> DailyRecord {
        DailyRecord {
            date: key(date),
            rx_bytes: rx,
            tx_bytes: tx,
            hourly: Vec::new(),
        }
    }

    #[test]
    fn retention_keeps_exactly_requested_days() {
        let today = Local::now().date_naive();
        let records: Vec<_> = (0..35)
            .map(|days_ago| record(today - chrono::Duration::days(days_ago), 1, 1))
            .collect();

        let kept = apply_retention(records, 30);

        assert_eq!(kept.len(), 30, "30 days must mean 30 records, not 31");
        assert!(kept.iter().any(|r| r.date == key(today)));
        assert!(
            !kept
                .iter()
                .any(|r| r.date == key(today - chrono::Duration::days(30)))
        );
    }

    #[test]
    fn update_today_creates_then_accumulates() {
        let today = Local::now().date_naive();
        let records = update_today(Vec::new(), 10, 20);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].date, key(today));
        assert_eq!((records[0].rx_bytes, records[0].tx_bytes), (10, 20));

        let records = update_today(records, 5, 6);
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].rx_bytes, records[0].tx_bytes), (15, 26));
    }

    #[test]
    fn clear_today_leaves_history_alone() {
        let today = Local::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let records = vec![record(today, 1, 1), record(yesterday, 2, 2)];

        let records = clear_today(records);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].date, key(yesterday));
    }

    #[test]
    fn aggregate_separates_this_and_last_month() {
        let today = Local::now().date_naive();
        let last_month = if today.month() == 1 {
            NaiveDate::from_ymd_opt(today.year() - 1, 12, 15).unwrap()
        } else {
            NaiveDate::from_ymd_opt(today.year(), today.month() - 1, 15).unwrap()
        };
        let records = vec![record(today, 100, 50), record(last_month, 7, 3)];

        let agg = aggregate(&records);

        assert_eq!((agg.today_rx, agg.today_tx), (100, 50));
        assert_eq!((agg.this_month_rx, agg.this_month_tx), (100, 50));
        assert_eq!((agg.last_month_rx, agg.last_month_tx), (7, 3));
    }

    #[test]
    fn update_hourly_adds_to_current_hour() {
        let today = Local::now().date_naive();
        let hour = Local::now().hour() as u8;
        let records = vec![record(today, 0, 0)];

        let records = update_hourly(records, 4, 6);

        let snapshot = records[0]
            .hourly
            .iter()
            .find(|s| s.hour == hour)
            .expect("current hour snapshot");
        assert_eq!((snapshot.rx_bytes, snapshot.tx_bytes), (4, 6));
    }
}
