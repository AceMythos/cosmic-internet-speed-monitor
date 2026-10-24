use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, Timelike};

use crate::cellular;

use cosmic::app::Core;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::widget::canvas::{Canvas, Frame, Geometry};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Color, Length, Limits, Point, Rectangle, Subscription};
use cosmic::widget::{self, button, settings, text};
use cosmic::{Action, Element, Task};

use crate::backend;
use crate::backend::InterfaceStats;
use crate::config::Config;
use crate::storage;

const APP_ID: &str = "com.github.AceMythos.InternetSpeedMonitor";

/// How long each half of the red quota blink lasts. 800ms is a full cycle of
/// 1.25Hz, comfortably under the WCAG 2.3.1 three-flashes-per-second limit.
const BLINK_PERIOD: Duration = Duration::from_millis(800);

/// Persist at most once per this interval. The UI updates every tick; the
/// stats file does not need to.
const SAVE_INTERVAL: Duration = Duration::from_secs(10);

/// Refresh gateway/DNS/Wi-Fi details while the popup is open.
const NET_INFO_REFRESH: Duration = Duration::from_secs(10);

/// Number of speed samples kept for the panel sparkline.
const PANEL_SAMPLES: usize = 60;

#[derive(Debug, Clone, Default)]
pub(crate) struct NetworkInfo {
    interface: String,
    local_ip: String,
    gateway: String,
    ipv6: String,
    ssid: String,
    signal_dbm: i32,
    link_mbps: u32,
    dns: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetAction {
    Today,
    Month,
}

pub struct AppModel {
    core: Core,
    popup: Option<Id>,
    config: Config,
    config_ctx: Option<cosmic_config::Config>,

    interface: String,
    available_interfaces: Vec<String>,
    rx_speed: f64,
    tx_speed: f64,
    local_ip: String,
    no_interface: bool,

    records: Vec<storage::DailyRecord>,
    today_rx: u64,
    today_tx: u64,
    this_month_rx: u64,
    this_month_tx: u64,
    last_month_rx: u64,
    last_month_tx: u64,

    prev_sample: Option<(HashMap<String, InterfaceStats>, Instant)>,

    monthly_expanded: bool,
    details_expanded: bool,
    settings_expanded: bool,
    settings_iface_opts: Vec<String>,

    last_tick: Instant,

    // Network info
    gateway: String,
    ipv6: String,
    dns_servers: Vec<String>,
    ssid: String,
    signal_level: String,
    link_speed: String,
    connection_duration: String,
    is_wireless: bool,
    session_start: Option<Instant>,
    last_retention_date: Option<NaiveDate>,

    /// Recent total throughput samples for the panel sparkline.
    panel_samples: VecDeque<f64>,

    /// Persistence is debounced and serialized: one write in flight at a time.
    last_save: Instant,
    save_in_flight: bool,
    save_pending: bool,

    /// When network details were last fetched while the popup is open.
    last_net_refresh: Option<Instant>,

    /// Value copied recently, so the UI can acknowledge it.
    copied: Option<(String, Instant)>,

    /// Destructive action waiting for confirmation.
    pending_reset: Option<ResetAction>,

    selected_today_bar: Option<usize>,
    selected_monthly_bar: Option<usize>,

    /// Last cellular state read from the phone via KDE Connect.
    cellular: cellular::Cellular,
    /// When that reading was taken, used to show how stale it is.
    cellular_at: Option<Instant>,
    /// Previous quota verdict, so notifications fire only on a change.
    last_quota: Option<cellular::Quota>,

    /// Blink phase for the red quota dot. Only ever advanced while the quota is
    /// burning, so green and grey stay steady.
    blink_on: bool,
    /// When the blink last flipped, so the phase is driven by elapsed time
    /// rather than by how often the poll tick happens to fire.
    blink_at: Instant,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            config: Config::default(),
            config_ctx: None,
            interface: String::new(),
            available_interfaces: Vec::new(),
            rx_speed: 0.0,
            tx_speed: 0.0,
            local_ip: String::new(),
            no_interface: true,
            records: Vec::new(),
            today_rx: 0,
            today_tx: 0,
            this_month_rx: 0,
            this_month_tx: 0,
            last_month_rx: 0,
            last_month_tx: 0,
            prev_sample: None,
            monthly_expanded: false,
            details_expanded: false,
            settings_expanded: false,
            settings_iface_opts: Vec::new(),
            last_tick: Instant::now(),
            gateway: String::new(),
            ipv6: String::new(),
            dns_servers: Vec::new(),
            ssid: String::new(),
            signal_level: String::new(),
            link_speed: String::new(),
            connection_duration: String::new(),
            is_wireless: false,
            session_start: None,
            last_retention_date: None,
            panel_samples: VecDeque::with_capacity(PANEL_SAMPLES),
            last_save: Instant::now() - SAVE_INTERVAL,
            save_in_flight: false,
            save_pending: false,
            last_net_refresh: None,
            copied: None,
            pending_reset: None,
            selected_today_bar: None,
            selected_monthly_bar: None,
            cellular: cellular::Cellular::default(),
            cellular_at: None,
            last_quota: None,
            blink_on: true,
            blink_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Tick,
    ConfigChanged(Config),
    StatsLoaded(Vec<storage::DailyRecord>),
    ToggleMonthly,
    ToggleDetails,
    SetRefreshInterval(u64),
    SetPanelPreset(usize),
    SetSpeedUnits(usize),
    SetNetworkInterface(usize),
    ToggleSettings,
    SetDataRetentionDays(usize),
    SetNotifications(bool),
    ResetToday,
    ResetMonthly,
    ConfirmReset,
    CancelReset,
    StatsSaved,
    Copy(String),
    NetworkInfoLoaded(NetworkInfo),
    CellularLoaded(Option<cellular::Cellular>),
    TodayBarSelected(Option<usize>),
    MonthlyBarSelected(Option<usize>),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let config_ctx = cosmic_config::Config::new(Self::APP_ID, Config::VERSION).ok();
        let mut cfg = config_ctx
            .as_ref()
            .map(|ctx| match Config::get_entry(ctx) {
                Ok(cfg) => cfg,
                Err((errors, cfg)) => {
                    // `get_entry` returns the successfully read fields plus
                    // defaults for missing ones. Using that value means adding a
                    // config field never resets the user's other settings.
                    for error in errors {
                        eprintln!("speed-monitor: config read: {error}");
                    }
                    cfg
                }
            })
            .unwrap_or_default();

