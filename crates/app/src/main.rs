// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod balance;
mod holidays;
mod panel;
mod settings;
mod tray;

use chrono::{NaiveDate, Utc};
use serde::Serialize;
use status_core::pricing::{self, CalendarDay};
use status_core::PricingSnapshot;
use tauri::{AppHandle, Manager, RunEvent, State};

use crate::balance::{BalanceView, Balances};
use crate::holidays::Holidays;

/// Everything the panel renders, fetched when it opens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelState {
    snapshot: PricingSnapshot,
    balances: Vec<BalanceView>,
}

#[tauri::command]
fn get_state(holidays: State<'_, Holidays>, balances: State<'_, Balances>) -> PanelState {
    PanelState {
        snapshot: holidays.snapshot_now(),
        balances: balances.views(),
    }
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

#[tauri::command]
fn hide_panel(app: AppHandle) {
    panel::hide(&app);
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_state,
            month_calendar,
            week_grid,
            refresh_balance,
            save_api_key,
            remove_api_key,
            hide_panel,
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
            app.manage(tray::Tray::build(handle)?);

            panel::install(handle);
            holidays::start(handle);
            balance::start(handle);
            tray::start(handle);

            // Development aid: open the panel at launch.
            if std::env::var_os("DEEPSEEK_STATUS_OPEN_PANEL").is_some() {
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
