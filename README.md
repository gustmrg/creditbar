# CreditBar

A tray app for macOS, Linux and Windows that shows how much API credit is left across your LLM
providers — DeepSeek, Kimi and OpenRouter so far — and, for DeepSeek, whether it's in peak or
off-peak pricing.

CreditBar started as a cross-platform port of
[DeepSeek Status](https://github.com/owenzhao/DeepSeekStatus) by Zhao Xin, the macOS menu bar app
whose pricing schedule and holiday calendar logic it carries over.

Status: **early**. Core logic plus a Tauri 2 tray shell with the panel. Tested on Linux
(GNOME, X11); macOS and Windows build paths exist but are untested.

## Layout

```
crates/core/            # creditbar-core: pure logic, no UI
  src/pricing.rs        # peak / off-peak rule (Beijing time), transitions, snapshot
  src/holidays.rs       # Apple holiday ICS parser + bundled 2026 fallback
  src/providers/        # BalanceProvider trait + DeepSeek, Kimi, OpenRouter
  src/http.rs           # performs a provider's request (feature `http`, on by default)
  tests/schedule.rs     # cases ported from the Swift app's self-check
  examples/status.rs    # prints the current snapshot (and balance with DEEPSEEK_API_KEY)
crates/app/             # creditbar: Tauri 2 tray app
  src/tray.rs           # tray icon + menu, updated every second
  src/panel.rs          # panel window: placement under the tray icon, hide on blur
  src/balance.rs        # per-provider balance state, OS keyring for API keys
  src/holidays.rs       # daily ICS refresh (ETag) + on-disk cache
ui/                     # panel: plain HTML/CSS/JS, no bundler
```

## The panel and the tray

- The panel lists every provider's balance; a provider without a key only offers **Add API key**.
- **DeepSeek pricing** (period, countdown, monthly calendar, weekly grid) is a section of its
  own, toggled from the tray menu (**Show DeepSeek Peak Hours**, on by default).
- The tray icon is a credit bar. It turns amber when a provider with a saved key failed to
  refresh or reports that its account can't make API calls.

## Platform notes

- **Linux / GNOME**: GNOME has no tray of its own. Enable the AppIndicator extension
  (`gnome-extensions enable ubuntu-appindicators@ubuntu.com`, package
  `gnome-shell-extension-appindicator`). Linux trays only open the menu, so the panel is
  reached through **Open Panel**; with DeepSeek pricing on, the menu itself shows the period and
  the next switch.
- **NVIDIA**: the app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux, otherwise WebKitGTK
  renders a blank window.
- **macOS**: no Dock icon (accessory activation policy); the countdown can sit next to the icon.
- **Windows**: no tray title, so the tooltip shows the next switch.

## Commands

```bash
cargo test
cargo run -p creditbar                       # the tray app
CREDITBAR_OPEN_PANEL=1 cargo run -p creditbar   # ...with the panel open
cargo run -p creditbar-core --example status
DEEPSEEK_API_KEY=sk-... cargo run -p creditbar-core --example status
```

## Installers

```bash
cargo install tauri-cli --version "^2" --locked   # once
cd crates/app
cargo tauri build --bundles deb,appimage          # Linux
cargo tauri build --bundles dmg                   # macOS (on a Mac)
cargo tauri build --bundles nsis                  # Windows (on Windows)
```

Output lands in `target/release/bundle/`. The `.deb` (≈5 MB) depends on the system WebKitGTK and
AppIndicator; the AppImage (≈110 MB) carries them. Builds are unsigned: macOS needs
right-click → Open the first time, and Windows SmartScreen asks once.

## Languages

English (en-US) and Brazilian Portuguese (pt-BR), picked from the system locale
(`CREDITBAR_LANG=pt-BR` overrides it). The tables live in `crates/app/locales/*.json` and
serve both the tray menu and the panel; a test checks every table has every English key.

## Providers

| Provider | Endpoint | What the panel shows |
| --- | --- | --- |
| DeepSeek | `GET api.deepseek.com/user/balance` | Account balance (CNY or USD), promo and paid parts |
| Kimi (Moonshot) | `GET api.moonshot.ai/v1/users/me/balance` | Account balance in USD, voucher and cash parts. International platform keys only; api.moonshot.cn keys are separate |
| OpenRouter | `GET openrouter.ai/api/v1/key` | What is left of the key's own limit, or what the key has spent if it has no limit. The account balance needs a management key, which this app deliberately doesn't ask for |

## Adding a provider

1. Add `crates/core/src/providers/<name>.rs` implementing `BalanceProvider`:
   `balance_request` describes the GET, `parse_balance` turns status + body into a `Balance`
   (use `check_status` for the usual 200 / 401 / 403 handling).
2. Register it in `providers::all()`.
3. Unit-test `parse_balance` against a captured response; no network needed.
