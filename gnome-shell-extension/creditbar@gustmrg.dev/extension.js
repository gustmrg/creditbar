// CreditBar's top-bar button. GNOME's AppIndicator extension opens a tray icon's menu on a
// plain click, so this button stands in for CreditBar's tray icon: one click opens the panel.
//
// It talks to the app over the session bus (see crates/app/src/shell.rs):
// - the app serves dev.gustmrg.CreditBar1 (TogglePanel, Attention, Label);
// - the extension owns dev.gustmrg.CreditBar.ShellIndicator while enabled, which makes the app
//   hide its own tray icon.
// The button only shows while the app is running.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_BUS_NAME = 'dev.gustmrg.CreditBar';
const APP_OBJECT_PATH = '/dev/gustmrg/CreditBar';
const INDICATOR_BUS_NAME = 'dev.gustmrg.CreditBar.ShellIndicator';
const PANEL_TITLE = 'CreditBar';

const BridgeProxy = Gio.DBusProxy.makeProxyWrapper(`
<node>
  <interface name="dev.gustmrg.CreditBar1">
    <method name="TogglePanel">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="i" direction="in" name="width"/>
      <arg type="i" direction="in" name="height"/>
      <arg type="b" direction="out" name="open"/>
    </method>
    <property name="Attention" type="b" access="read"/>
    <property name="Label" type="s" access="read"/>
  </interface>
</node>`);

const CreditBarButton = GObject.registerClass(
class CreditBarButton extends PanelMenu.Button {
    _init(extensionPath) {
        super._init(0.5, 'CreditBar', true);

        this._icons = {
            normal: Gio.icon_new_for_string(`${extensionPath}/icons/creditbar-symbolic.svg`),
            attention: Gio.icon_new_for_string(`${extensionPath}/icons/creditbar-attention.svg`),
        };
        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({gicon: this._icons.normal, style_class: 'system-status-icon'});
        this._label = new St.Label({
            style_class: 'creditbar-label',
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
        });
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);

        this._focusSource = 0;
        this._proxy = null;
        this._proxySignals = [];
        this._cancellable = new Gio.Cancellable();
        this.visible = false;
        this.container.visible = false;

        new BridgeProxy(Gio.DBus.session, APP_BUS_NAME, APP_OBJECT_PATH, (proxy, error) => {
            if (error) {
                if (!error.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    logError(error, 'CreditBar: no bus proxy');
                return;
            }
            this._proxy = proxy;
            this._proxySignals = [
                proxy.connect('notify::g-name-owner', () => this._sync()),
                proxy.connect('g-properties-changed', () => this._sync()),
            ];
            this._sync();
        }, this._cancellable, Gio.DBusProxyFlags.DO_NOT_AUTO_START);
    }

    /** Mirrors the app: hidden while it isn't running, amber icon, countdown label. */
    _sync() {
        const running = Boolean(this._proxy?.g_name_owner);
        this.visible = running;
        this.container.visible = running;
        if (!running)
            return;
        this._icon.gicon = this._proxy.Attention ? this._icons.attention : this._icons.normal;
        const label = this._proxy.Label ?? '';
        this._label.text = label;
        this._label.visible = label !== '';
    }

    vfunc_event(event) {
        const type = event.type();
        if (type === Clutter.EventType.BUTTON_PRESS || type === Clutter.EventType.TOUCH_BEGIN) {
            this._togglePanel();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    _togglePanel() {
        if (!this._proxy?.g_name_owner)
            return;
        const [x, y] = this.get_transformed_position();
        const [width, height] = this.get_transformed_size();
        this._proxy.TogglePanelRemote(
            Math.round(x), Math.round(y), Math.round(width), Math.round(height),
            ([open] = [], error) => {
                if (error) {
                    logError(error, 'CreditBar: TogglePanel failed');
                    return;
                }
                if (open)
                    this._focusPanel();
            });
    }

    /**
     * Focuses the panel once its window maps. GNOME's focus-stealing prevention would refuse
     * the app's own request, since the click happened in the Shell; the Shell may grant it.
     */
    _focusPanel(attempts = 20) {
        this._clearFocusSource();
        const panel = global.get_window_actors()
            .map(actor => actor.meta_window)
            .find(window => window.get_title() === PANEL_TITLE &&
                window.get_wm_class_instance?.() === 'creditbar' &&
                window.showing_on_its_workspace());
        if (panel) {
            Main.activateWindow(panel);
            return;
        }
        if (attempts > 0) {
            this._focusSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 25, () => {
                this._focusSource = 0;
                this._focusPanel(attempts - 1);
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    _clearFocusSource() {
        if (this._focusSource) {
            GLib.source_remove(this._focusSource);
            this._focusSource = 0;
        }
    }

    destroy() {
        this._cancellable.cancel();
        this._clearFocusSource();
        for (const id of this._proxySignals)
            this._proxy.disconnect(id);
        this._proxySignals = [];
        this._proxy = null;
        super.destroy();
    }
});

export default class CreditBarExtension extends Extension {
    enable() {
        this._button = new CreditBarButton(this.path);
        Main.panel.addToStatusArea(this.uuid, this._button);
        // While this name has an owner, the app hides its own tray icon.
        this._nameId = Gio.bus_own_name(
            Gio.BusType.SESSION, INDICATOR_BUS_NAME, Gio.BusNameOwnerFlags.NONE,
            null, null, null);
    }

    disable() {
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = 0;
        }
        this._button?.destroy();
        this._button = null;
    }
}
