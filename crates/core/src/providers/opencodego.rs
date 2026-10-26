//! OpenCode Go, the subscription on OpenCode Zen: `GET /zen/go/v1/usage` with the Zen API key
//! (what t3code and CodexBar read). A plan, so no balance: a rolling 5-hour window, a weekly
//! and a monthly one, each a percentage with its reset time.
//!
//! A bad key gets a 401. A 403 is a valid Zen key without a Go subscription (Zen itself is
//! pay-as-you-go, and its balance is only on the web dashboard), which becomes a note.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{
    bearer_headers, decode, BoxFuture, FetchError, HttpClient, Note, Provider, ProviderInfo,
    ProviderSnapshot, ResetPeriod, UsageWindow, WindowKind,
};

pub const ENDPOINT: &str = "https://opencode.ai/zen/go/v1/usage";

static INFO: ProviderInfo = ProviderInfo {
    id: "opencodego",
    name: "OpenCode Go",
    dashboard_url: "https://opencode.ai/auth",
    keys_url: "https://opencode.ai/auth",
    key_placeholder: "sk-…",
    logo_color: None,
    peak_hours: None,
};

pub struct OpenCodeGo;

#[derive(Deserialize)]
struct Response {
    usage: Usage,
}

#[derive(Deserialize)]
struct Usage {
    rolling: Window,
    weekly: Option<Window>,
    monthly: Option<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    /// 0–100.
    percent: f64,
    resets_at: Option<DateTime<Utc>>,
}

impl Window {
    fn into_usage(self, kind: WindowKind, reset: Option<ResetPeriod>) -> UsageWindow {
        UsageWindow {
            kind,
            used: None,
            limit: None,
            used_percent: self.percent.clamp(0.0, 100.0),
            reset,
            resets_at: self.resets_at,
        }
    }
}

impl Provider for OpenCodeGo {
    fn info(&self) -> &'static ProviderInfo {
        &INFO
    }

    fn fetch<'a>(
        &'a self,
        http: &'a dyn HttpClient,
        api_key: &'a str,
        _now: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<ProviderSnapshot, FetchError>> {
        Box::pin(async move {
            let response = http
                .get(ENDPOINT, bearer_headers(api_key))
                .await
                .map_err(FetchError::Network)?;
            match response.status {
                200 => parse(&response.body),
                403 => Ok(ProviderSnapshot {
                    can_make_calls: true,
                    notes: vec![Note::NoGoSubscription],
                    ..ProviderSnapshot::default()
                }),
                401 => Err(FetchError::Unauthorized),
                status => Err(FetchError::Http { status }),
            }
        })
    }
}

fn parse(body: &[u8]) -> Result<ProviderSnapshot, FetchError> {
    let usage = decode::<Response>(body)?.usage;
    let mut windows = vec![usage.rolling.into_usage(WindowKind::FiveHour, None)];
    windows.extend(
        usage
            .weekly
            .map(|w| w.into_usage(WindowKind::PlanQuota, Some(ResetPeriod::Weekly))),
    );
    windows.extend(
        usage
            .monthly
            .map(|w| w.into_usage(WindowKind::PlanQuota, Some(ResetPeriod::Monthly))),
    );
    let can_make_calls = windows.iter().all(|w| w.used_percent < 100.0);
    Ok(ProviderSnapshot {
        can_make_calls,
        windows,
        notes: if can_make_calls {
            Vec::new()
        } else {
            vec![Note::CannotMakeCalls]
        },
        ..ProviderSnapshot::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::testing::{block_on, FakeHttp};

    fn fetch_with(status: u16, body: &str) -> Result<ProviderSnapshot, FetchError> {
        let http = FakeHttp::default().with(ENDPOINT, status, body);
        block_on(OpenCodeGo.fetch(&http, "sk-test", Utc::now()))
    }

    #[test]
    fn parses_the_three_windows() {
        let snapshot = fetch_with(
            200,
            r#"{"usage":{
              "rolling":{"percent":22.5,"resetsAt":"2026-10-26T08:00:00Z"},
              "weekly":{"percent":44,"resetsAt":"2026-10-30T00:00:00Z"},
              "monthly":{"percent":55,"resetsAt":"2026-11-15T00:00:00Z"}}}"#,
        )
        .unwrap();
        assert!(snapshot.can_make_calls);
        assert!(snapshot.balances.is_empty() && snapshot.notes.is_empty());
        let [rolling, weekly, monthly] = snapshot.windows.as_slice() else {
            panic!("three windows: {:?}", snapshot.windows);
        };
        assert_eq!(rolling.kind, WindowKind::FiveHour);
        assert_eq!(rolling.used_percent, 22.5);
        assert_eq!(
            rolling.resets_at,
            Some("2026-10-26T08:00:00Z".parse().unwrap())
        );
        assert_eq!(
            (weekly.kind, weekly.reset),
            (WindowKind::PlanQuota, Some(ResetPeriod::Weekly))
        );
        assert_eq!(monthly.reset, Some(ResetPeriod::Monthly));
        assert_eq!(monthly.used_percent, 55.0);
    }

    #[test]
    fn a_used_up_window_blocks_calls() {
        let snapshot = fetch_with(
            200,
            r#"{"usage":{"rolling":{"percent":100,"resetsAt":"2026-10-26T08:00:00Z"}}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert!(!snapshot.can_make_calls);
        assert_eq!(snapshot.notes, vec![Note::CannotMakeCalls]);
    }

    #[test]
    fn maps_statuses() {
        let unauthorized =
            r#"{"type":"error","error":{"type":"AuthError","message":"Unauthorized"}}"#;
        assert_eq!(fetch_with(401, unauthorized), Err(FetchError::Unauthorized));
        assert_eq!(
            fetch_with(403, "").unwrap().notes,
            vec![Note::NoGoSubscription]
        );
        assert_eq!(fetch_with(500, ""), Err(FetchError::Http { status: 500 }));
        assert_eq!(
            fetch_with(200, r#"{"usage":{}}"#),
            Err(FetchError::Decoding)
        );
    }
}
