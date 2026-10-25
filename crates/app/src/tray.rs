//! Tray icon and menu, updated once a second from the pricing snapshot.
//!
//! The menu carries the current period and the next switch so Linux, whose trays only open a
//! menu, shows the essentials without the panel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use status_core::pricing::{beijing, compact_countdown};
use status_core::{PricePeriod, PricingSnapshot};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::holidays::Holidays;
use crate::i18n::{t, tf};
use crate::{panel, settings};

pub struct Tray {
    icon: TrayIcon,
    status_item: MenuItem<Wry>,
    next_item: MenuItem<Wry>,
    shows_countdown: AtomicBool,
    last: Mutex<Option<Shown>>,
}

/// What the tray currently displays, to touch it only when something changed.
#[derive(PartialEq)]
struct Shown {
    period: PricePeriod,
    next_text: String,
    title: Option<String>,
}

fn icon(period: PricePeriod) -> Image<'static> {
    match period {
        PricePeriod::Peak => tauri::include_image!("icons/tray-peak.png"),
        PricePeriod::OffPeak => tauri::include_image!("icons/tray-offpeak.png"),
    }
}

impl Tray {
    pub fn build(app: &AppHandle) -> tauri::Result<Self> {
        let settings = settings::load(app);
        let status_item = MenuItem::with_id(app, "status", "…", false, None::<&str>)?;
        let next_item = MenuItem::with_id(app, "next", "…", false, None::<&str>)?;
        let open = MenuItem::with_id(app, "open", t("tray.open"), true, None::<&str>)?;
        let countdown = CheckMenuItem::with_id(
            app,
            "countdown",
            t("tray.countdown"),
            true,
            settings.shows_countdown,
            None::<&str>,
        )?;
        let autostart = CheckMenuItem::with_id(
            app,
            "autostart",
            t("tray.autostart"),
            true,
            app.autolaunch().is_enabled().unwrap_or(false),
            None::<&str>,
        )?;
        let quit = MenuItem::with_id(app, "quit", t("tray.quit"), true, None::<&str>)?;
        let menu = Menu::with_items(
            app,
            &[
                &status_item,
                &next_item,
                &PredefinedMenuItem::separator(app)?,
                &open,
                &countdown,
                &autostart,
                &PredefinedMenuItem::separator(app)?,
                &quit,
            ],
        )?;

        let icon = TrayIconBuilder::with_id("main")
            .icon(icon(PricePeriod::Peak))
            .tooltip("DeepSeek Status")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(move |app, event| match event.id().as_ref() {
                "open" => panel::show(app, None),
                "countdown" => {
                    let shows_countdown = countdown.is_checked().unwrap_or(false);
                    app.state::<Tray>()
                        .shows_countdown
                        .store(shows_countdown, Ordering::Relaxed);
                    settings::save(app, &settings::Settings { shows_countdown });
                    update(app);
                }
                "autostart" => {
                    let launcher = app.autolaunch();
                    let wanted = autostart.is_checked().unwrap_or(false);
                    let result = if wanted {
                        launcher.enable()
                    } else {
                        launcher.disable()
                    };
                    if let Err(error) = result {
                        eprintln!("couldn't change launch at login: {error}");
                        let _ = autostart.set_checked(launcher.is_enabled().unwrap_or(false));
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } = event
                {
                    panel::toggle(tray.app_handle(), rect);
                }
            })
            .build(app)?;

        Ok(Tray {
            icon,
            status_item,
            next_item,
            shows_countdown: AtomicBool::new(settings.shows_countdown),
            last: Mutex::new(None),
        })
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut every_second = tokio::time::interval(Duration::from_secs(1));
        loop {
            every_second.tick().await;
            update(&app);
        }
    });
}

fn update(app: &AppHandle) {
    let snapshot = app.state::<Holidays>().snapshot_now();
    let tray = app.state::<Tray>();
    let shown = Shown {
        period: snapshot.period,
        next_text: next_text(&snapshot),
        title: tray
            .shows_countdown
            .load(Ordering::Relaxed)
            .then(|| compact_countdown(snapshot.seconds_until_transition())),
    };

    let mut last = tray.last.lock().unwrap();
    if last.as_ref() == Some(&shown) {
        return;
    }
    let period_changed = last.as_ref().map_or(true, |l| l.period != shown.period);
    if period_changed {
        let _ = tray.icon.set_icon(Some(icon(shown.period)));
        let _ = tray.status_item.set_text(status_text(shown.period));
        let _ = app.emit("snapshot", &snapshot);
    }
    if last
        .as_ref()
        .map_or(true, |l| l.next_text != shown.next_text)
    {
        let _ = tray.next_item.set_text(&shown.next_text);
        // Windows has no tray title: the tooltip carries the next switch instead.
        let _ = tray
            .icon
            .set_tooltip(Some(format!("DeepSeek Status — {}", shown.next_text)));
    }
    if last.as_ref().map_or(true, |l| l.title != shown.title) {
        let _ = tray.icon.set_title(shown.title.as_deref());
    }
    *last = Some(shown);
}

fn status_text(period: PricePeriod) -> String {
    t(match period {
        PricePeriod::Peak => "tray.status.peak",
        PricePeriod::OffPeak => "tray.status.offPeak",
    })
}

/// "Peak at 09:00 Beijing (in 3h 40m)". Minute precision keeps the menu from redrawing every second.
fn next_text(snapshot: &PricingSnapshot) -> String {
    let period = t(match snapshot.next_period {
        PricePeriod::Peak => "period.peakShort",
        PricePeriod::OffPeak => "period.offPeakShort",
    });
    let at = beijing_time(snapshot.next_transition);
    let minutes = (snapshot.seconds_until_transition() + 59) / 60;
    let remaining = match (minutes / (24 * 60), (minutes / 60) % 24, minutes % 60) {
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m:02}m"),
        (d, h, _) => format!("{d}d {h}h"),
    };
    tf(
        "tray.next",
        &[
            ("period", &period),
            ("time", &at),
            ("remaining", &remaining),
        ],
    )
}

fn beijing_time(at: DateTime<Utc>) -> String {
    at.with_timezone(&beijing()).format("%H:%M").to_string()
}
