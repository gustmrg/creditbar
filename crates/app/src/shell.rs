//! The bridge to CreditBar's GNOME Shell extension (`gnome-shell-extension/`), Linux only.
//!
//! GNOME's AppIndicator extension opens a tray icon's menu on a plain click, so no tray icon can
//! open the panel with one click. The companion extension draws CreditBar's own top-bar button
//! instead and talks to the app over the session bus:
//!
//! - The app owns `dev.gustmrg.CreditBar` and serves `dev.gustmrg.CreditBar1` at
//!   `/dev/gustmrg/CreditBar`: `TogglePanel(x, y, width, height) → open`, plus the `Attention`
//!   and `Label` properties the button mirrors (amber icon, countdown).
//! - The extension owns `dev.gustmrg.CreditBar.ShellIndicator` while it is enabled. While that
//!   name has an owner, the app hides its tray icon; when it goes away (extension disabled, Shell
//!   restarted), the tray icon comes back.

use std::sync::Mutex;

use futures_util::StreamExt;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Rect};
use zbus::fdo::DBusProxy;
use zbus::{connection, interface, Connection};

use crate::panel;
use crate::tray::Tray;

const BUS_NAME: &str = "dev.gustmrg.CreditBar";
const OBJECT_PATH: &str = "/dev/gustmrg/CreditBar";
const EXTENSION_NAME: &str = "dev.gustmrg.CreditBar.ShellIndicator";

/// What the Shell button shows.
#[derive(Debug, Clone, Default, PartialEq)]
struct Indicator {
    attention: bool,
    label: String,
}

#[derive(Default)]
pub struct Shell {
    connection: Mutex<Option<Connection>>,
    indicator: Mutex<Indicator>,
}

struct Bridge {
    app: AppHandle,
}

#[interface(name = "dev.gustmrg.CreditBar1")]
impl Bridge {
    /// Opens or closes the panel under the button at the given rectangle, in physical screen
    /// pixels. Returns whether the panel is open afterwards.
    fn toggle_panel(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let anchor = Rect {
            position: PhysicalPosition::new(x, y).into(),
            size: PhysicalSize::new(width.max(0) as u32, height.max(0) as u32).into(),
        };
        panel::toggle(&self.app, anchor)
    }

    /// A provider needs attention: the button turns amber.
    #[zbus(property)]
    fn attention(&self) -> bool {
        self.app
            .state::<Shell>()
            .indicator
            .lock()
            .unwrap()
            .attention
    }

    /// Text next to the button (the DeepSeek countdown), or empty.
    #[zbus(property)]
    fn label(&self) -> String {
        self.app
            .state::<Shell>()
            .indicator
            .lock()
            .unwrap()
            .label
            .clone()
    }
}

/// Serves the bridge and follows the extension's presence. Without a session bus, or with the
/// name taken by another instance, the app simply keeps its tray icon.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = serve(&app).await {
            eprintln!("GNOME Shell bridge unavailable: {error}");
        }
    });
}

async fn serve(app: &AppHandle) -> zbus::Result<()> {
    let connection = connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Bridge { app: app.clone() })?
        .build()
        .await?;
    *app.state::<Shell>().connection.lock().unwrap() = Some(connection.clone());

    // Subscribe before asking, so a change in between isn't missed.
    let dbus = DBusProxy::new(&connection).await?;
    let mut changes = dbus
        .receive_name_owner_changed_with_args(&[(0, EXTENSION_NAME)])
        .await?;
    let present = dbus.name_has_owner(EXTENSION_NAME.try_into()?).await?;
    app.state::<Tray>().set_visible(!present);

    while let Some(change) = changes.next().await {
        let present = change.args()?.new_owner().is_some();
        app.state::<Tray>().set_visible(!present);
    }
    Ok(())
}

/// Publishes what the button should show; only changes reach the bus.
pub fn publish(app: &AppHandle, attention: bool, label: String) {
    let shell = app.state::<Shell>();
    let next = Indicator { attention, label };
    let previous = {
        let mut current = shell.indicator.lock().unwrap();
        if *current == next {
            return;
        }
        std::mem::replace(&mut *current, next.clone())
    };
    let Some(connection) = shell.connection.lock().unwrap().clone() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let Ok(bridge) = connection
            .object_server()
            .interface::<_, Bridge>(OBJECT_PATH)
            .await
        else {
            return;
        };
        let emitter = bridge.signal_emitter();
        let bridge = bridge.get().await;
        if previous.attention != next.attention {
            let _ = bridge.attention_changed(emitter).await;
        }
        if previous.label != next.label {
            let _ = bridge.label_changed(emitter).await;
        }
    });
}
