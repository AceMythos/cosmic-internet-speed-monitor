use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate};

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
    network_info_loaded: bool,
    connection_start: Option<Instant>,
    last_retention_date: Option<NaiveDate>,
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
            network_info_loaded: false,
            connection_start: None,
            last_retention_date: None,
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
    ResetToday,
    ResetMonthly,
    NetworkInfoLoaded(Vec<String>),
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
        let cfg = config_ctx
            .as_ref()
            .and_then(|ctx| Config::get_entry(ctx).ok())
            .unwrap_or_default();

        let app = Self {
            core,
            config: cfg,
            config_ctx,
            network_info_loaded: false,
            ..Default::default()
        };

        let stats_task = Task::perform(
            async { storage::load_stats().await },
            |records| Action::App(Message::StatsLoaded(records)),
        );

        (app, stats_task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let speed = self.format_panel_speed();
        let content = text::body(speed);

        let btn = button::custom(content)
            .on_press_down(Message::TogglePopup)
            .padding([4, 8]);

        self.core.applet.autosize_window(btn).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut col: Vec<Element<Message>> = Vec::new();

        if self.no_interface {
            col.push(
                widget::container(text::body("No network interface detected"))
                    .padding(16)
                    .into(),
            );
            return self
                .core
                .applet
                .popup_container(widget::column::with_children(col))
                .into();
        }

        // 1. Live Activity Row
        col.push(speed_section(
            self.rx_speed,
            self.tx_speed,
            &self.config.speed_units,
        ));

        // 2. Subtle divider
        col.push(subtle_divider());

        // 3. Today section
        col.push(today_section(self.today_rx, self.today_tx));

        // 4. Today's activity sparkline
        let today_key = format!(
            "{:04}-{:02}-{:02}",
            Local::now().year(),
            Local::now().month(),
            Local::now().day()
        );
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
        col.push(today_graph(hourly));

        // 5. Subtle divider
        col.push(subtle_divider());

        // 6. Monthly Summary
        col.push(self.expander(
            "Monthly Summary",
            self.monthly_expanded,
            Message::ToggleMonthly,
            monthly_content(
                &self.records,
                self.this_month_rx,
                self.this_month_tx,
            ),
        ));
        col.push(subtle_divider());

        // 7. Connection Details
        col.push(self.expander(
            "Connection Details",
            self.details_expanded,
            Message::ToggleDetails,
            self.connection_details_content(),
        ));
        col.push(subtle_divider());

        // 8. Settings
        col.push(self.expander(
            "Settings",
            self.settings_expanded,
            Message::ToggleSettings,
            self.settings_view(),
        ));

        let content = widget::column::with_children(col).spacing(0);
        let content = widget::container(content)
            .style(popup_inner_style)
            .padding(0);
        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            Subscription::run_with(
                std::any::TypeId::of::<()>(),
                |_state| {
                    futures_util::stream::unfold(false, |initialized| async move {
                        if initialized {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                        Some((Message::Tick, true))
                    })
                },
            ),
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

                    if !self.network_info_loaded {
                        let iface = self.interface.clone();
                        let fetch_task = Task::perform(
                            async move {
                                let gw = backend::default_gateway().unwrap_or_default();
                                let ip6 = backend::ipv6_address(&iface).unwrap_or_default();
                                let (ssid, sig, link) =
                                    backend::wifi_info(&iface).unwrap_or_default();
                                let dns = backend::dns_servers().join(",");
                                vec![gw, ip6, ssid, sig.to_string(), link.to_string(), dns]
                            },
                            |data| Action::App(Message::NetworkInfoLoaded(data)),
                        );
                        return Task::batch(vec![base, fetch_task]);
                    }

                    base
                };
            }
            Message::PopupClosed(popup_id) => {
                if self.popup.as_ref() == Some(&popup_id) {
                    self.popup = None;
                }
            }
            Message::Tick => {
                let now = Instant::now();
                let elapsed = now.duration_since(self.last_tick);
                let min_interval = Duration::from_millis(self.config.refresh_interval);

                if elapsed < min_interval && self.prev_sample.is_some() {
                    return Task::none();
                }

                let curr = match backend::read_proc_net_dev() {
                    Ok(s) => s,
                    Err(_) => return Task::none(),
                };

                let interface = backend::filter_interfaces(&curr, &self.config.network_interface);
                self.interface.clone_from(&interface);

                self.available_interfaces = curr.keys().cloned().collect();
                self.available_interfaces.sort();

                let mut opts = vec!["auto".to_string()];
                opts.extend(self.available_interfaces.clone());
                self.settings_iface_opts = opts;

                if !curr.contains_key(&interface) {
                    self.no_interface = true;
                    self.rx_speed = 0.0;
                    self.tx_speed = 0.0;
                    return Task::none();
                }
                self.no_interface = false;

                if self.local_ip.is_empty() {
                    self.local_ip = backend::local_ip().unwrap_or_default();
                }

                // Track connection start
                if self.connection_start.is_none() {
                    self.connection_start = Some(now);
                }
                if let Some(start) = self.connection_start {
                    let elapsed_conn = now.duration_since(start);
                    let secs = elapsed_conn.as_secs();
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

                        let records = self.records.clone();
                        return Task::perform(
                            async move { storage::save_stats(&records).await },
                            |_| Action::None,
                        );
                    }
                }

                self.prev_sample = Some((curr, now));
                self.last_tick = now;
            }
            Message::ConfigChanged(config) => {
                self.config = config;
            }
            Message::StatsLoaded(records) => {
                self.records = records;
                let agg = storage::aggregate(&self.records);
                self.today_rx = agg.today_rx;
                self.today_tx = agg.today_tx;
                self.this_month_rx = agg.this_month_rx;
                self.this_month_tx = agg.this_month_tx;
                self.last_month_rx = agg.last_month_rx;
                self.last_month_tx = agg.last_month_tx;
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
                if let Some(val) = self.settings_iface_opts.get(idx) {
                    self.config.network_interface = val.clone();
                    self.persist_config();
                }
            }
            Message::SetDataRetentionDays(idx) => {
                let retention_vals: [u64; 5] = [7, 15, 30, 60, 90];
                if let Some(val) = retention_vals.get(idx) {
                    self.config.data_retention_days = *val;
                    self.persist_config();
                    let records = std::mem::take(&mut self.records);
                    self.records = storage::apply_retention(records, *val);
                    self.last_retention_date = Some(Local::now().date_naive());
                }
            }
            Message::ResetToday => {
                self.records = storage::clear_today(std::mem::take(&mut self.records));
                let agg = storage::aggregate(&self.records);
                self.today_rx = agg.today_rx;
                self.today_tx = agg.today_tx;
                self.this_month_rx = agg.this_month_rx;
                self.this_month_tx = agg.this_month_tx;
                self.last_month_rx = agg.last_month_rx;
                self.last_month_tx = agg.last_month_tx;
                let records = self.records.clone();
                return Task::perform(
                    async move { storage::save_stats(&records).await },
                    |_| Action::None,
                );
            }
            Message::ResetMonthly => {
                self.records = storage::clear_month(std::mem::take(&mut self.records));
                let agg = storage::aggregate(&self.records);
                self.today_rx = agg.today_rx;
                self.today_tx = agg.today_tx;
                self.this_month_rx = agg.this_month_rx;
                self.this_month_tx = agg.this_month_tx;
                self.last_month_rx = agg.last_month_rx;
                self.last_month_tx = agg.last_month_tx;
                let records = self.records.clone();
                return Task::perform(
                    async move { storage::save_stats(&records).await },
                    |_| Action::None,
                );
            }
            Message::NetworkInfoLoaded(data) => {
                if data.len() >= 6 {
                    self.gateway = data[0].clone();
                    self.ipv6 = data[1].clone();
                    self.ssid = data[2].clone();
                    self.signal_level = data[3].clone();
                    self.link_speed = data[4].clone();
                    self.dns_servers = if data[5].is_empty() {
                        Vec::new()
                    } else {
                        data[5].split(',').map(|s| s.to_string()).collect()
                    };
                }
                self.is_wireless = backend::is_wireless(&self.interface);
                self.network_info_loaded = true;
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

    fn expander<'a>(
        &self,
        title: &'a str,
        expanded: bool,
        toggle: Message,
        inner: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let icon = widget::icon::from_name(if expanded {
            "go-down-symbolic"
        } else {
            "go-next-symbolic"
        });
        let header = button::custom(
            widget::row![icon, text::body(title),]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .on_press(toggle)
        .padding([8, 12])
        .width(Length::Fill)
        .class(cosmic::theme::Button::ListItem([4.0; 4]));

        if expanded {
            widget::column![header, inner].spacing(0).into()
        } else {
            header.into()
        }
    }

    fn connection_details_content(&self) -> Element<'_, Message> {
        let conn_type = if self.is_wireless {
            "Wi-Fi"
        } else {
            "Ethernet"
        };
        let network_name = if self.is_wireless {
            &self.ssid
        } else {
            &self.interface
        };

        let label_w = Length::Fixed(100.0);
        let val_w = Length::Fill;

        let signal = &self.signal_level;
        let signal_str = if signal.is_empty() || signal == "0" {
            "—".to_string()
        } else {
            format!("{} dBm", signal)
        };
        let link = &self.link_speed;
        let link_str = if link.is_empty() || link == "0" {
            "—".to_string()
        } else {
            format!("{} Mbps", link)
        };
        let ipv6_str = if self.ipv6.is_empty() {
            "—".to_string()
        } else {
            let parts: Vec<&str> = self.ipv6.splitn(5, ':').collect();
            if parts.len() > 4 {
                format!("{}:\n{}", parts[..4].join(":"), parts[4..].join(":"))
            } else {
                self.ipv6.clone()
            }
        };

        widget::column![
            widget::row![text::body(conn_type).width(Length::Fill)].padding([4, 12, 0, 12]),
            widget::row![text::body(network_name).width(Length::Fill)].padding([0, 12, 4, 12]),
            widget::row![text::body("Interface").width(label_w), text::body(&self.interface).width(val_w)].padding([4, 12]),
            widget::row![text::body("IPv4").width(label_w), text::body(&self.local_ip).width(val_w)].padding([4, 12]),
            widget::row![text::body("IPv6").width(label_w), text::body(ipv6_str).width(val_w)].padding([4, 12]),
            widget::row![text::body("Gateway").width(label_w), text::body(if self.gateway.is_empty() { "—".to_string() } else { self.gateway.clone() }).width(val_w)].padding([4, 12]),
            widget::row![text::body("DNS").width(label_w), text::body(self.dns_servers.first().map(|s| s.as_str()).unwrap_or("—")).width(val_w)].padding([4, 12]),
            if self.is_wireless {
                widget::row![text::body("Signal").width(label_w), text::body(signal_str).width(val_w)].padding([4, 12])
            } else { widget::row![].into() },
            if self.is_wireless {
                widget::row![text::body("Link Speed").width(label_w), text::body(link_str).width(val_w)].padding([4, 12])
            } else { widget::row![].into() },
            widget::row![text::body("Connected").width(label_w), text::body(&self.connection_duration).width(val_w)].padding([4, 12]),
        ]
        .spacing(0)
        .padding([0, 0, 8, 0])
        .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let presets: &[&str] = &["compact", "standard", "detailed"];
        let unit_opts: &[&str] = &["bps", "bytes"];
        let retention_opts: &[&str] = &["7 days", "15 days", "30 days", "60 days", "90 days"];
        let retention_vals: &[u64] = &[7, 15, 30, 60, 90];

        let interval = self.config.refresh_interval;
        let preset_idx = presets
            .iter()
            .position(|&p| p == self.config.panel_preset.as_str());
        let units_idx = unit_opts
            .iter()
            .position(|&u| u == self.config.speed_units.as_str());
        let iface_idx = self
            .settings_iface_opts
            .iter()
            .position(|i| i == &self.config.network_interface);
        let retention_idx = retention_vals
            .iter()
            .position(|&v| v == self.config.data_retention_days);

        widget::column![
            settings::item("Panel Preset", widget::dropdown::dropdown(presets, preset_idx, Message::SetPanelPreset)),
            settings::item("Speed Units", widget::dropdown::dropdown(unit_opts, units_idx, Message::SetSpeedUnits)),
            settings::item(
                "Refresh Interval (ms)",
                widget::row![
                    button::custom(text::body("-")).on_press(Message::SetRefreshInterval(interval.saturating_sub(100).max(200))),
                    text::body(format!("{} ms", interval)).width(Length::Fixed(70.0)),
                    button::custom(text::body("+")).on_press(Message::SetRefreshInterval((interval + 100).min(5000))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ),
            settings::item("Interface", widget::dropdown::dropdown(self.settings_iface_opts.as_slice(), iface_idx, Message::SetNetworkInterface)),
            settings::item("Data Retention", widget::dropdown::dropdown(retention_opts, retention_idx, |idx| Message::SetDataRetentionDays(idx))),
            widget::row![
                button::custom(text::body("Reset Today")).on_press(Message::ResetToday),
                widget::Space::new().width(Length::Fixed(8.0)),
                button::custom(text::body("Reset Monthly")).on_press(Message::ResetMonthly),
            ]
            .padding([8, 12])
            .spacing(8),
        ]
        .spacing(4)
        .padding([0, 0, 8, 0])
        .into()
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

fn speed_section(rx: f64, tx: f64, units: &str) -> Element<'static, Message> {
    let fmt = |v| AppModel::format_compact_speed(v, units, None);

    widget::container(
        widget::row![
            text::body(format!("↓ {}", fmt(rx))),
            widget::Space::new().width(Length::Fixed(16.0)),
            text::body(format!("↑ {}", fmt(tx))),
        ]
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .into()
}

fn subtle_divider() -> Element<'static, Message> {
    widget::container(
        widget::divider::horizontal::default(),
    )
    .width(Length::Fill)
    .padding([8, 12])
    .into()
}

fn today_section(rx: u64, tx: u64) -> Element<'static, Message> {
    let total = rx + tx;
    widget::container(
        widget::column![
            text::title3("Today"),
            text::title1(volume_str(total)),
            text::body(format!("↓ {}    ↑ {}", volume_str(rx), volume_str(tx))),
        ]
        .spacing(4),
    )
    .padding([8, 12])
    .into()
}

fn today_graph(hourly: Vec<f64>) -> Element<'static, Message> {
    if hourly.iter().all(|&v| v == 0.0) {
        return widget::Space::new()
            .height(Length::Fixed(32.0))
            .width(Length::Fill)
            .into();
    }
    let max = hourly.iter().cloned().fold(0.0_f64, f64::max);
    let bars: Vec<f64> = hourly
        .iter()
        .map(|&v| if max > 0.0 { v / max } else { 0.0 })
        .collect();

    let canvas_widget = Canvas::<TodaySparkline, Message, cosmic::Theme>::new(TodaySparkline { bars })
        .width(Length::Fill)
        .height(Length::Fixed(32.0));

    widget::column![
        canvas_widget,
        widget::row![
            text::body("12A").size(10),
            widget::Space::new().width(Length::FillPortion(6)),
            text::body("6A").size(10),
            widget::Space::new().width(Length::FillPortion(6)),
            text::body("12P").size(10),
            widget::Space::new().width(Length::FillPortion(6)),
            text::body("6P").size(10),
            widget::Space::new().width(Length::FillPortion(5)),
            text::body("Now").size(10),
        ]
        .padding([0, 12, 0, 12]),
    ]
    .spacing(2)
    .padding([0, 0, 4, 0])
    .into()
}

