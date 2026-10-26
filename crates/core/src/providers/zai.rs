//! Z.AI Coding Plan (GLM), global platform: `GET /api/monitor/usage/quota/limit`, the endpoint
//! behind the plan's usage page (CodexBar reads the same). A plan has no balance, only quotas,
//! each reported as a percentage:
//!
//! - `TOKENS_LIMIT` / `CREDIT_LIMIT`: a rolling 5-hour window and a weekly one. Plans sold since
//!   July 30, 2026 count credits; older ("legacy") plans count tokens.
//! - `TIME_LIMIT`: the monthly allowance of MCP tool calls (web search, vision…), with counts.
//!
//! Peak hours are Monday to Friday 14:00–18:00 UTC+8 (docs.z.ai/devpack/overview). Credit plans
//! count off-peak usage at half rate; legacy plans count GLM-5.3 at 3× at peak and 1× off-peak
//! (docs.z.ai/devpack/notice/usage-revision). BigModel (open.bigmodel.cn) keys are separate.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::{
    decode, get_ok, BoxFuture, FetchError, HttpClient, Note, PeakRates, Provider, ProviderInfo,
    ProviderSnapshot, Quantity, ResetPeriod, UsageWindow, WindowKind,
};
use crate::peak::PeakHours;

pub const ENDPOINT: &str = "https://api.z.ai/api/monitor/usage/quota/limit";

// One peak range a day; clippy mistakes the list for a range meant to be collected.
#[allow(clippy::single_range_in_vec_init)]
static PEAK_HOURS: PeakHours = PeakHours {
    utc_offset_hours: 8,
    ranges: &[14 * 60..18 * 60],
};

static INFO: ProviderInfo = ProviderInfo {
    id: "zai",
    name: "Z.AI Coding Plan",
    dashboard_url: "https://z.ai/manage-apikey/coding-plan/personal/my-plan",
    keys_url: "https://z.ai/manage-apikey/apikey-list",
    key_placeholder: "API key",
    color: "#14B8A6",
    peak_hours: Some(&PEAK_HOURS),
};

const CREDIT_RATES: PeakRates = PeakRates {
    peak: 1.0,
    off_peak: 0.5,
};
/// GLM-5.3's, the flagship model; GLM-5.3-Flash counts 1.2× and 0.4×.
const LEGACY_RATES: PeakRates = PeakRates {
    peak: 3.0,
    off_peak: 1.0,
};

/// Zhipu's authentication error codes, which arrive in a 200 response.
const AUTH_ERRORS: [i64; 6] = [401, 1000, 1001, 1002, 1003, 1004];

pub struct Zai;

#[derive(Deserialize)]
struct Response {
    code: i64,
    #[serde(default)]
    success: bool,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    limits: Vec<Limit>,
    /// The plan's name has turned up under several fields.
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limit {
    #[serde(rename = "type")]
    kind: String,
    /// 1 day, 3 hour, 5 minute, 6 week; `number` of them.
    unit: i64,
    number: i64,
    percentage: f64,
    /// Unix milliseconds.
    next_reset_time: Option<i64>,
    /// The allowance, when counted.
    usage: Option<u64>,
    current_value: Option<u64>,
    remaining: Option<u64>,
}

impl Limit {
    fn minutes(&self) -> Option<i64> {
        let per_unit = match self.unit {
            1 => 1440,
            3 => 60,
            5 => 1,
            6 => 10080,
            _ => return None,
        };
        (self.number > 0).then(|| self.number * per_unit)
    }

    fn used(&self) -> Option<u64> {
        let limit = self.usage?;
        self.current_value
            .or_else(|| self.remaining.map(|left| limit.saturating_sub(left)))
            .map(|used| used.min(limit))
    }

    fn used_percent(&self) -> f64 {
        match (self.usage, self.used()) {
            (Some(limit), Some(used)) if limit > 0 => used as f64 / limit as f64 * 100.0,
            _ => self.percentage.clamp(0.0, 100.0),
        }
    }
}

impl Provider for Zai {
    fn info(&self) -> &'static ProviderInfo {
        &INFO
    }

    fn fetch<'a>(
        &'a self,
        http: &'a dyn HttpClient,
        api_key: &'a str,
        _now: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<ProviderSnapshot, FetchError>> {
        Box::pin(async move { parse(&get_ok(http, ENDPOINT, api_key).await?) })
    }
}

fn reset_after(minutes: Option<i64>) -> Option<ResetPeriod> {
    match minutes? {
        1440 => Some(ResetPeriod::Daily),
        10080 => Some(ResetPeriod::Weekly),
        43200 => Some(ResetPeriod::Monthly),
        _ => None,
    }
}

/// The window for a limit, with a sort key: 5-hour, then plan quotas shortest first, then tools.
fn window(limit: &Limit) -> Option<((u8, i64), UsageWindow)> {
    let minutes = limit.minutes();
    let (rank, kind, reset, count): (u8, _, _, fn(u64) -> Quantity) = match limit.kind.as_str() {
        "TOKENS_LIMIT" | "CREDIT_LIMIT" => {
            let count = if limit.kind == "CREDIT_LIMIT" {
                Quantity::Credits
            } else {
                Quantity::Tokens
            };
            if minutes == Some(300) {
                (0, WindowKind::FiveHour, None, count)
            } else {
                (1, WindowKind::PlanQuota, reset_after(minutes), count)
            }
        }
        // The monthly allowance is reported as "1 minute".
        "TIME_LIMIT" => {
            let reset = if minutes == Some(1) {
                Some(ResetPeriod::Monthly)
            } else {
                reset_after(minutes)
            };
            (2, WindowKind::ToolCalls, reset, Quantity::Calls)
        }
        _ => return None,
    };
    let window = UsageWindow {
        kind,
        used: limit.used().map(count),
        limit: limit.usage.map(count),
        used_percent: limit.used_percent(),
        reset,
        resets_at: limit
            .next_reset_time
            .and_then(DateTime::from_timestamp_millis),
    };
    Some(((rank, minutes.unwrap_or(i64::MAX)), window))
}

