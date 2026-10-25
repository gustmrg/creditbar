# AGENTS.md

Rules for agents working in this repository. CreditBar is a Tauri 2 tray app: a Rust core
(`crates/core`), the app shell (`crates/app`) and a plain HTML/CSS/JS panel (`ui/`). See the
[README](README.md) for the layout and the [release runbook](docs/release-runbook.md).

Everything in the repository is written in English: code, comments, commit messages, PRs and
docs.

## Commits

- Use Conventional Commits with a scope: `type(scope): summary`.
  - Types: `feat`, `fix`, `refactor`, `test`, `docs`, `style`, `chore`, `ci`.
  - The scope names the product area, not a file: `panel`, `tray`, `providers`, `pricing`,
    `holidays`, `i18n`, `settings`, `gnome`, `release`, `deps`, …
  - The summary is lowercase, imperative, without a final period, and says what changes for
    the user: `feat(providers): read the OpenRouter account balance`.
- The body is prose wrapped at about 72 columns that explains why and what changed. Trivial
  commits, such as dependency bumps, may have none.
- No attribution: no `Co-Authored-By` trailers and no "Generated with …" lines in commits or
  PR bodies.
- `chore(release): prepare vX.Y.Z` commits come only from the release workflow. Never write
  one by hand, bump the version yourself, or tag a release.

## Checks

CI runs these on Linux and macOS; run them before committing:

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

- Providers never reach the network in tests: use `providers::testing::FakeHttp`.
- A new string goes into every table in `crates/app/locales/`; a test fails otherwise.

## Releases

Betas and releases are cut by the **Release** workflow; see the
[release runbook](docs/release-runbook.md). While a beta is out, the next release must be
`minor` so it sorts above it.
