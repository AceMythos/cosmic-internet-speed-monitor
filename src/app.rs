use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::backend::InterfaceStats;

use cosmic::app::Core;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits, Subscription};
use cosmic::iced::widget::pick_list;
use cosmic::widget::{self, button, settings, text};
use cosmic::{Action, Element, Task};

use crate::backend;
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

    last_tick: Instant,
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
            last_tick: Instant::now(),
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
    SetPanelPreset(String),
    SetSpeedUnits(String),
    SetNetworkInterface(String),
    ToggleSettings,
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
        let cfg = config_ctx.as_ref()
            .and_then(|ctx| Config::get_entry(ctx).ok().map(|(c, _)| c))
            .unwrap_or_default();

        let app = Self {
            core,
            config: cfg,
            config_ctx,
            last_tick: Instant::now(),
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
                    .padding(12)
                    .into(),
            );
            return self.core.applet.popup_container(widget::column::with_children(col)).into();
        }

        col.push(speed_section(self.rx_speed, self.tx_speed, &self.config.speed_units));
        col.push(widget::divider::horizontal::default().into());

        col.push(today_section(self.today_rx, self.today_tx));
        col.push(widget::divider::horizontal::default().into());

        col.push(self.expander(
            "Monthly Summary",
            self.monthly_expanded,
            Message::ToggleMonthly,
            monthly_content(self.this_month_rx, self.this_month_tx, self.last_month_rx, self.last_month_tx),
        ));

        col.push(self.expander(
            "Connection Details",
            self.details_expanded,
            Message::ToggleDetails,
            details_content(self.interface.clone(), self.local_ip.clone(), backend::vpn_active()),
        ));

        let content = widget::column::with_children(col);
        let content = widget::container(content)
            .style(popup_inner_style)
            .padding(8);
        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            Subscription::run_with(
                std::any::TypeId::of::<()>(),
                |_state| {
                    futures_util::stream::unfold(
                        false,
                        |initialized| async move {
                            if initialized {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                            }
                            Some((Message::Tick, true))
                        },
                    )
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
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);

                    get_popup(popup_settings)
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

                        let records = storage::update_today(
                            std::mem::take(&mut self.records),
                            rx_diff,
                            tx_diff,
                        );
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
        let rx = Self::format_compact_speed(self.rx_speed, units);
        let tx = Self::format_compact_speed(self.tx_speed, units);

        match self.config.panel_preset.as_str() {
            "standard" => format!("↓{} ↑{}", rx, tx),
            "detailed" => format!("↓{}  ↑{}", rx, tx),
            _ => format!("{} ↓ | {} ↑", rx, tx),
        }
    }

    fn format_compact_speed(value_bps: f64, units: &str) -> String {
        let v = if units == "bytes" { value_bps / 8.0 } else { value_bps };
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

        if scaled >= 10.0 {
            format!("{:.0} {}{}", scaled, prefix, suf)
        } else if scaled >= 1.0 {
            format!("{:.1} {}{}", scaled, prefix, suf)
        } else if scaled > 0.0 {
            format!("{:.2} {}{}", scaled, prefix, suf)
        } else {
            format!("0 {suf}")
        }
    }

    fn expander<'a>(
        &self,
        title: &'a str,
        expanded: bool,
        toggle: Message,
        inner: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let header = button::custom(
            widget::row![
                text::body(if expanded { "▼" } else { "▶" }).size(12),
                text::body(title),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(toggle)
        .padding([8, 12])
        .width(Length::Fill);

        if expanded {
            widget::column![header, inner].spacing(0).into()
        } else {
            header.into()
        }
    }
}

