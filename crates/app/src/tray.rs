//! Tray icon and menu, updated once a second.
//!
//! The icon turns amber when a balance needs attention. With DeepSeek pricing shown, the menu
//! also carries the current period and the next switch, so Linux, whose trays only open a menu,
//! shows them without the panel.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use creditbar_core::pricing::{beijing, compact_countdown};
use creditbar_core::{PricePeriod, PricingSnapshot};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::balance::Balances;
use crate::holidays::Holidays;
use crate::i18n::{t, tf};
use crate::panel;
use crate::settings::{self, Settings};

pub struct Tray {
    icon: TrayIcon,
    menu: Menu<Wry>,
    /// Menu entries that only exist while DeepSeek pricing is shown.
    pricing: PricingItems,
    settings: Mutex<Settings>,
    last: Mutex<Option<Shown>>,
}

struct PricingItems {
    status: MenuItem<Wry>,
    next: MenuItem<Wry>,
    separator: PredefinedMenuItem<Wry>,
    countdown: CheckMenuItem<Wry>,
}

/// What the tray currently displays, to touch it only when something changed.
#[derive(PartialEq)]
struct Shown {
    attention: bool,
    /// `None` while DeepSeek pricing is hidden.
    pricing: Option<ShownPricing>,
}

#[derive(PartialEq)]
struct ShownPricing {
    period: PricePeriod,
    next_text: String,
    countdown: Option<String>,
}

fn icon(attention: bool) -> Image<'static> {
    if attention {
        tauri::include_image!("icons/tray-attention.png")
    } else {
        tauri::include_image!("icons/tray.png")
    }
}

/// Position of "Show Countdown in Tray": after "Open Panel" and the pricing toggle.
const COUNTDOWN_POSITION: usize = 5;

impl Tray {
    pub fn build(app: &AppHandle) -> tauri::Result<Self> {
        let settings = settings::load(app);
        let pricing = PricingItems {
            status: MenuItem::with_id(app, "status", "…", false, None::<&str>)?,
            next: MenuItem::with_id(app, "next", "…", false, None::<&str>)?,
            separator: PredefinedMenuItem::separator(app)?,
            countdown: CheckMenuItem::with_id(
                app,
                "countdown",
                t("tray.countdown"),
                true,
                settings.shows_countdown,
                None::<&str>,
            )?,
        };
        let open = MenuItem::with_id(app, "open", t("tray.open"), true, None::<&str>)?;
        let pricing_toggle = CheckMenuItem::with_id(
            app,
            "pricing",
            t("tray.deepseekPricing"),
            true,
            settings.shows_deepseek_pricing,
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
                &open,
                &pricing_toggle,
                &autostart,
                &PredefinedMenuItem::separator(app)?,
                &quit,
            ],
        )?;

        let icon = TrayIconBuilder::with_id("main")
            .icon(icon(false))
            .icon_as_template(true)
            .tooltip("CreditBar")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(move |app, event| match event.id().as_ref() {
                "open" => panel::show(app, None),
                "pricing" => {
                    let shown = pricing_toggle.is_checked().unwrap_or(true);
                    change_settings(app, |s| s.shows_deepseek_pricing = shown);
                }
                "countdown" => {
                    let tray = app.state::<Tray>();
                    let shown = tray.pricing.countdown.is_checked().unwrap_or(false);
                    change_settings(app, |s| s.shows_countdown = shown);
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

        let tray = Tray {
            icon,
            menu,
            pricing,
            settings: Mutex::new(settings.clone()),
            last: Mutex::new(None),
        };
        tray.show_pricing_items(settings.shows_deepseek_pricing);
        Ok(tray)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Adds or removes the pricing entries (a menu item can't be hidden, only removed).
    fn show_pricing_items(&self, shown: bool) {
        let items = &self.pricing;
        // Removing an entry that isn't in the menu fails harmlessly.
        let _ = self.menu.remove(&items.status);
        let _ = self.menu.remove(&items.next);
        let _ = self.menu.remove(&items.separator);
        let _ = self.menu.remove(&items.countdown);
        if shown {
            let _ = self.menu.insert(&items.status, 0);
            let _ = self.menu.insert(&items.next, 1);
            let _ = self.menu.insert(&items.separator, 2);
            let _ = self.menu.insert(&items.countdown, COUNTDOWN_POSITION);
        }
    }
}

fn change_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let tray = app.state::<Tray>();
    let settings = {
        let mut settings = tray.settings.lock().unwrap();
        let before = settings.shows_deepseek_pricing;
        change(&mut settings);
        if settings.shows_deepseek_pricing != before {
            tray.show_pricing_items(settings.shows_deepseek_pricing);
        }
        settings.clone()
    };
    settings::save(app, &settings);
    let _ = app.emit("settings", &settings);
    // Menu entries were re-inserted with stale text; force a full redraw.
    *tray.last.lock().unwrap() = None;
    update(app);
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
    let tray = app.state::<Tray>();
    let settings = tray.settings();
    let snapshot = app.state::<Holidays>().snapshot_now();
    let shown = Shown {
        attention: app.state::<Balances>().needs_attention(),
        pricing: settings.shows_deepseek_pricing.then(|| ShownPricing {
            period: snapshot.period,
            next_text: next_text(&snapshot),
            countdown: settings
                .shows_countdown
                .then(|| compact_countdown(snapshot.seconds_until_transition())),
        }),
    };

    let mut last = tray.last.lock().unwrap();
    if last.as_ref() == Some(&shown) {
        return;
    }
    let previous = last.take();
    let changed = |f: &dyn Fn(&Shown) -> bool| previous.as_ref().map_or(true, f);

    if changed(&|l| l.attention != shown.attention) {
        let _ = tray.icon.set_icon(Some(icon(shown.attention)));
        // The amber mark must keep its colour; the plain bar follows the menu bar (macOS).
        let _ = tray.icon.set_icon_as_template(!shown.attention);
    }

    let period = shown.pricing.as_ref().map(|p| p.period);
    if changed(&|l| l.pricing.as_ref().map(|p| p.period) != period) {
        if let Some(period) = period {
            let _ = tray.pricing.status.set_text(status_text(period));
        }
        let _ = app.emit("snapshot", &snapshot);
    }

    let next_text = shown.pricing.as_ref().map(|p| p.next_text.clone());
    if changed(&|l| l.pricing.as_ref().map(|p| p.next_text.clone()) != next_text) {
        if let Some(text) = &next_text {
            let _ = tray.pricing.next.set_text(text);
        }
        // Windows has no tray title: the tooltip carries the next switch instead.
        let tooltip = match &next_text {
            Some(text) => format!("CreditBar — {text}"),
            None => "CreditBar".to_owned(),
        };
        let _ = tray.icon.set_tooltip(Some(tooltip));
    }

    let countdown = shown.pricing.as_ref().and_then(|p| p.countdown.clone());
    if changed(&|l| l.pricing.as_ref().and_then(|p| p.countdown.clone()) != countdown) {
        let _ = tray.icon.set_title(countdown.as_deref());
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
