# CreditBar

A tray app for macOS, Linux and Windows that shows how much API credit or plan quota is left
across your LLM providers — DeepSeek, Kimi, OpenRouter and the Z.AI Coding Plan so far — and, for
DeepSeek and Z.AI, whether it's peak or off-peak.

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
  src/providers/        # Provider trait, ProviderSnapshot + DeepSeek, Kimi, OpenRouter, Z.AI
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

- **Linux / GNOME**: GNOME has no tray of its own. With the AppIndicator extension
  (`gnome-extensions enable ubuntu-appindicators@ubuntu.com`, package
  `gnome-shell-extension-appindicator`) a plain click always opens the tray menu, which leads
  with **Open Panel**. For one-click access install CreditBar's own
  [GNOME Shell extension](gnome-shell-extension/README.md): it replaces the tray icon with a
  top-bar button that opens the panel. The menu's preferences are also in the panel's
  **Settings**.
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

Releases are cut from GitHub Actions → **Release** → Run workflow, with **version** set to
`patch`, `minor`, `major`, `beta` or an exact version. The workflow bumps the version, runs the
checks, builds a universal macOS `.dmg` and a Linux `.deb` and AppImage, commits
`chore(release): prepare vX.Y.Z`, tags it and publishes the release with `SHA256SUMS.txt`.
See the [release runbook](docs/release-runbook.md).

`ci.yml` runs `cargo fmt --check`, `clippy -D warnings` and the tests on Linux and macOS for
every push to `main` and every pull request.

The builds are unsigned. On macOS, clear the quarantine flag once with
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
| Z.AI Coding Plan | `GET api.z.ai/api/monitor/usage/quota/limit` | A plan, so no balance: the 5-hour and weekly quotas as meters with their resets, the monthly MCP tool calls, the plan's name, and peak hours (weekdays 14:00–18:00 UTC+8) with a countdown and the plan's rates: 1×/0.5× for credit plans, 3×/1× for GLM-5.3 on legacy plans. Global platform keys only; open.bigmodel.cn keys are separate |

## The panel

Modelled on [CodexBar](https://github.com/steipete/CodexBar): every provider returns the same
`ProviderSnapshot` (balances, quota windows, details, notes), and the panel renders it with one
generic card, so adding a provider needs no UI code.

- One row per provider with a key: logo, name, headline figure, a one-line summary and the quota
  meter. Logos are single-colour SVGs in `ui/icons/<id>.svg`, taken from CodexBar's
  `ProviderIcon-*.svg` (MIT), in the brand's colour (`ProviderInfo::logo_color`) or, for
  black-and-white logos, the text colour; the marks belong to their owners. Clicking a row opens its card; the choice is remembered.
- Providers without a key sit under **Add a provider**.
- DeepSeek's pricing (period, countdown, calendar, weekly grid) lives inside the DeepSeek card;
  the DeepSeek row stays listed for it even without a key while **Show DeepSeek Peak Hours** is on.
- Providers with plain weekday peak hours (`ProviderInfo::peak_hours`, e.g. Z.AI) get a peak block
  in their card: period, rates, countdown and schedule. DeepSeek's holiday-aware schedule is
  separate.

## Adding a provider

1. Add `crates/core/src/providers/<name>.rs` implementing `Provider`: a static `ProviderInfo`
   (id, name, dashboard and key URLs, logo colour, peak hours) and `fetch`, which turns an API key into a
   `ProviderSnapshot` through the given `HttpClient` (`get_ok` handles 200 / 401 / 403).
2. Register it in `providers::all()`, and add its single-colour logo as `ui/icons/<id>.svg`.
3. Test `fetch` with `testing::FakeHttp` and captured responses; no network needed.
4. If the snapshot uses a new kind (`BalancePart`, `WindowKind`, `DetailKind`, `Note`), add its
   string to both tables in `crates/app/locales/`.
