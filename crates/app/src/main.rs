// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod balance;
mod holidays;
mod i18n;
mod panel;
mod settings;
#[cfg(target_os = "linux")]
mod shell;
mod tray;

use chrono::{NaiveDate, Utc};
use creditbar_core::pricing::{self, CalendarDay};
use creditbar_core::PricingSnapshot;
use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, State};

use crate::balance::{Balances, ProviderView};
use crate::holidays::Holidays;

/// Everything the panel renders, fetched when it opens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelState {
    snapshot: PricingSnapshot,
    providers: Vec<ProviderView>,
    settings: tray::SettingsView,
}

#[tauri::command]
fn get_state(
    app: AppHandle,
    holidays: State<'_, Holidays>,
    balances: State<'_, Balances>,
) -> PanelState {
    PanelState {
        snapshot: holidays.snapshot_now(),
        providers: balances.views(),
        settings: tray::settings_view(&app),
    }
}

/// Changes the preferences given; the others stay as they are.
#[tauri::command]
fn update_settings(
    app: AppHandle,
    shows_deepseek_pricing: Option<bool>,
    shows_countdown: Option<bool>,
    animates_aquarium: Option<bool>,
) {
    tray::change_settings(&app, |settings| {
        if let Some(value) = shows_deepseek_pricing {
            settings.shows_deepseek_pricing = value;
        }
        if let Some(value) = shows_countdown {
            settings.shows_countdown = value;
        }
        if let Some(value) = animates_aquarium {
            settings.animates_aquarium = value;
        }
    });
}

#[tauri::command]
fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<bool, String> {
    tray::set_launch_at_login(&app, enabled)
}

/// Every day of a month. `month` is 1-based.
#[tauri::command]
fn month_calendar(holidays: State<'_, Holidays>, year: i32, month: u32) -> Vec<CalendarDay> {
    holidays.with_schedule(|schedule| pricing::month_days(year, month, schedule))
}

/// Monday to Sunday of the week containing `date` (`YYYY-MM-DD`), or of today in Beijing.
#[tauri::command]
fn week_grid(holidays: State<'_, Holidays>, date: Option<NaiveDate>) -> Vec<CalendarDay> {
    let date = date.unwrap_or_else(|| pricing::beijing_date(Utc::now()));
    holidays.with_schedule(|schedule| pricing::week_days(date, schedule))
}

/// The UI strings for the system language.
#[tauri::command]
fn get_i18n() -> &'static i18n::I18n {
    i18n::get()
}

#[tauri::command]
async fn refresh_balance(app: AppHandle, provider_id: String) {
    balance::refresh(app, provider_id, false).await;
}

#[tauri::command]
async fn save_api_key(app: AppHandle, provider_id: String, key: String) -> Result<(), String> {
    balance::save_key(app, provider_id, key).await
}

#[tauri::command]
async fn remove_api_key(app: AppHandle, provider_id: String) -> Result<(), String> {
    balance::remove_key(app, provider_id).await
}

/// Opens a provider's billing dashboard (`page = "dashboard"`) or API key page (`"keys"`).
/// URLs come from the provider descriptor, never from the panel.
#[tauri::command]
fn open_provider_page(app: AppHandle, provider_id: String, page: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let provider = creditbar_core::providers::by_id(&provider_id).ok_or("unknown provider")?;
    let url = match page.as_str() {
        "dashboard" => provider.info().dashboard_url,
        "keys" => provider.info().keys_url,
        _ => return Err(format!("unknown page {page}")),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

/// The panel reports its content height whenever it changes.
#[tauri::command]
fn resize_panel(app: AppHandle, height: f64) {
    panel::resize(&app, height);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn main() {
    // WebKitGTK's DMA-BUF renderer leaves the window blank on NVIDIA's proprietary driver.
    // Set before any thread starts; a value from the environment wins.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_i18n,
            update_settings,
            set_launch_at_login,
            month_calendar,
            week_grid,
            refresh_balance,
            save_api_key,
            remove_api_key,
            hide_panel,
            open_provider_page,
            resize_panel,
            quit
        ])
        .setup(|app| {
            // Tray-only app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            app.manage(Holidays::load(handle));
            app.manage(Balances::new());
            app.manage(panel::PanelState::default());
            #[cfg(target_os = "linux")]
            app.manage(shell::Shell::default());
            app.manage(tray::Tray::build(handle)?);

            panel::install(handle);
            holidays::start(handle);
            balance::start(handle);
            tray::start(handle);
            #[cfg(target_os = "linux")]
            shell::start(handle);

            // Development aid: open the panel at launch.
            if std::env::var_os("CREDITBAR_OPEN_PANEL").is_some() {
                panel::show(handle, None);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|_, event| {
        // Hiding the panel must not quit; only `app.exit` (Quit) does.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
