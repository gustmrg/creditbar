# CreditBar GNOME Shell extension

GNOME's AppIndicator extension opens a tray icon's menu on a plain click, so CreditBar's tray icon
needs two clicks to reach the panel. This extension draws CreditBar's own button in the top bar
instead: **one click opens the panel**, right under the button. It mirrors the tray icon (amber
when a provider needs attention, DeepSeek countdown next to it) and only shows while the app runs.

While the extension is enabled, the app hides its AppIndicator tray icon; disabling the extension
brings it back. The preferences that lived in the tray menu are also in the panel's
**Settings**.

Requires GNOME Shell 45 or later and the CreditBar app.

## Install

From a release, download `creditbar@gustmrg.dev.shell-extension.zip`, then:

```bash
gnome-extensions install --force creditbar@gustmrg.dev.shell-extension.zip
```

From the repository:

```bash
cp -r gnome-shell-extension/creditbar@gustmrg.dev ~/.local/share/gnome-shell/extensions/
```

GNOME Shell only discovers a newly installed extension after a restart: on X11 press
`Alt+F2`, type `r`, Enter; on Wayland log out and back in. Then:

```bash
gnome-extensions enable creditbar@gustmrg.dev
```

## How it talks to the app

Over the session bus (see `crates/app/src/shell.rs`):

- The app owns `dev.gustmrg.CreditBar` and serves `dev.gustmrg.CreditBar1` at
  `/dev/gustmrg/CreditBar`: `TogglePanel(x, y, width, height) → open` and the `Attention` and
  `Label` properties.
- The extension owns `dev.gustmrg.CreditBar.ShellIndicator` while enabled; the app hides its tray
  icon while that name has an owner.

After opening the panel, the extension focuses it: GNOME's focus-stealing prevention would refuse
the app's own request, because the click happened in the Shell.

On Wayland, applications can't place their windows, so the panel opens where the compositor puts
it rather than under the button.