        // Older configs used "auto" for the same behaviour as "bps". The
        // settings dropdown has no "auto" entry, so normalize it on load.
        if cfg.speed_units == "auto" {
            cfg.speed_units = "bps".to_string();
        }

        let app = Self {
            core,
            config: cfg,
            config_ctx,
            ..Default::default()
        };

        let stats_task = Task::perform(async { storage::load_stats().await }, |records| {
            Action::App(Message::StatsLoaded(records))
        });

        (app, stats_task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let mut row: Vec<Element<Message>> = vec![text::monotext(self.format_panel_speed()).into()];

        if matches!(self.config.panel_preset.as_str(), "standard" | "detailed") {
            row.push(self.panel_sparkline());
        }

        if self.config.panel_preset == "detailed" {
            row.push(text::caption(volume_str(self.today_rx + self.today_tx)).into());
        }

        if self.cellular_at.is_some() {
            row.push(self.quota_dot());
        }

        let btn = button::custom(
            widget::row::with_children(row)
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .on_press_down(Message::TogglePopup)
        .padding([4, 8]);

        self.core.applet.autosize_window(btn).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut col: Vec<Element<Message>> = vec![self.live_speed_card()];

        if self.no_interface {
            col.push(card(
                widget::column![
                    text::heading("No network interface"),
                    text::body("No usable interface in /proc/net/dev. Retrying automatically."),
                ]
                .spacing(6)
                .into(),
            ));
        } else {
            col.push(self.today_card());

            col.push(self.expander(
                "This month",
                Some(volume_str(self.this_month_rx + self.this_month_tx)),
                self.monthly_expanded,
                Message::ToggleMonthly,
                monthly_content(
                    &self.records,
                    self.this_month_rx,
                    self.this_month_tx,
                    self.last_month_rx,
                    self.last_month_tx,
                    self.selected_monthly_bar,
                ),
            ));

            if self.cellular_at.is_some() {
                col.push(self.cellular_section());
            }

            col.push(self.expander(
                "Network",
                self.network_summary(),
                self.details_expanded,
                Message::ToggleDetails,
                self.connection_details_content(),
            ));
        }

        col.push(self.expander(
            "Settings",
            None,
            self.settings_expanded,
            Message::ToggleSettings,
            self.settings_view(),
        ));

        let content = widget::column::with_children(col).spacing(8).padding(8);
        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            Subscription::run_with(std::any::TypeId::of::<()>(), |_state| {
                futures_util::stream::unfold(false, |initialized| async move {
                    if initialized {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    Some((Message::Tick, true))
                })
            }),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::ConfigChanged(update.config)),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                return if let Some(popup_id) = self.popup.take() {
                    destroy_popup(popup_id)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);

                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );

                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(400.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);

                    let base = get_popup(popup_settings);

                    let iface = self.interface.clone();
                    let fetch_task = Task::perform(fetch_network_info(iface), |data| {
                        Action::App(Message::NetworkInfoLoaded(data))
                    });
                    self.last_net_refresh = Some(Instant::now());
                    return Task::batch(vec![base, fetch_task]);
                };
            }
            Message::PopupClosed(popup_id) => {
                if self.popup.as_ref() == Some(&popup_id) {
                    self.popup = None;
                    self.selected_today_bar = None;
                    self.selected_monthly_bar = None;
                }
            }
            Message::Tick => {
                let now = Instant::now();
                let mut tasks: Vec<Task<Action<Message>>> = Vec::new();

                if self
                    .copied
                    .as_ref()
                    .is_some_and(|(_, at)| now.duration_since(*at) >= Duration::from_millis(1500))
                {
                    self.copied = None;
                }

                // Advance the blink before the poll-interval early return below.
                // The tick stream fires every 200ms but bails out of the polling
                // work unless the refresh interval has elapsed, so a flip placed
                // after that guard would be throttled to the poll rate and the
                // dot would look frozen at long refresh intervals.
                if self.cellular.quota == cellular::Quota::Burning
                    && now.duration_since(self.blink_at) >= BLINK_PERIOD
                {
                    self.blink_at = now;
                    self.blink_on = !self.blink_on;
                }

                // Keep the popup's address details current while it is open.
                if self.popup.is_some()
                    && self
                        .last_net_refresh
                        .is_none_or(|last| now.duration_since(last) >= NET_INFO_REFRESH)
                {
                    self.last_net_refresh = Some(now);
                    tasks.push(Task::perform(
                        fetch_network_info(self.interface.clone()),
                        |data| Action::App(Message::NetworkInfoLoaded(data)),
                    ));
                }

                let elapsed = now.duration_since(self.last_tick);
                let min_interval = Duration::from_millis(self.config.refresh_interval);

                if elapsed < min_interval && self.prev_sample.is_some() {
                    return Task::batch(tasks);
                }

                // Poll the phone for its cellular state alongside the interface
                // counters. This is a D-Bus round trip, so it runs on its own
                // task rather than blocking the tick.
                tasks.push(Task::perform(cellular::read(), |data| {
                    Action::App(Message::CellularLoaded(data))
                }));

                let curr = match backend::read_proc_net_dev() {
                    Ok(stats) => stats,
                    Err(_) => {
                        self.no_interface = true;
                        self.rx_speed = 0.0;
                        self.tx_speed = 0.0;
                        self.session_start = None;
                        self.connection_duration.clear();
                        self.prev_sample = None;
                        self.last_tick = now;
                        return Task::batch(tasks);
                    }
                };

                let interface = backend::filter_interfaces(&curr, &self.config.network_interface);

                self.available_interfaces = curr.keys().cloned().collect();
                self.available_interfaces.sort();

                let mut opts = vec!["auto".to_string()];
                opts.extend(self.available_interfaces.clone());
                self.settings_iface_opts = opts;

                if !curr.contains_key(&interface) {
                    self.no_interface = true;
                    self.rx_speed = 0.0;
                    self.tx_speed = 0.0;
                    self.session_start = None;
                    self.connection_duration.clear();
                    self.prev_sample = None;
                    self.last_tick = now;
                    return Task::batch(tasks);
                }

                if self.interface != interface {
                    // A new interface is a new session. Keeping the old start
                    // time made the popup report applet uptime as connection
                    // uptime after every switch.
                    self.interface.clone_from(&interface);
                    self.session_start = Some(now);
                    self.prev_sample = None;
                    self.last_net_refresh = None;
                    self.local_ip.clear();
                    self.gateway.clear();
                    self.ipv6.clear();
                    self.ssid.clear();
                    self.signal_level.clear();
                    self.link_speed.clear();
                    self.dns_servers.clear();
                }

                self.no_interface = false;

                if self.session_start.is_none() {
                    self.session_start = Some(now);
                }
                if let Some(start) = self.session_start {
                    let secs = now.duration_since(start).as_secs();
                    self.connection_duration = format!(
                        "{:02}:{:02}:{:02}",
                        secs / 3600,
                        (secs % 3600) / 60,
                        secs % 60
                    );
                }

                if let Some((prev, ts)) = self.prev_sample.as_ref() {
                    let dur = now.duration_since(*ts);
                    if let Some((rx, tx)) = backend::compute_speed(prev, &curr, dur, &interface) {
                        let prev_rx = prev.get(&interface).map(|s| s.rx_bytes).unwrap_or(0);
                        let prev_tx = prev.get(&interface).map(|s| s.tx_bytes).unwrap_or(0);
                        let curr_rx = curr.get(&interface).map(|s| s.rx_bytes).unwrap_or(0);
                        let curr_tx = curr.get(&interface).map(|s| s.tx_bytes).unwrap_or(0);

                        let rx_diff = curr_rx.saturating_sub(prev_rx);
                        let tx_diff = curr_tx.saturating_sub(prev_tx);

                        self.rx_speed = rx;
                        self.tx_speed = tx;

                        self.panel_samples.push_back(rx + tx);
                        if self.panel_samples.len() > PANEL_SAMPLES {
                            self.panel_samples.pop_front();
                        }

                        let records = std::mem::take(&mut self.records);
                        let records = storage::update_today(records, rx_diff, tx_diff);
                        let records = storage::update_hourly(records, rx_diff, tx_diff);
                        let today = Local::now().date_naive();
                        let records = if self.last_retention_date != Some(today) {
                            self.last_retention_date = Some(today);
                            storage::apply_retention(records, self.config.data_retention_days)
                        } else {
                            records
                        };
                        self.records = records;

                        let agg = storage::aggregate(&self.records);
                        self.today_rx = agg.today_rx;
                        self.today_tx = agg.today_tx;
                        self.this_month_rx = agg.this_month_rx;
                        self.this_month_tx = agg.this_month_tx;
                        self.last_month_rx = agg.last_month_rx;
                        self.last_month_tx = agg.last_month_tx;

                        self.prev_sample = Some((curr, now));
                        self.last_tick = now;

                        tasks.push(self.maybe_save(false));
                        return Task::batch(tasks);
                    }
                }

                self.prev_sample = Some((curr, now));
                self.last_tick = now;
                return Task::batch(tasks);
            }
            Message::ConfigChanged(mut config) => {
                if config.speed_units == "auto" {
                    config.speed_units = "bps".to_string();
                }
                self.config = config;
            }
            Message::StatsLoaded(records) => {
                self.records = records;
                self.refresh_aggregates();
            }
            Message::StatsSaved => {
                self.save_in_flight = false;
                if self.save_pending {
                    return self.maybe_save(true);
                }
            }
            Message::ToggleMonthly => {
                self.monthly_expanded = !self.monthly_expanded;
            }
            Message::ToggleDetails => {
                self.details_expanded = !self.details_expanded;
            }
            Message::ToggleSettings => {
                self.settings_expanded = !self.settings_expanded;
            }
            Message::SetRefreshInterval(val) => {
                self.config.refresh_interval = val;
                self.persist_config();
            }
            Message::SetPanelPreset(idx) => {
                let presets = ["compact", "standard", "detailed"];
                if let Some(val) = presets.get(idx) {
                    self.config.panel_preset = val.to_string();
                    self.persist_config();
                }
            }
            Message::SetSpeedUnits(idx) => {
                let opts = ["bps", "bytes"];
                if let Some(val) = opts.get(idx) {
                    self.config.speed_units = val.to_string();
                    self.persist_config();
                }
            }
            Message::SetNetworkInterface(idx) => {
                let Some(val) = self.settings_iface_opts.get(idx).cloned() else {
                    return Task::none();
                };
                if self.config.network_interface == val {
                    return Task::none();
                }
                self.config.network_interface = val.clone();
                self.persist_config();
                self.prev_sample = None;
                self.session_start = None;
                self.connection_duration.clear();
                self.local_ip.clear();
                self.last_net_refresh = Some(Instant::now());
                let iface = if val == "auto" {
                    backend::default_interface().unwrap_or_default()
                } else {
                    val
                };
                return Task::perform(fetch_network_info(iface), |data| {
                    Action::App(Message::NetworkInfoLoaded(data))
                });
            }
            Message::SetDataRetentionDays(idx) => {
                let retention_vals: [u64; 5] = [7, 15, 30, 60, 90];
                if let Some(val) = retention_vals.get(idx) {
                    self.config.data_retention_days = *val;
                    self.persist_config();
                    let records = std::mem::take(&mut self.records);
                    self.records = storage::apply_retention(records, *val);
                    self.last_retention_date = Some(Local::now().date_naive());
                    self.refresh_aggregates();
                    return self.maybe_save(true);
                }
            }
            Message::SetNotifications(enabled) => {
                self.config.notifications_enabled = enabled;
                self.persist_config();
            }
            Message::ResetToday => {
                self.pending_reset = Some(ResetAction::Today);
            }
            Message::ResetMonthly => {
                self.pending_reset = Some(ResetAction::Month);
            }
            Message::CancelReset => {
                self.pending_reset = None;
            }
            Message::ConfirmReset => {
                match self.pending_reset.take() {
                    Some(ResetAction::Today) => {
                        self.records = storage::clear_today(std::mem::take(&mut self.records));
                    }
                    Some(ResetAction::Month) => {
                        self.records = storage::clear_month(std::mem::take(&mut self.records));
                    }
                    None => {}
                }
                self.refresh_aggregates();
                return self.maybe_save(true);
            }
            Message::Copy(value) => {
                self.copied = Some((value.clone(), Instant::now()));
                return cosmic::iced::clipboard::write(value);
            }
            Message::CellularLoaded(data) => {
                match data {
                    Some(c) => {
                        // Only notify on a real transition, so a steady state
                        // does not repeat every refresh.
                        let previous = self.last_quota;
                        self.cellular = c;
                        self.cellular_at = Some(Instant::now());
                        if previous != Some(self.cellular.quota) {
                            self.last_quota = Some(self.cellular.quota);
                            if self.config.notifications_enabled {
                                return cellular::notify_on_change(
                                    previous,
                                    self.cellular.quota,
                                    &self.cellular.label(),
                                );
                            }
                        }
                    }
                    None => {
                        // Phone offline or KDE Connect not running. Keep the
                        // last reading so the panel does not blank, but mark it
                        // stale so it is never presented as live.
                        self.cellular.live = false;
                    }
                }
            }
            Message::NetworkInfoLoaded(data) => {
                // A fetch started before an interface switch must not overwrite
                // the new interface's details.
                if data.interface != self.interface {
                    return Task::none();
                }
                self.local_ip = data.local_ip;
                self.gateway = data.gateway;
                self.ipv6 = data.ipv6;
                self.ssid = data.ssid;
                self.signal_level = if data.signal_dbm != 0 {
                    data.signal_dbm.to_string()
                } else {
                    String::new()
                };
                self.link_speed = if data.link_mbps > 0 {
                    data.link_mbps.to_string()
                } else {
                    String::new()
                };
                self.dns_servers = data.dns;
                self.is_wireless = backend::is_wireless(&self.interface);
            }
            Message::TodayBarSelected(bar) => {
                self.selected_today_bar = bar;
            }
            Message::MonthlyBarSelected(bar) => {
                self.selected_monthly_bar = bar;
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    fn format_panel_speed(&self) -> String {
        let units = &self.config.speed_units;
        let precision = match self.config.panel_preset.as_str() {
            "standard" => Some(1),
            "detailed" => Some(2),
            _ => Some(0),
        };
        let rx = Self::format_compact_speed(self.rx_speed, units, precision);
        let tx = Self::format_compact_speed(self.tx_speed, units, precision);

        format!("↓{} | ↑{}", rx, tx)
    }

    fn format_compact_speed(value_bps: f64, units: &str, precision: Option<usize>) -> String {
        let v = if units == "bytes" {
            value_bps / 8.0
        } else {
            value_bps
        };
        let suf = if units == "bytes" { "B/s" } else { "bps" };

        let (scaled, prefix) = if v >= 1_000_000_000.0 {
            (v / 1_000_000_000.0, "G")
        } else if v >= 1_000_000.0 {
            (v / 1_000_000.0, "M")
        } else if v >= 1_000.0 {
            (v / 1_000.0, "k")
        } else {
            (v, "")
        };

        if let Some(p) = precision {
            if scaled >= 1000.0 {
                format!("{:.0} {}{}", scaled, prefix, suf)
            } else {
                format!("{:.p$} {}{}", scaled, prefix, suf, p = p)
            }
        } else if scaled >= 10.0 {
            format!("{:.0} {}{}", scaled, prefix, suf)
        } else if scaled >= 1.0 {
            let s = format!("{:.1} {}{}", scaled, prefix, suf);
            s.replace(".0 ", " ")
        } else if scaled > 0.0 {
            format!("{:.2} {}{}", scaled, prefix, suf)
        } else {
            format!("0 {suf}")
        }
    }

    fn persist_config(&self) {
        if let Some(ctx) = &self.config_ctx {
            let _ = self.config.write_entry(ctx);
        }
    }

    fn refresh_aggregates(&mut self) {
        let agg = storage::aggregate(&self.records);
        self.today_rx = agg.today_rx;
        self.today_tx = agg.today_tx;
        self.this_month_rx = agg.this_month_rx;
        self.this_month_tx = agg.this_month_tx;
        self.last_month_rx = agg.last_month_rx;
        self.last_month_tx = agg.last_month_tx;
    }

    /// Debounce disk writes and keep at most one write in flight.
    fn maybe_save(&mut self, force: bool) -> Task<Action<Message>> {
        if self.save_in_flight {
            self.save_pending = true;
            return Task::none();
        }
        if !force && self.last_save.elapsed() < SAVE_INTERVAL {
            return Task::none();
        }

        self.save_in_flight = true;
        self.save_pending = false;
        self.last_save = Instant::now();

        let records = self.records.clone();
        Task::perform(async move { storage::save_stats(&records).await }, |_| {
            Action::App(Message::StatsSaved)
        })
    }

    fn live_speed_card(&self) -> Element<'_, Message> {
        let units = &self.config.speed_units;
        let (down, up) = if self.no_interface {
            ("—".to_string(), "—".to_string())
        } else {
            (
                Self::format_compact_speed(self.rx_speed, units, Some(1)),
                Self::format_compact_speed(self.tx_speed, units, Some(1)),
            )
        };

        card(
            widget::row![
                widget::column![text::caption("↓ DOWN"), text::title3(down)].spacing(2),
                widget::Space::new().width(Length::Fill),
                widget::column![text::caption("↑ UP"), text::title3(up)]
                    .spacing(2)
                    .align_x(Alignment::End),
            ]
            .align_y(Alignment::Center)
            .into(),
        )
    }