fn parse(body: &[u8]) -> Result<ProviderSnapshot, FetchError> {
    let response: Response = decode(body)?;
    if AUTH_ERRORS.contains(&response.code) {
        return Err(FetchError::Unauthorized);
    }
    let Some(data) = response
        .data
        .filter(|_| response.success && response.code == 200)
    else {
        return Err(FetchError::Decoding);
    };

    let mut ranked: Vec<_> = data.limits.iter().filter_map(window).collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let windows: Vec<UsageWindow> = ranked.into_iter().map(|(_, window)| window).collect();

    let can_make_calls = windows
        .iter()
        .filter(|w| w.kind != WindowKind::ToolCalls)
        .all(|w| w.used_percent < 100.0);
    let mut notes = Vec::new();
    if !can_make_calls {
        notes.push(Note::CannotMakeCalls);
    }
    let has = |kind: &str| data.limits.iter().any(|limit| limit.kind == kind);
    let peak_rates = if has("CREDIT_LIMIT") {
        Some(CREDIT_RATES)
    } else if has("TOKENS_LIMIT") {
        notes.push(Note::ZaiLegacyRates);
        Some(LEGACY_RATES)
    } else {
        None
    };
    let plan = ["planName", "plan", "plan_type", "packageName", "level"]
        .iter()
        .filter_map(|field| data.rest.get(*field)?.as_str())
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(str::to_owned);

    Ok(ProviderSnapshot {
        can_make_calls,
        windows,
        notes,
        plan,
        peak_rates,
        ..ProviderSnapshot::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::testing::{block_on, FakeHttp};

    fn fetch(body: &str) -> Result<ProviderSnapshot, FetchError> {
        let http = FakeHttp::default().with(ENDPOINT, 200, body);
        block_on(Zai.fetch(&http, "key.secret", Utc::now()))
    }

    #[test]
    fn parses_a_credit_plan() {
        let snapshot = fetch(
            r#"{"code":200,"msg":"success","success":true,"data":{"planName":"Pro","limits":[
              {"type":"CREDIT_LIMIT","unit":6,"number":1,"percentage":9,"nextResetTime":1786291200000},
              {"type":"CREDIT_LIMIT","unit":3,"number":5,"percentage":25,"nextResetTime":1785816000000},
              {"type":"TIME_LIMIT","unit":5,"number":1,"usage":1000,"currentValue":224,"remaining":776,
               "percentage":22,"usageDetails":[{"modelCode":"search-prime","usage":210}]}
            ]}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        assert_eq!(snapshot.peak_rates, Some(CREDIT_RATES));
        assert!(snapshot.can_make_calls);
        assert!(snapshot.notes.is_empty());
        assert!(snapshot.balances.is_empty());

        let [five_hour, weekly, tools] = snapshot.windows.as_slice() else {
            panic!("three windows: {:?}", snapshot.windows);
        };
        assert_eq!(five_hour.kind, WindowKind::FiveHour);
        assert_eq!(five_hour.used_percent, 25.0);
        assert_eq!(five_hour.used, None);
        assert_eq!(
            five_hour.resets_at,
            DateTime::from_timestamp_millis(1785816000000)
        );
        assert_eq!(weekly.kind, WindowKind::PlanQuota);
        assert_eq!(weekly.reset, Some(ResetPeriod::Weekly));
        assert_eq!(tools.kind, WindowKind::ToolCalls);
        assert_eq!(tools.reset, Some(ResetPeriod::Monthly));
        assert_eq!(tools.used, Some(Quantity::Calls(224)));
        assert_eq!(tools.limit, Some(Quantity::Calls(1000)));
        assert!((tools.used_percent - 22.4).abs() < 1e-9);
    }

    #[test]
    fn a_legacy_plan_counts_tokens_and_notes_its_rates() {
        let snapshot = fetch(
            r#"{"code":200,"success":true,"data":{"limits":[
              {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":100,"nextResetTime":1785816000000}
            ]}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.plan, None);
        assert_eq!(snapshot.peak_rates, Some(LEGACY_RATES));
        assert!(!snapshot.can_make_calls);
        assert_eq!(
            snapshot.notes,
            vec![Note::CannotMakeCalls, Note::ZaiLegacyRates]
        );
    }

    #[test]
    fn maps_errors() {
        assert_eq!(
            fetch(r#"{"code":1001,"msg":"Authorization header missing","success":false}"#),
            Err(FetchError::Unauthorized)
        );
        assert_eq!(
            fetch(r#"{"code":500,"msg":"busy","success":false}"#),
            Err(FetchError::Decoding)
        );
        let http = FakeHttp::default().with(ENDPOINT, 401, "");
        assert_eq!(
            block_on(Zai.fetch(&http, "k", Utc::now())),
            Err(FetchError::Unauthorized)
        );
    }
}