fn monthly_content(
    records: &[storage::DailyRecord],
    this_rx: u64,
    this_tx: u64,
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
        if let Some((y, m, d)) = parse_ymd(&r.date) {
            if y == this_year && m == this_month && d >= 1 && (d as usize) <= days_in_month {
                let total = r.rx_bytes + r.tx_bytes;
                daily[(d - 1) as usize] = total;
                total_bytes += total;
                if total > max_day {
                    max_day = total;
                }
            }
        }
    }

    let avg = if days_in_month > 0 {
        total_bytes / days_in_month as u64
    } else {
        0
    };
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

    widget::column![
        widget::row![text::body("This Month").width(Length::Fill), text::body(volume_str(this_rx + this_tx))].padding([4, 12]),
        widget::row![text::body(format!("↓ {}    ↑ {}", volume_str(this_rx), volume_str(this_tx)))].padding([4, 12]),
        widget::row![text::body("Total Daily Usage").width(Length::Fill),].padding([4, 12]),
        Canvas::<MonthlySparkline, Message, cosmic::Theme>::new(MonthlySparkline { bars: monthly_bars }).width(Length::Fill).height(Length::Fixed(28.0)),
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
        widget::row![text::body("Highest Day").width(Length::Fill), text::body(if max_day > 0 { volume_str(max_day) } else { "-".to_string() })].padding([4, 12]),
        widget::row![text::body("Average / Day").width(Length::Fill), text::body(if avg > 0 { volume_str(avg) } else { "-".to_string() })].padding([4, 12]),
        widget::row![text::body("Lowest Day").width(Length::Fill), text::body(if lowest > 0 { volume_str(lowest) } else { "-".to_string() })].padding([4, 12]),
    ]
    .spacing(4)
    .padding([0, 0, 8, 0])
    .into()
}