    fn today_card(&self) -> Element<'_, Message> {
        let total = self.today_rx + self.today_tx;
        let now = Local::now();
        let today_key = format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day());
        let hourly: Vec<f64> = self
            .records
            .iter()
            .find(|r| r.date == today_key)
            .map(|r| {
                (0..24)
                    .map(|h| {
                        r.hourly
                            .iter()
                            .find(|s| s.hour == h as u8)
                            .map(|s| (s.rx_bytes + s.tx_bytes) as f64)
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .unwrap_or_else(|| vec![0.0; 24]);

        let peak = hourly
            .iter()
            .enumerate()
            .filter(|(_, value)| **value > 0.0)
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx);

        let mut content = widget::column![
            widget::row![
                text::heading("Today"),
                widget::Space::new().width(Length::Fill),
                text::title3(volume_str(total)),
            ]
            .align_y(Alignment::Center),
            text::caption(format!(
                "↓ {}   ↑ {}",
                volume_str(self.today_rx),
                volume_str(self.today_tx)
            )),
            today_graph(hourly.clone(), self.selected_today_bar),
        ]
        .spacing(6);

        if let Some(idx) = peak {
            content = content.push(text::caption(format!(
                "Peak {} · {}",
                hour_label(idx),
                volume_str(hourly[idx] as u64)
            )));
        }

        card(content.into())
    }

    fn panel_sparkline(&self) -> Element<'_, Message> {
        let bars: Vec<f64> = self.panel_samples.iter().copied().collect();
        Canvas::<MiniSparkline, Message, cosmic::Theme>::new(MiniSparkline { bars })
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(18.0))
            .into()
    }

    fn quota_dot(&self) -> Element<'_, Message> {
        // A stale reading must never stay green. Green is reserved for a live
        // phone that positively reported unlimited data.
        let quota = if self.cellular.live {
            self.cellular.quota
        } else {
            cellular::Quota::Disconnected
        };
        let blink_on = self.blink_on;
        let glyph = match quota {
            cellular::Quota::Unlimited => "●",
            cellular::Quota::Burning => "◆",
            cellular::Quota::UnknownBand => "○",
            cellular::Quota::Disconnected => "○",
        };

        widget::container(text::body(glyph))
            .class(cosmic::theme::Container::custom(move |theme| {
                let cosmic = theme.cosmic();
                let color: Color = match quota {
                    cellular::Quota::Unlimited => cosmic.success_color().into(),
                    cellular::Quota::Burning if blink_on => cosmic.destructive_color().into(),
                    cellular::Quota::Burning => {
                        let mut color: Color = cosmic.destructive_color().into();
                        color.a = 0.25;
                        color
                    }
                    _ => cosmic.warning_color().into(),
                };
                cosmic::iced::widget::container::Style {
                    text_color: Some(color),
                    ..Default::default()
                }
            }))
            .into()
    }

    fn network_summary(&self) -> Option<String> {
        let conn_type = if self.is_wireless {
            "Wi-Fi"
        } else {
            "Ethernet"
        };
        if self.is_wireless && !self.ssid.is_empty() {
            Some(format!("{conn_type} · {}", self.ssid))
        } else if !self.interface.is_empty() {
            Some(format!("{conn_type} · {}", self.interface))
        } else {
            Some(conn_type.to_string())
        }
    }

    fn expander<'a>(
        &self,
        title: &'a str,
        summary: Option<String>,
        expanded: bool,
        toggle: Message,
        inner: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let icon = widget::icon::from_name(if expanded {
            "go-down-symbolic"
        } else {
            "go-next-symbolic"
        })
        .size(16);

        let mut header_row = widget::row![
            text::heading(title),
            widget::Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center);

        if let Some(summary) = summary {
            header_row = header_row.push(text::caption(summary));
        }
        header_row = header_row.push(icon);

        let header = button::custom(header_row)
            .on_press(toggle)
            .padding(12)
            .width(Length::Fill)
            .class(cosmic::theme::Button::Text);

        let mut content = widget::column![header].spacing(0);
        if expanded {
            content = content.push(inner);
        }

        widget::container(content)
            .class(cosmic::theme::Container::Card)
            .padding([0, 0, 4, 0])
            .into()
    }

    /// Reports what the phone's radio is doing and whether that draws from the
    /// daily allowance. States we cannot resolve are shown as such rather than
    /// being guessed at, because a wrong "unlimited" here costs real quota.
    fn cellular_section(&self) -> Element<'_, Message> {
        let quota = if self.cellular.live {
            self.cellular.quota
        } else {
            cellular::Quota::Disconnected
        };
        let (status, advice) = match quota {
            cellular::Quota::Unlimited => {
                ("Unlimited data", "No daily quota is being used right now")
            }
            cellular::Quota::Burning => (
                "Using daily quota",
                "Data counts against your daily allowance",
            ),
            cellular::Quota::UnknownBand => {
                ("Band unknown", "Cannot confirm whether this is unlimited")
            }
            cellular::Quota::Disconnected => (
                "Phone not connected",
                "Start KDE Connect on your phone to see this",
            ),
        };

        let mut content = widget::column![
            widget::row![self.quota_dot(), text::heading(status)]
                .spacing(6)
                .align_y(Alignment::Center),
            text::body(advice),
            text::caption(format!("Link: {}", self.cellular.label())),
        ]
        .spacing(4);

        // Show how old the reading is, so a stale value is never mistaken for live.
        if let Some(at) = self.cellular_at {
            let age = Instant::now().duration_since(at).as_secs();
            let note = if self.cellular.live {
                format!("Updated {age}s ago")
            } else {
                format!("Stale — phone unreachable, last seen {age}s ago")
            };
            content = content.push(text::caption(note));
        }

        card(content.into())
    }

    fn connection_details_content(&self) -> Element<'_, Message> {
        let conn_type = if self.is_wireless {
            "Wi-Fi"
        } else {
            "Ethernet"
        };
        let network_name = if self.is_wireless && !self.ssid.is_empty() {
            self.ssid.as_str()
        } else {
            self.interface.as_str()
        };

        let signal_str = if self.signal_level.is_empty() || self.signal_level == "0" {
            "—".to_string()
        } else {
            format!("{} dBm", self.signal_level)
        };
        let link_str = if self.link_speed.is_empty() || self.link_speed == "0" {
            "—".to_string()
        } else {
            format!("{} Mbps", self.link_speed)
        };
        let ipv4 = if self.local_ip.is_empty() {
            "—".to_string()
        } else {
            self.local_ip.clone()
        };
        let ipv6 = if self.ipv6.is_empty() {
            "—".to_string()
        } else {
            let parts: Vec<&str> = self.ipv6.splitn(5, ':').collect();
            if parts.len() > 4 {
                format!("{}:\n{}", parts[..4].join(":"), parts[4..].join(":"))
            } else {
                self.ipv6.clone()
            }
        };
        let gateway = if self.gateway.is_empty() {
            "—".to_string()
        } else {
            self.gateway.clone()
        };
        let dns = if self.dns_servers.is_empty() {
            "—".to_string()
        } else {
            self.dns_servers.join(", ")
        };

        let mut rows: Vec<Element<Message>> = vec![
            widget::row![
                text::heading(conn_type),
                widget::Space::new().width(Length::Fill),
                text::body(network_name),
            ]
            .align_y(Alignment::Center)
            .into(),
            self.detail_row(
                "Interface",
                self.interface.clone(),
                Some(self.interface.clone()),
            ),
            self.detail_row("IPv4", ipv4, Some(self.local_ip.clone())),
            self.detail_row("IPv6", ipv6, Some(self.ipv6.clone())),
            self.detail_row("Gateway", gateway, Some(self.gateway.clone())),
            self.detail_row("DNS", dns, Some(self.dns_servers.join(", "))),
        ];

        if self.is_wireless {
            rows.push(self.detail_row("Signal", signal_str, None));
            rows.push(self.detail_row("Link speed", link_str, None));
        }

        rows.push(self.detail_row("Session", self.connection_duration.clone(), None));
        rows.push(text::caption("Session starts when the applet sees a new interface.").into());

        widget::column::with_children(rows)
            .spacing(2)
            .padding([0, 12, 8, 12])
            .into()
    }

    fn detail_row(
        &self,
        label: &'static str,
        value: String,
        copy_value: Option<String>,
    ) -> Element<'_, Message> {
        let mut row = widget::row![
            text::caption(label).width(Length::Fixed(78.0)),
            text::body(value).width(Length::Fill),
        ]
        .align_y(Alignment::Center);

        if let Some(copy) = copy_value.filter(|value| !value.is_empty()) {
            let copied = self
                .copied
                .as_ref()
                .is_some_and(|(value, _)| value == &copy);
            row = row.push(
                button::custom(text::caption(if copied { "copied" } else { "copy" }))
                    .class(cosmic::theme::Button::Text)
                    .on_press(Message::Copy(copy))
                    .padding(0),
            );
        }

        row.padding([3, 0]).into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let presets: &[&str] = &["Compact", "Standard", "Detailed"];
        let preset_vals: &[&str] = &["compact", "standard", "detailed"];
        let unit_opts: &[&str] = &["Bits (bps)", "Bytes (B/s)"];
        let unit_vals: &[&str] = &["bps", "bytes"];
        let retention_opts: &[&str] = &["7 days", "15 days", "30 days", "60 days", "90 days"];
        let retention_vals: &[u64] = &[7, 15, 30, 60, 90];

        let preset_idx = preset_vals
            .iter()
            .position(|&p| p == self.config.panel_preset.as_str());
        let units_idx = unit_vals
            .iter()
            .position(|&u| u == self.config.speed_units.as_str());
        let iface_idx = self
            .settings_iface_opts
            .iter()
            .position(|i| i == &self.config.network_interface);
        let retention_idx = retention_vals
            .iter()
            .position(|&v| v == self.config.data_retention_days);

        // Keep non-standard intervals from an older config selectable instead
        // of snapping the UI to the wrong value.
        let interval = self.config.refresh_interval;
        let mut refresh_vals: Vec<u64> = vec![500, 1000, 2000, 5000, 10000];
        if !refresh_vals.contains(&interval) {
            refresh_vals.push(interval);
            refresh_vals.sort_unstable();
        }
        let refresh_idx = refresh_vals.iter().position(|&v| v == interval);
        let refresh_labels: Vec<String> = refresh_vals
            .iter()
            .map(|&value| {
                if value % 1000 == 0 {
                    format!("{} s", value / 1000)
                } else {
                    format!("{value} ms")
                }
            })
            .collect();

        let reset_controls: Element<Message> = if let Some(action) = self.pending_reset {
            let what = match action {
                ResetAction::Today => "today",
                ResetAction::Month => "this month",
            };
            widget::column![
                text::body(format!("Reset {what}? This cannot be undone.")),
                widget::row![
                    button::custom(text::body("Reset"))
                        .class(cosmic::theme::Button::Destructive)
                        .on_press(Message::ConfirmReset),
                    button::custom(text::body("Cancel")).on_press(Message::CancelReset),
                ]
                .spacing(8),
            ]
            .spacing(6)
            .into()
        } else {
            widget::row![
                button::custom(text::body("Reset today"))
                    .class(cosmic::theme::Button::Destructive)
                    .on_press(Message::ResetToday),
                button::custom(text::body("Reset month"))
                    .class(cosmic::theme::Button::Destructive)
                    .on_press(Message::ResetMonthly),
            ]
            .spacing(8)
            .into()
        };

        widget::column![
            settings::item(
                "Panel style",
                widget::dropdown::dropdown(presets, preset_idx, Message::SetPanelPreset),
            ),
            settings::item(
                "Units",
                widget::dropdown::dropdown(unit_opts, units_idx, Message::SetSpeedUnits),
            ),
            settings::item(
                "Refresh",
                widget::dropdown::dropdown(refresh_labels, refresh_idx, move |idx| {
                    Message::SetRefreshInterval(refresh_vals[idx])
                }),
            ),
            settings::item(
                "Interface",
                widget::dropdown::dropdown(
                    self.settings_iface_opts.as_slice(),
                    iface_idx,
                    Message::SetNetworkInterface,
                ),
            ),
            settings::item(
                "Keep data",
                widget::dropdown::dropdown(retention_opts, retention_idx, |idx| {
                    Message::SetDataRetentionDays(idx)
                }),
            ),
            settings::item(
                "Quota notifications",
                widget::toggler(self.config.notifications_enabled)
                    .on_toggle(Message::SetNotifications),
            ),
            reset_controls,
        ]
        .spacing(8)
        .padding([0, 12, 12, 12])
        .into()
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

