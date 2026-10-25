//! OpenRouter. Two requests with the same API key:
//!
//! - `GET /api/v1/key` describes the key: its own spending limit (with daily, weekly or monthly
//!   reset) and its spend. Required; a 401 here means the key is bad.
//! - `GET /api/v1/credits` gives the account balance (`total_credits - total_usage`).
//!   OpenRouter documents it as management-key only, but it answered a regular key with 200
//!   when checked on 2026-10-25 (CodexBar relies on the same). Its failure only adds a note.
//!
//! Amounts are USD.

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

use super::{
    bearer_headers, decode, get_ok, AccountBalance, BoxFuture, Detail, DetailKind, FetchError,
    HttpClient, Money, Note, Provider, ProviderInfo, ProviderSnapshot, ResetPeriod, UsageWindow,
    WindowKind,
};

pub const KEY_ENDPOINT: &str = "https://openrouter.ai/api/v1/key";
pub const CREDITS_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";

static INFO: ProviderInfo = ProviderInfo {
    id: "openrouter",
    name: "OpenRouter",
    dashboard_url: "https://openrouter.ai/settings/credits",
    keys_url: "https://openrouter.ai/settings/keys",
    key_placeholder: "sk-or-v1-…",
    color: "#6467F2",
};

pub struct OpenRouter;

#[derive(Deserialize)]
struct KeyResponse {
    data: Key,
}

#[derive(Deserialize)]
struct Key {
    limit: Option<f64>,
    limit_remaining: Option<f64>,
    limit_reset: Option<String>,
    usage: f64,
    usage_daily: Option<f64>,
    usage_weekly: Option<f64>,
    usage_monthly: Option<f64>,
}

#[derive(Deserialize)]
struct CreditsResponse {
    data: Credits,
}

#[derive(Deserialize)]
struct Credits {
    total_credits: f64,
    total_usage: f64,
}

/// What the credits request yielded.
enum CreditsOutcome {
    Balance(f64),
    NeedsManagementKey,
    Unavailable,
}

impl Provider for OpenRouter {
    fn info(&self) -> &'static ProviderInfo {
        &INFO
    }

    fn fetch<'a>(
        &'a self,
        http: &'a dyn HttpClient,
        api_key: &'a str,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<ProviderSnapshot, FetchError>> {
        Box::pin(async move {
            let key: KeyResponse = decode(&get_ok(http, KEY_ENDPOINT, api_key).await?)?;
            let credits = match http.get(CREDITS_ENDPOINT, bearer_headers(api_key)).await {
                Ok(response) if response.status == 200 => decode::<CreditsResponse>(&response.body)
                    .map(|c| {
                        CreditsOutcome::Balance(
                            (c.data.total_credits - c.data.total_usage).max(0.0),
                        )
                    })
                    .unwrap_or(CreditsOutcome::Unavailable),
                Ok(response) if response.status == 403 => CreditsOutcome::NeedsManagementKey,
                _ => CreditsOutcome::Unavailable,
            };
            Ok(snapshot(key.data, credits, now))
        })
    }
}

fn snapshot(key: Key, credits: CreditsOutcome, now: DateTime<Utc>) -> ProviderSnapshot {
    let mut notes = Vec::new();
    let mut balances = Vec::new();
    let mut balance_left = true;
    match credits {
        CreditsOutcome::Balance(balance) => {
            balance_left = balance > 0.0;
            balances.push(AccountBalance {
                total: Money::usd(balance),
                parts: Vec::new(),
            });
        }
        CreditsOutcome::NeedsManagementKey => notes.push(Note::BalanceNeedsManagementKey),
        CreditsOutcome::Unavailable => notes.push(Note::BalanceUnavailable),
    }

    let mut windows = Vec::new();
    let mut limit_left = true;
    match key.limit.filter(|limit| *limit > 0.0) {
        Some(limit) => {
            let remaining = key
                .limit_remaining
                .unwrap_or(limit - key.usage)
                .clamp(0.0, limit);
            limit_left = remaining > 0.0;
            let reset = key.limit_reset.as_deref().and_then(parse_reset);
            windows.push(UsageWindow {
                kind: WindowKind::KeyLimit,
                used: Money::usd(limit - remaining),
                limit: Money::usd(limit),
                used_percent: (limit - remaining) / limit * 100.0,
                reset,
                resets_at: reset.map(|period| next_reset(period, now)),
            });
        }
        None if balances.is_empty() => notes.push(Note::NoKeyLimit),
        None => {}
    }

    let can_make_calls = balance_left && limit_left;
    if !can_make_calls {
        notes.insert(0, Note::CannotMakeCalls);
    }

    let details = [
        (DetailKind::SpentToday, key.usage_daily),
        (DetailKind::SpentThisWeek, key.usage_weekly),
        (DetailKind::SpentThisMonth, key.usage_monthly),
        (DetailKind::SpentWithKey, Some(key.usage)),
    ]
    .into_iter()
    .filter_map(|(kind, value)| {
        value.map(|v| Detail {
            kind,
            value: Money::usd(v),
        })
    })
    .collect();

    ProviderSnapshot {
        balances,
        can_make_calls,
        windows,
        details,
        notes,
    }
}

fn parse_reset(value: &str) -> Option<ResetPeriod> {
    match value {
        "daily" => Some(ResetPeriod::Daily),
        "weekly" => Some(ResetPeriod::Weekly),
        "monthly" => Some(ResetPeriod::Monthly),
        _ => None,
    }
}

