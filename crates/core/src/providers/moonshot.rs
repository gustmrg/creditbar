//! Kimi (Moonshot AI), international platform: `GET /v1/users/me/balance`, amounts in USD.
//! Keys from the China platform (api.moonshot.cn) are separate and get a 401 here.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{
    decode, get_ok, AccountBalance, BalancePart, BoxFuture, FetchError, HttpClient, Money, Note,
    Provider, ProviderInfo, ProviderSnapshot,
};

pub const ENDPOINT: &str = "https://api.moonshot.ai/v1/users/me/balance";

static INFO: ProviderInfo = ProviderInfo {
    id: "moonshot",
    name: "Kimi (Moonshot)",
    dashboard_url: "https://platform.kimi.ai/console/account",
    keys_url: "https://platform.kimi.ai/console/api-keys",
    key_placeholder: "sk-…",
    logo_color: None,
    peak_hours: None,
};

pub struct Moonshot;

#[derive(Deserialize)]
struct Response {
    /// 0 means success.
    code: i64,
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    /// Voucher plus cash. At or below zero, inference calls are refused.
    available_balance: f64,
    voucher_balance: f64,
    /// Negative when the account owes money.
    cash_balance: f64,
}

impl Provider for Moonshot {
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

fn parse(body: &[u8]) -> Result<ProviderSnapshot, FetchError> {
    let response: Response = decode(body)?;
    if response.code != 0 {
        return Err(FetchError::Decoding);
    }
    let data = response.data;
    let can_make_calls = data.available_balance > 0.0;

    let mut notes = Vec::new();
    if !can_make_calls {
        notes.push(Note::CannotMakeCalls);
    }
    if data.cash_balance < 0.0 {
        notes.push(Note::Deficit {
            amount: Money::usd(-data.cash_balance),
        });
    }

    Ok(ProviderSnapshot {
        balances: vec![AccountBalance {
            total: Money::usd(data.available_balance),
            parts: vec![
                (BalancePart::Paid, Money::usd(data.cash_balance.max(0.0))),
                (BalancePart::Promo, Money::usd(data.voucher_balance)),
            ],
        }],
        can_make_calls,
        windows: Vec::new(),
        details: Vec::new(),
        notes,
        ..ProviderSnapshot::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::testing::{block_on, FakeHttp};

    fn fetch(body: &str) -> Result<ProviderSnapshot, FetchError> {
        let http = FakeHttp::default().with(ENDPOINT, 200, body);
        block_on(Moonshot.fetch(&http, "sk-test", Utc::now()))
    }

    #[test]
    fn parses_balance() {
        let snapshot = fetch(r#"{"code":0,"data":{"available_balance":49.58894,"voucher_balance":46.58893,"cash_balance":3.00001},"scode":"0x0","status":true}"#).unwrap();
        assert!(snapshot.can_make_calls);
        assert_eq!(snapshot.balances[0].total, Money::usd(49.58894));
        assert_eq!(
            snapshot.balances[0].parts,
            vec![
                (BalancePart::Paid, Money::usd(3.00001)),
                (BalancePart::Promo, Money::usd(46.58893)),
            ]
        );
        assert!(snapshot.notes.is_empty());
    }

    #[test]
    fn owing_money_is_noted() {
        let snapshot = fetch(
            r#"{"code":0,"data":{"available_balance":0,"voucher_balance":0,"cash_balance":-1.5}}"#,
        )
        .unwrap();
        assert!(!snapshot.can_make_calls);
        assert_eq!(
            snapshot.notes,
            vec![
                Note::CannotMakeCalls,
                Note::Deficit {
                    amount: Money::usd(1.5)
                }
            ]
        );
    }

    #[test]
    fn maps_errors() {
        let http = FakeHttp::default().with(ENDPOINT, 401, "");
        assert_eq!(
            block_on(Moonshot.fetch(&http, "k", Utc::now())),
            Err(FetchError::Unauthorized)
        );
        let failed =
            r#"{"code":5,"data":{"available_balance":0,"voucher_balance":0,"cash_balance":0}}"#;
        assert_eq!(fetch(failed), Err(FetchError::Decoding));
    }
}