fn card<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    widget::container(content)
        .class(cosmic::theme::Container::Card)
        .padding(12)
        .width(Length::Fill)
        .into()
}

async fn fetch_network_info(interface: String) -> NetworkInfo {
    let local_ip = backend::local_ip().unwrap_or_default();
    let gateway = backend::default_gateway().unwrap_or_default();

    let (ipv6, ssid, signal_dbm, link_mbps, dns) = if interface.is_empty() {
        (String::new(), String::new(), 0, 0, Vec::new())
    } else {
        let ipv6 = backend::ipv6_address(&interface).unwrap_or_default();
        let dns = backend::dns_servers(&interface);
        let (ssid, signal_dbm, link_mbps) = if backend::is_wireless(&interface) {
            backend::wifi_info(&interface).unwrap_or_else(|| {
                (
                    String::new(),
                    backend::wifi_signal(&interface).unwrap_or(0),
                    0,
                )
            })
        } else {
            (String::new(), 0, 0)
        };
        (ipv6, ssid, signal_dbm, link_mbps, dns)
    };

    NetworkInfo {
        interface,
        local_ip,
        gateway,
        ipv6,
        ssid,
        signal_dbm,
        link_mbps,
        dns,
    }
}

fn hour_label(idx: usize) -> String {
    match idx {
        0 => "12 AM".to_string(),
        12 => "12 PM".to_string(),
        1..=11 => format!("{idx} AM"),
        _ => format!("{} PM", idx - 12),
    }
}

