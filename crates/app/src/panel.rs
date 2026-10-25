//! The panel window: opens under (or above) the tray icon and hides when it loses focus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewWindow,
    WindowEvent,
};

use crate::balance;

const LABEL: &str = "panel";
const MARGIN: f64 = 8.0;
/// Room kept free above and below the panel (menu bar, taskbar), in logical pixels.
const SCREEN_GAP: f64 = 48.0;
const MIN_HEIGHT: f64 = 200.0;

#[derive(Default)]
pub struct PanelState {
    last_hidden: Mutex<Option<Instant>>,
    /// Whether the panel got focus since it was shown. Window managers may refuse focus
    /// (GNOME's focus-stealing prevention) and still report a blur; only a real blur hides.
    focused: AtomicBool,
    /// The tray icon the panel was last opened from, to keep it anchored while it resizes.
    anchor: Mutex<Option<Rect>>,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

pub fn install(app: &AppHandle) {
    let Some(panel) = window(app) else { return };
    // The panel is resizable in tauri.conf.json because a non-resizable GTK window never gets
    // smaller than its initial size, which would stop it from shrinking to its content. On
    // Linux, with no decorations, there are no resize handles anyway; elsewhere, lock it.
    #[cfg(not(target_os = "linux"))]
    let _ = panel.set_resizable(false);
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
        let _ = app.emit_to(LABEL, "panel-hidden", ());
    }
}

/// Tray (or GNOME Shell button) click; returns whether the panel is open afterwards. Clicking
/// the icon while the panel is open first blurs (and hides) it, so a click right after a hide
/// means "close", not "reopen".
pub fn toggle(app: &AppHandle, anchor: Rect) -> bool {
    let Some(panel) = window(app) else {
        return false;
    };
    if panel.is_visible().unwrap_or(false) {
        hide(app);
        return false;
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
    !just_hidden
}

/// `anchor` is the tray icon's rectangle; Linux trays don't report clicks, so the
/// "Open panel" menu item passes `None` and the panel goes to the top-right corner.
pub fn show(app: &AppHandle, anchor: Option<Rect>) {
    let Some(panel) = window(app) else { return };
    *app.state::<PanelState>().anchor.lock().unwrap() = anchor;
    if let Some(position) = panel
        .outer_size()
        .ok()
        .and_then(|size| position(&panel, anchor, size))
    {
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

/// Fits the panel to its content (logical pixels), up to the screen height; beyond that the
/// page scrolls. A panel opened above a bottom taskbar moves up so it stays attached.
pub fn resize(app: &AppHandle, content_height: f64) {
    let Some(panel) = window(app) else { return };
    let (Ok(scale), Ok(current)) = (panel.scale_factor(), panel.inner_size()) else {
        return;
    };
    let max = panel
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| panel.primary_monitor().ok().flatten())
        .map_or(f64::MAX, |m| {
            f64::from(m.size().height) / scale - 2.0 * SCREEN_GAP
        });
    let height = content_height.ceil().clamp(MIN_HEIGHT, max.max(MIN_HEIGHT));
    let width = f64::from(current.width) / scale;
    if (f64::from(current.height) / scale - height).abs() < 1.0 {
        return;
    }
    let _ = panel.set_size(LogicalSize::new(width, height));

    if panel.is_visible().unwrap_or(false) {
        let anchor = *app.state::<PanelState>().anchor.lock().unwrap();
        let size = PhysicalSize::new(current.width, (height * scale).round() as u32);
        if let Some(position) = position(&panel, anchor, size) {
            let _ = panel.set_position(position);
        }
    }
}

fn position(
    panel: &WebviewWindow,
    anchor: Option<Rect>,
    size: PhysicalSize<u32>,
) -> Option<PhysicalPosition<f64>> {
    let scale = panel.scale_factor().ok()?;
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
