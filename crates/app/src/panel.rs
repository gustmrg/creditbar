//! The panel window: opens under (or above) the tray icon and hides when it loses focus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewWindow, WindowEvent};

use crate::balance;

const LABEL: &str = "panel";
const MARGIN: f64 = 8.0;

#[derive(Default)]
pub struct PanelState {
    last_hidden: Mutex<Option<Instant>>,
    /// Whether the panel got focus since it was shown. Window managers may refuse focus
    /// (GNOME's focus-stealing prevention) and still report a blur; only a real blur hides.
    focused: AtomicBool,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

pub fn install(app: &AppHandle) {
    let Some(panel) = window(app) else { return };
    let handle = app.clone();
    panel.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => handle
            .state::<PanelState>()
            .focused
            .store(true, Ordering::Relaxed),
        WindowEvent::Focused(false) => {
            if handle
                .state::<PanelState>()
                .focused
                .swap(false, Ordering::Relaxed)
            {
                hide(&handle);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(&handle);
        }
        _ => {}
    });
}

pub fn hide(app: &AppHandle) {
    if let Some(panel) = window(app).filter(|w| w.is_visible().unwrap_or(false)) {
        let _ = panel.hide();
        *app.state::<PanelState>().last_hidden.lock().unwrap() = Some(Instant::now());
    }
}

/// Tray click. Clicking the icon while the panel is open first blurs (and hides) it, so a
/// click right after a hide means "close", not "reopen".
pub fn toggle(app: &AppHandle, anchor: Rect) {
    let Some(panel) = window(app) else { return };
    if panel.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    let just_hidden = app
        .state::<PanelState>()
        .last_hidden
        .lock()
        .unwrap()
        .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
    if !just_hidden {
        show(app, Some(anchor));
    }
}

/// `anchor` is the tray icon's rectangle; Linux trays don't report clicks, so the
/// "Open panel" menu item passes `None` and the panel goes to the top-right corner.
pub fn show(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = window(app) else { return };
    if let Some(position) = position(&panel, anchor) {
        let _ = panel.set_position(position);
    }
    app.state::<PanelState>()
        .focused
        .store(false, Ordering::Relaxed);
    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = app.emit_to(LABEL, "panel-shown", ());
    balance::refresh_if_stale(app);
}

fn position(panel: &WebviewWindow, anchor: Option<Rect>) -> Option<PhysicalPosition<f64>> {
    let scale = panel.scale_factor().ok()?;
    let size = panel.outer_size().ok()?;
    let (width, height) = (f64::from(size.width), f64::from(size.height));

    let Some(anchor) = anchor else {
        let monitor = panel.primary_monitor().ok()??;
        let (origin, area) = (monitor.position(), monitor.size());
        return Some(PhysicalPosition::new(
            f64::from(origin.x) + f64::from(area.width) - width - MARGIN * scale,
            f64::from(origin.y) + 40.0 * scale,
        ));
    };

    let icon = anchor.position.to_physical::<f64>(scale);
    let icon_size = anchor.size.to_physical::<f64>(scale);
    let monitor = panel
        .monitor_from_point(icon.x, icon.y)
        .ok()
        .flatten()
        .or_else(|| panel.primary_monitor().ok().flatten())?;
    let (origin, area) = (monitor.position(), monitor.size());
    let (left, top) = (f64::from(origin.x), f64::from(origin.y));
    let (right, bottom) = (left + f64::from(area.width), top + f64::from(area.height));

    let x = (icon.x + icon_size.width / 2.0 - width / 2.0)
        .clamp(left + MARGIN, (right - width - MARGIN).max(left + MARGIN));
    // Menu bar at the top (macOS, most Linux panels) or taskbar at the bottom (Windows).
    let y = if icon.y < (top + bottom) / 2.0 {
        icon.y + icon_size.height + MARGIN
    } else {
        icon.y - height - MARGIN
    };
    Some(PhysicalPosition::new(x, y))
}