fn today_graph(hourly: Vec<f64>, selected: Option<usize>) -> Element<'static, Message> {
    let has_data = hourly.iter().any(|&v| v > 0.0);
    let max = hourly.iter().cloned().fold(0.0_f64, f64::max);
    let bars: Vec<f64> = hourly
        .iter()
        .map(|&v| if max > 0.0 { v / max } else { 0.0 })
        .collect();

    let now = Local::now();
    let now_hour = now.hour() as f32 + now.minute() as f32 / 60.0;

    let canvas_widget: Element<'static, Message> = if has_data {
        Canvas::<TodaySparkline, Message, cosmic::Theme>::new(TodaySparkline {
            bars,
            selected,
            now_hour,
        })
        .width(Length::Fill)
        .height(Length::Fixed(40.0))
        .into()
    } else {
        widget::container(text::caption("No traffic recorded yet today"))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    };

    let selected_label = selected.and_then(|idx| {
        hourly
            .get(idx)
            .map(|&value| format!("{} — {}", hour_label(idx), volume_str(value as u64)))
    });

    let mut col = widget::column![canvas_widget]
        .spacing(2)
        .padding([0, 0, 4, 0]);

    if let Some(label) = selected_label {
        col = col.push(
            widget::row![
                widget::Space::new().width(Length::Fill),
                text::body(label).size(10),
                widget::Space::new().width(Length::Fill),
            ]
            .padding([0, 12]),
        );
    }

    col = col.push(
        widget::row![
            text::body("12 AM").size(10),
            widget::Space::new().width(Length::Fill),
            text::body("6 AM").size(10),
            widget::Space::new().width(Length::Fill),
            text::body("12 PM").size(10),
            widget::Space::new().width(Length::Fill),
            text::body("6 PM").size(10),
            widget::Space::new().width(Length::Fill),
            text::body("12 AM").size(10),
        ]
        .padding([0, 12]),
    );

    col.into()
}