fn popup_inner_style(theme: &cosmic::Theme) -> cosmic::widget::container::Style {
    let cosmic = theme.cosmic();
    let is_dark = cosmic.is_dark;
    let text_col = if is_dark {
        Color::from_rgb8(0xF3, 0xF1, 0xEC)
    } else {
        Color::from_rgb8(0x1A, 0x1A, 0x1A)
    };
    let border_col = if is_dark {
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.08)
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0.08)
    };
    let bg: Color = cosmic.background(false).base.into();
    cosmic::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(Color { a: 0.93, ..bg })),
        text_color: Some(text_col),
        border: cosmic::iced::Border {
            radius: 12.0.into(),
            width: 1.0,
            color: border_col,
        },
        ..Default::default()
    }
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
}

impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> for TodaySparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::iced::Renderer,
        _theme: &cosmic::Theme,
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

        let bar_width = width / bar_count as f32;
        let gap = 1.0_f32;
        let draw_w = (bar_width - gap).max(1.0);

        for (i, &val) in self.bars.iter().enumerate() {
            let bar_h = (val as f32 * (height - 2.0)).max(1.0);
            let x = i as f32 * bar_width;
            let y = height - bar_h;
            frame.fill_rectangle(
                Point::new(x, y),
                cosmic::iced::Size::new(draw_w, bar_h),
                Color::from_rgba8(0x7F, 0xAC, 0xE8, 0.6),
            );
        }
        vec![frame.into_geometry()]
    }
}

struct MonthlySparkline {
    bars: Vec<f64>,
}

impl cosmic::iced::widget::canvas::Program<Message, cosmic::Theme> for MonthlySparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::iced::Renderer,
        _theme: &cosmic::Theme,
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
        let bw = w / n as f32;
        let gap = 1.0_f32;
        let dw = (bw - gap).max(1.0);
        for (i, &v) in self.bars.iter().enumerate() {
            let bh = (v as f32 * (h - 2.0)).max(1.0);
            frame.fill_rectangle(
                Point::new(i as f32 * bw, h - bh),
                cosmic::iced::Size::new(dw, bh),
                Color::from_rgba8(0x7F, 0xAC, 0xE8, 0.6),
            );
        }
        vec![frame.into_geometry()]
    }
}
