# DeepSeek Status (Rust)

Cross-platform port of [DeepSeek Status](https://github.com/owenzhao/DeepSeekStatus), the macOS
menu bar app that shows whether DeepSeek is in peak or off-peak pricing and how much API balance
is left — extended to support more balance providers.

Status: **early**. Core logic plus a Tauri 2 tray shell with the panel. Tested on Linux
(GNOME, X11); macOS and Windows build paths exist but are untested.

## Layout

```
crates/core/            # status-core: pure logic, no UI
  src/pricing.rs        # peak / off-peak rule (Beijing time), transitions, snapshot
  src/holidays.rs       # Apple holiday ICS parser + bundled 2026 fallback
  src/providers/        # BalanceProvider trait + DeepSeek, Kimi, OpenRouter
  src/http.rs           # performs a provider's request (feature `http`, on by default)
  tests/schedule.rs     # cases ported from the Swift app's self-check
  examples/status.rs    # prints the current snapshot (and balance with DEEPSEEK_API_KEY)
crates/app/             # deepseek-status: Tauri 2 tray app
  src/tray.rs           # tray icon + menu, updated every second
  src/panel.rs          # panel window: placement under the tray icon, hide on blur
  src/balance.rs        # per-provider balance state, OS keyring for API keys
  src/holidays.rs       # daily ICS refresh (ETag) + on-disk cache
ui/                     # panel: plain HTML/CSS/JS, no bundler
```

## Platform notes

- **Linux / GNOME**: GNOME has no tray of its own. Enable the AppIndicator extension
  (`gnome-extensions enable ubuntu-appindicators@ubuntu.com`, package
  `gnome-shell-extension-appindicator`). Linux trays only open the menu, so the panel is
  reached through **Open Panel**; the menu itself shows the period and the next switch.
- **NVIDIA**: the app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux, otherwise WebKitGTK
  renders a blank window.
- **macOS**: no Dock icon (accessory activation policy); the countdown can sit next to the icon.
- **Windows**: no tray title, so the tooltip shows the next switch.

## Commands

```bash
cargo test
cargo run -p deepseek-status                       # the tray app
DEEPSEEK_STATUS_OPEN_PANEL=1 cargo run -p deepseek-status   # ...with the panel open
cargo run -p status-core --example status
DEEPSEEK_API_KEY=sk-... cargo run -p status-core --example status
```

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