fn monthly_content(
    records: &[storage::DailyRecord],
    this_rx: u64,
    this_tx: u64,
    last_rx: u64,
    last_tx: u64,
    selected: Option<usize>,
) -> Element<'static, Message> {
    let now = Local::now();
    let this_year = now.year();
    let this_month = now.month();

    // Calculate days in month
    let next_month = if this_month == 12 {
        NaiveDate::from_ymd_opt(this_year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(this_year, this_month + 1, 1)
    };
    let days_in_month = next_month
        .map(|d| d.pred_opt().unwrap_or(d).day())
        .unwrap_or(30) as usize;

    let mut daily: Vec<u64> = vec![0; days_in_month];
    let mut max_day = 0u64;
    let mut total_bytes = 0u64;

    fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() == 3 {
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            ))
        } else {
            None
        }
    }

    for r in records {
        if let Some((y, m, d)) = parse_ymd(&r.date)
            && y == this_year
            && m == this_month
            && d >= 1
            && (d as usize) <= days_in_month
        {
            let total = r.rx_bytes + r.tx_bytes;
            daily[(d - 1) as usize] = total;
            total_bytes += total;
            if total > max_day {
                max_day = total;
            }
        }
    }

    // Averaging across the whole month understates the rate early in the month.
    // Use days elapsed so far, which is the number the user can act on.
    let elapsed_days = now.day().max(1) as u64;
    let avg = total_bytes / elapsed_days;
    let min_day = daily
        .iter()
        .filter(|&&v| v > 0)
        .cloned()
        .fold(u64::MAX, u64::min);
    let lowest = if min_day == u64::MAX { 0 } else { min_day };

    let monthly_bars: Vec<f64> = if max_day > 0 {
        daily.iter().map(|&v| v as f64 / max_day as f64).collect()
    } else {
        vec![0.0; days_in_month]
    };

    let selected_label = selected.and_then(|idx| {
        daily.get(idx).map(|&v| {
            if v > 0 {
                format!("Day {} — {}", idx + 1, volume_str(v))
            } else {
                format!("Day {} — No data", idx + 1)
            }
        })
    });

    let mut col = widget::column![].spacing(4).padding([0, 0, 8, 0]);

    col = col.push(
        widget::row![text::body(format!(
            "↓ {}    ↑ {}",
            volume_str(this_rx),
            volume_str(this_tx)
        ))]
        .padding([4, 12]),
    );
    col = col.push(widget::row![text::caption("Daily total")].padding([4, 12]));
    col = col.push(
        Canvas::<MonthlySparkline, Message, cosmic::Theme>::new(MonthlySparkline {
            bars: monthly_bars,
            selected,
        })
        .width(Length::Fill)
        .height(Length::Fixed(28.0)),
    );

    if let Some(label) = selected_label {
        col = col.push(
            widget::row![
                widget::Space::new().width(Length::Fill),
                text::body(label).size(10),
                widget::Space::new().width(Length::Fill),
            ]
            .padding([0, 12]),
        );
    }

    col = col.push(
        widget::row![
            text::body("1").size(10),
            widget::Space::new().width(Length::Fill),
            text::body(format!("{}", 1 + days_in_month / 4)).size(10),
            widget::Space::new().width(Length::Fill),
            text::body(format!("{}", 1 + 2 * days_in_month / 4)).size(10),
            widget::Space::new().width(Length::Fill),
            text::body(format!("{}", 1 + 3 * days_in_month / 4)).size(10),
            widget::Space::new().width(Length::Fill),
            text::body(format!("{}", days_in_month)).size(10),
        ]
        .padding([0, 12]),
    );
    col = col.push(
        widget::row![
            text::body("Highest day").width(Length::Fill),
            text::body(if max_day > 0 {
                volume_str(max_day)
            } else {
                "-".to_string()
            })
        ]
        .padding([4, 12]),
    );
    col = col.push(
        widget::row![
            text::body("Avg / day").width(Length::Fill),
            text::body(if avg > 0 {
                volume_str(avg)
            } else {
                "-".to_string()
            })
        ]
        .padding([4, 12]),
    );
    col = col.push(
        widget::row![
            text::body("Lowest active day").width(Length::Fill),
            text::body(if lowest > 0 {
                volume_str(lowest)
            } else {
                "-".to_string()
            })
        ]
        .padding([4, 12]),
    );
    col = col.push(
        widget::container(text::caption(format!(
            "Average uses {elapsed_days} elapsed day(s), not the full month."
        )))
        .padding([0, 12]),
    );

    let last_total = last_rx + last_tx;
    col = col.push(
        widget::container(text::caption(if last_total > 0 {
            format!("Last month: {}", volume_str(last_total))
        } else {
            "Last month: no data".to_string()
        }))
        .padding([0, 12]),
    );
    col.into()
}

