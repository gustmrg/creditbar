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
  src/providers/        # Provider trait, ProviderSnapshot + DeepSeek, Kimi, OpenRouter
  src/http.rs           # HttpClient over reqwest (feature `http`, on by default)
  tests/schedule.rs     # cases ported from the Swift app's self-check
  examples/status.rs    # prints the current snapshot (and balance with DEEPSEEK_API_KEY)
crates/app/             # creditbar: Tauri 2 tray app
  src/tray.rs           # tray icon + menu, updated every second
  src/panel.rs          # panel window: placement under the tray icon, hide on blur
  src/balance.rs        # per-provider balance state, OS keyring for API keys
  src/holidays.rs       # daily ICS refresh (ETag) + on-disk cache
ui/                     # panel: plain HTML/CSS/JS, no bundler
```

## The tray

- The tray icon is a credit bar. It turns amber when a provider with a saved key failed to
  refresh, reports that its account can't make API calls, or has a quota 90% used.
- **Show DeepSeek Peak Hours** (on by default) adds the period, next switch and optional
  countdown to the tray menu.

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

### Releases (GitHub Actions)

`.github/workflows/release.yml` builds the macOS and Linux installers when a version tag is
pushed, and attaches them to a **draft** release to review and publish:

```bash
# bump "version" in crates/app/tauri.conf.json first; the tag must match it
git tag v0.1.0 && git push origin v0.1.0
```

| Platform | Runner | Output |
| --- | --- | --- |
| macOS | `macos-latest` | universal `.dmg` (Apple Silicon + Intel) |
| Linux | `ubuntu-22.04` | `.deb` and AppImage |

`ci.yml` runs `cargo fmt --check`, `clippy -D warnings` and the tests on Linux and macOS for
every push to `main` and every pull request.

The builds are unsigned. On macOS, an unsigned app downloaded from the internet is reported as
"damaged"; clear the quarantine flag once with
`xattr -dr com.apple.quarantine /Applications/CreditBar.app`.

### Local builds

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

| Provider | Endpoints | What the card shows |
| --- | --- | --- |
| DeepSeek | `GET api.deepseek.com/user/balance` | Account balance (USD first, then CNY) split into paid and promo; DeepSeek pricing section |
| Kimi (Moonshot) | `GET api.moonshot.ai/v1/users/me/balance` | Account balance in USD split into paid and voucher; what the account owes. International platform keys only; api.moonshot.cn keys are separate |
| OpenRouter | `GET openrouter.ai/api/v1/key` and `/credits` | Account balance; the key's own spending limit as a meter with its reset; spend today, this week, this month and in total. `/credits` is documented as management-key only but answers regular keys; if it refuses, the card says so and shows the key's figures |

## The panel

Modelled on [CodexBar](https://github.com/steipete/CodexBar): every provider returns the same
`ProviderSnapshot` (balances, quota windows, details, notes), and the panel renders it with one
generic card, so adding a provider needs no UI code.

- One row per provider with a key: name, headline figure, a one-line summary and the quota
  meter. Clicking a row opens its card; the choice is remembered.
- Providers without a key sit under **Add a provider**.
- DeepSeek's pricing (period, countdown, calendar, weekly grid) lives inside the DeepSeek card;
  the DeepSeek row stays listed for it even without a key while **Show DeepSeek Peak Hours** is on.

## Adding a provider

1. Add `crates/core/src/providers/<name>.rs` implementing `Provider`: a static `ProviderInfo`
   (id, name, dashboard and key URLs, colour) and `fetch`, which turns an API key into a
   `ProviderSnapshot` through the given `HttpClient` (`get_ok` handles 200 / 401 / 403).
2. Register it in `providers::all()`.
3. Test `fetch` with `testing::FakeHttp` and captured responses; no network needed.
4. If the snapshot uses a new kind (`BalancePart`, `WindowKind`, `DetailKind`, `Note`), add its
   string to both tables in `crates/app/locales/`.