fn speed_section(rx: f64, tx: f64, units: &str) -> Element<'static, Message> {
    let fmt = |v: f64| -> String {
        let bps = v;
        let (scaled, prefix) = if bps >= 1_000_000_000.0 {
            (bps / 1_000_000_000.0, "G")
        } else if bps >= 1_000_000.0 {
            (bps / 1_000_000.0, "M")
        } else if bps >= 1_000.0 {
            (bps / 1_000.0, "k")
        } else {
            (bps, "")
        };
        let suf = if units == "bytes" { "B/s" } else { "bps" };
        if scaled >= 10.0 {
            format!("{:.0} {}{}", scaled, prefix, suf)
        } else if scaled >= 1.0 {
            format!("{:.1} {}{}", scaled, prefix, suf)
        } else if scaled > 0.0 {
            format!("{:.2} {}{}", scaled, prefix, suf)
        } else {
            format!("0 {suf}")
        }
    };

    let rx_v = if units == "bytes" { rx / 8.0 } else { rx };
    let tx_v = if units == "bytes" { tx / 8.0 } else { tx };

    widget::container(
        widget::row![
            text::body(format!("↓ {}", fmt(rx_v))),
            widget::Space::new().width(Length::Fixed(16.0)),
            text::body(format!("↑ {}", fmt(tx_v))),
        ]
        .align_y(Alignment::Center),
    )
    .padding([8, 12, 4, 12])
    .into()
}

fn today_section(rx: u64, tx: u64) -> Element<'static, Message> {
    let total = rx + tx;
    widget::container(
        widget::column![
            text::title3("Today"),
            text::title1(volume_str(total)),
            text::body(format!("↓ {}  ↑ {}", volume_str(rx), volume_str(tx))),
        ]
        .spacing(2),
    )
    .padding([8, 12])
    .into()
}

fn monthly_content(
    this_rx: u64, this_tx: u64,
    last_rx: u64, last_tx: u64,
) -> Element<'static, Message> {
    widget::column![
        widget::row![
            text::body("This Month").width(Length::Fill),
            text::body(volume_str(this_rx + this_tx)),
        ]
        .padding([4, 12]),
        widget::row![
            text::body(format!("↓ {}  ↑ {}", volume_str(this_rx), volume_str(this_tx))),
        ]
        .padding([2, 12]),
        widget::row![
            text::body("Last Month").width(Length::Fill),
            text::body(volume_str(last_rx + last_tx)),
        ]
        .padding([4, 12]),
        widget::row![
            text::body(format!("↓ {}  ↑ {}", volume_str(last_rx), volume_str(last_tx))),
        ]
        .padding([2, 12]),
    ]
    .spacing(4)
    .padding([0, 0, 8, 0])
    .into()
}

fn details_content(iface: String, ip: String, vpn: bool) -> Element<'static, Message> {
    let vpn_text = if vpn { "Connected" } else { "Disconnected" }.to_string();
    widget::column![
        widget::row![
            text::body("Interface").width(Length::Fill),
            text::body(iface),
        ]
        .padding([4, 12]),
        widget::row![
            text::body("Local IP").width(Length::Fill),
            text::body(ip),
        ]
        .padding([4, 12]),
        widget::row![
            text::body("VPN").width(Length::Fill),
            text::body(vpn_text),
        ]
        .padding([4, 12]),
    ]
    .spacing(4)
    .padding([0, 0, 8, 0])
    .into()
}

fn popup_inner_style(theme: &cosmic::Theme) -> cosmic::widget::container::Style {
    let bg = if theme.transparent {
        cosmic::iced::Background::Gradient(cosmic::iced::Gradient::Linear(
            cosmic::iced::gradient::Linear::new(std::f32::consts::PI)
                .add_stop(0.0, cosmic::iced::Color::from_rgba8(0x27, 0x27, 0x27, 0.55))
                .add_stop(1.0, cosmic::iced::Color::from_rgba8(0x10, 0x10, 0x10, 0.75)),
        ))
    } else {
        cosmic::iced::Background::Color(cosmic::iced::Color::from_rgb8(0x27, 0x27, 0x27))
    };
    cosmic::widget::container::Style {
        background: Some(bg),
        text_color: Some(cosmic::iced::Color::from_rgb8(0xF3, 0xF1, 0xEC)),
        border: cosmic::iced::Border {
            radius: 12.0.into(),
            width: 1.0,
            color: cosmic::iced::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.08),
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
        format!("{:.0}{}", scaled, prefix)
    } else if scaled >= 1.0 {
        format!("{:.1}{}", scaled, prefix)
    } else {
        format!("{:.2}{}", scaled, prefix)
    }
}