fn volume_str(bytes: u64) -> String {
    let value = bytes as f64;
    let (scaled, prefix) = if value >= 1_000_000_000_000.0 {
        (value / 1_000_000_000_000.0, "TB")
    } else if value >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "GB")
    } else if value >= 1_000_000.0 {
        (value / 1_000_000.0, "MB")
    } else if value >= 1_000.0 {
        (value / 1_000.0, "kB")
    } else {
        (value, "B")
    };

    if scaled >= 10.0 {
        format!("{:.0} {}", scaled, prefix)
    } else if scaled >= 1.0 {
        format!("{:.1} {}", scaled, prefix)
    } else {
        format!("{:.2} {}", scaled, prefix)
    }
}

// ── Canvas Program implementations ──────────────────────────────────────────

struct TodaySparkline {
    bars: Vec<f64>,
    selected: Option<usize>,
    now_hour: f32,
}

impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> for TodaySparkline {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &cosmic::iced::widget::canvas::Event,
        bounds: Rectangle,
        cursor: cosmic::iced::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<Message>> {
        if let cosmic::iced::widget::canvas::Event::Mouse(
            cosmic::iced::mouse::Event::ButtonPressed(cosmic::iced::mouse::Button::Left),
        ) = event
        {
            if let Some(pos) = cursor.position_in(bounds) {
                let bar_count = self.bars.len();
                if bar_count > 0 {
                    let bar_width = bounds.width / bar_count as f32;
                    let idx = (pos.x / bar_width) as usize;
                    if idx < bar_count {
                        let gap = 1.0_f32;
                        let draw_w = (bar_width - gap).max(1.0);
                        let bar_x = idx as f32 * bar_width;
                        if pos.x >= bar_x && pos.x < bar_x + draw_w {
                            let new = if self.selected == Some(idx) {
                                None
                            } else {
                                Some(idx)
                            };
                            return Some(cosmic::iced::widget::canvas::Action::publish(
                                Message::TodayBarSelected(new),
                            ));
                        }
                    }
                }
            }
            return Some(cosmic::iced::widget::canvas::Action::publish(
                Message::TodayBarSelected(None),
            ));
        }
        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::iced::Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: cosmic::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;
        let bar_count = self.bars.len();
        if bar_count == 0 {
            return vec![frame.into_geometry()];
        }

        let accent: Color = theme.cosmic().accent_color().into();
        let bar_width = width / bar_count as f32;
        let gap = 1.0_f32;
        let draw_w = (bar_width - gap).max(1.0);

        for (i, &val) in self.bars.iter().enumerate() {
            let bar_h = (val as f32 * (height - 2.0)).max(1.0);
            let x = i as f32 * bar_width;
            let y = height - bar_h;
            let color = if self.selected == Some(i) {
                accent
            } else {
                let mut color = accent;
                color.a = 0.55;
                color
            };
            frame.fill_rectangle(
                Point::new(x, y),
                cosmic::iced::Size::new(draw_w, bar_h),
                color,
            );
        }

        // "Now" marker: the bars are hourly buckets, so the right edge is
        // midnight and the marker shows where the day actually is.
        let now_x = (self.now_hour / 24.0).clamp(0.0, 1.0) * width;
        let mut marker = accent;
        marker.a = 0.9;
        frame.fill_rectangle(
            Point::new(now_x.min((width - 1.0).max(0.0)), 0.0),
            cosmic::iced::Size::new(1.0, height),
            marker,
        );

        vec![frame.into_geometry()]
    }
}