/// OpenRouter's usage periods are UTC days, Monday-to-Sunday weeks and calendar months; the
/// key limit resets at the start of the next one.
fn next_reset(period: ResetPeriod, now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive();
    let day: NaiveDate = match period {
        ResetPeriod::Daily => today + Days::new(1),
        ResetPeriod::Weekly => {
            let days_to_monday = 7 - u64::from(today.weekday().num_days_from_monday());
            today + Days::new(days_to_monday)
        }
        ResetPeriod::Monthly => today.with_day(1).expect("day 1 exists") + Months::new(1),
    };
    Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::testing::{block_on, FakeHttp};

    const LIMITED_KEY: &str = r#"{"data":{"label":"sk-or-v1-abc","limit":100,"limit_remaining":74.5,"limit_reset":"monthly","usage":25.5,"usage_daily":1.25,"usage_weekly":6,"usage_monthly":25.5,"is_free_tier":false}}"#;
    const UNLIMITED_KEY: &str = r#"{"data":{"label":"x","limit":null,"limit_remaining":null,"limit_reset":null,"usage":3.25}}"#;
    const CREDITS: &str = r#"{"data":{"total_credits":20,"total_usage":12.5}}"#;

    fn now() -> DateTime<Utc> {
        // A Wednesday.
        Utc.with_ymd_and_hms(2026, 10, 28, 15, 30, 0).unwrap()
    }

    fn fetch(http: FakeHttp) -> Result<ProviderSnapshot, FetchError> {
        block_on(OpenRouter.fetch(&http, "sk-or-v1-test", now()))
    }

    #[test]
    fn balance_key_limit_and_spend() {
        let http = FakeHttp::default()
            .with(KEY_ENDPOINT, 200, LIMITED_KEY)
            .with(CREDITS_ENDPOINT, 200, CREDITS);
        let snapshot = fetch(http).unwrap();

        assert_eq!(snapshot.balances[0].total, Money::usd(7.5));
        assert!(snapshot.can_make_calls);
        assert!(snapshot.notes.is_empty());

        let window = &snapshot.windows[0];
        assert_eq!(window.kind, WindowKind::KeyLimit);
        assert_eq!(window.used, Money::usd(25.5));
        assert_eq!(window.limit, Money::usd(100.0));
        assert!((window.used_percent - 25.5).abs() < 1e-9);
        assert_eq!(window.reset, Some(ResetPeriod::Monthly));
        assert_eq!(
            window.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap())
        );

        let kinds: Vec<_> = snapshot.details.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DetailKind::SpentToday,
                DetailKind::SpentThisWeek,
                DetailKind::SpentThisMonth,
                DetailKind::SpentWithKey,
            ]
        );
    }

    #[test]
    fn regular_key_refused_by_credits_still_shows_the_key() {
        let http = FakeHttp::default()
            .with(KEY_ENDPOINT, 200, UNLIMITED_KEY)
            .with(CREDITS_ENDPOINT, 403, r#"{"error":{"code":403,"message":"Only management keys can perform this operation"}}"#);
        let snapshot = fetch(http).unwrap();
        assert!(snapshot.balances.is_empty());
        assert!(snapshot.windows.is_empty());
        assert_eq!(
            snapshot.notes,
            vec![Note::BalanceNeedsManagementKey, Note::NoKeyLimit]
        );
        assert_eq!(
            snapshot.details,
            vec![Detail {
                kind: DetailKind::SpentWithKey,
                value: Money::usd(3.25)
            }]
        );
        assert!(snapshot.can_make_calls);
    }

    #[test]
    fn credits_outage_is_only_a_note() {
        let http = FakeHttp::default()
            .with(KEY_ENDPOINT, 200, LIMITED_KEY)
            .failing(CREDITS_ENDPOINT);
        let snapshot = fetch(http).unwrap();
        assert_eq!(snapshot.notes, vec![Note::BalanceUnavailable]);
        assert_eq!(snapshot.windows.len(), 1);
    }

    #[test]
    fn exhausted_limit_or_balance_cannot_make_calls() {
        let exhausted =
            r#"{"data":{"limit":10,"limit_remaining":0,"limit_reset":"daily","usage":10}}"#;
        let http = FakeHttp::default().with(KEY_ENDPOINT, 200, exhausted).with(
            CREDITS_ENDPOINT,
            200,
            CREDITS,
        );
        let snapshot = fetch(http).unwrap();
        assert!(!snapshot.can_make_calls);
        assert_eq!(snapshot.notes[0], Note::CannotMakeCalls);
        assert_eq!(
            snapshot.windows[0].resets_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 29, 0, 0, 0).unwrap())
        );

        let empty = r#"{"data":{"total_credits":5,"total_usage":5}}"#;
        let http = FakeHttp::default()
            .with(KEY_ENDPOINT, 200, UNLIMITED_KEY)
            .with(CREDITS_ENDPOINT, 200, empty);
        assert!(!fetch(http).unwrap().can_make_calls);
    }

    #[test]
    fn bad_key_fails_the_whole_fetch() {
        let http = FakeHttp::default().with(KEY_ENDPOINT, 401, "");
        assert_eq!(fetch(http), Err(FetchError::Unauthorized));
    }

    #[test]
    fn weekly_and_monthly_resets() {
        // Wednesday → next Monday; a Monday → the following Monday.
        assert_eq!(
            next_reset(ResetPeriod::Weekly, now()),
            Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap()
        );
        let monday = Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 1).unwrap();
        assert_eq!(
            next_reset(ResetPeriod::Weekly, monday),
            Utc.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap()
        );
        let december = Utc.with_ymd_and_hms(2026, 12, 31, 23, 0, 0).unwrap();
        assert_eq!(
            next_reset(ResetPeriod::Monthly, december),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
    }
}