struct MiniSparkline {
    bars: Vec<f64>,
}

impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> for MiniSparkline {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &cosmic::iced::widget::canvas::Event,
        _bounds: Rectangle,
        _cursor: cosmic::iced::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::iced::Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: cosmic::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let n = self.bars.len();
        if n == 0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let max = self.bars.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
        let bar_width = bounds.width / n as f32;
        let mut color: Color = theme.cosmic().accent_color().into();
        color.a = 0.85;

        for (i, &value) in self.bars.iter().enumerate() {
            let bar_height = ((value / max) as f32 * (bounds.height - 1.0)).max(1.0);
            frame.fill_rectangle(
                Point::new(i as f32 * bar_width, bounds.height - bar_height),
                cosmic::iced::Size::new((bar_width - 0.5).max(0.5), bar_height),
                color,
            );
        }

        vec![frame.into_geometry()]
    }
}

struct MonthlySparkline {
    bars: Vec<f64>,
    selected: Option<usize>,
}

impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> for MonthlySparkline {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &cosmic::iced::widget::canvas::Event,
        bounds: Rectangle,
        cursor: cosmic::iced::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<Message>> {
        if let cosmic::iced::widget::canvas::Event::Mouse(
            cosmic::iced::mouse::Event::ButtonPressed(cosmic::iced::mouse::Button::Left),
        ) = event
        {
            if let Some(pos) = cursor.position_in(bounds) {
                let n = self.bars.len();
                if n > 0 {
                    let bw = bounds.width / n as f32;
                    let idx = (pos.x / bw) as usize;
                    if idx < n {
                        let gap = 1.0_f32;
                        let dw = (bw - gap).max(1.0);
                        let bar_x = idx as f32 * bw;
                        if pos.x >= bar_x && pos.x < bar_x + dw {
                            let new = if self.selected == Some(idx) {
                                None
                            } else {
                                Some(idx)
                            };
                            return Some(cosmic::iced::widget::canvas::Action::publish(
                                Message::MonthlyBarSelected(new),
                            ));
                        }
                    }
                }
            }
            return Some(cosmic::iced::widget::canvas::Action::publish(
                Message::MonthlyBarSelected(None),
            ));
        }
        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::iced::Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: cosmic::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let n = self.bars.len();
        if n == 0 {
            return vec![frame.into_geometry()];
        }
        let accent: Color = theme.cosmic().accent_color().into();
        let bw = w / n as f32;
        let gap = 1.0_f32;
        let dw = (bw - gap).max(1.0);
        for (i, &v) in self.bars.iter().enumerate() {
            let bh = (v as f32 * (h - 2.0)).max(1.0);
            let color = if self.selected == Some(i) {
                accent
            } else {
                let mut color = accent;
                color.a = 0.55;
                color
            };
            frame.fill_rectangle(
                Point::new(i as f32 * bw, h - bh),
                cosmic::iced::Size::new(dw, bh),
                color,
            );
        }
        vec![frame.into_geometry()]
    }
}
