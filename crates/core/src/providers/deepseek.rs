//! DeepSeek: `GET /user/balance` with the API key. The query itself costs nothing.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{
    decode, get_ok, AccountBalance, BalancePart, BoxFuture, FetchError, HttpClient, Money, Note,
    Provider, ProviderInfo, ProviderSnapshot,
};

pub const ENDPOINT: &str = "https://api.deepseek.com/user/balance";

static INFO: ProviderInfo = ProviderInfo {
    id: "deepseek",
    name: "DeepSeek",
    dashboard_url: "https://platform.deepseek.com/usage",
    keys_url: "https://platform.deepseek.com/api_keys",
    key_placeholder: "sk-…",
    logo_color: Some("#4D6BFE"),
    peak_hours: None,
};

pub struct DeepSeek;

#[derive(Deserialize)]
struct Response {
    is_available: bool,
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Deserialize)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

impl Provider for DeepSeek {
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
    let mut balances: Vec<AccountBalance> = response
        .balance_infos
        .into_iter()
        .map(|info| {
            let money = |amount: String| Money {
                currency: info.currency.clone(),
                amount,
            };
            AccountBalance {
                parts: vec![
                    (BalancePart::Paid, money(info.topped_up_balance)),
                    (BalancePart::Promo, money(info.granted_balance)),
                ],
                total: money(info.total_balance),
            }
        })
        .collect();
    // An account can hold CNY and USD; show USD first.
    balances.sort_by_key(|b| b.total.currency != "USD");

    Ok(ProviderSnapshot {
        balances,
        can_make_calls: response.is_available,
        windows: Vec::new(),
        details: Vec::new(),
        notes: if response.is_available {
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

    fn fetch(http: FakeHttp) -> Result<ProviderSnapshot, FetchError> {
        block_on(DeepSeek.fetch(&http, "sk-test", Utc::now()))
    }

    #[test]
    fn parses_balance_with_paid_and_promo_parts() {
        let body = r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"110.00","granted_balance":"10.00","topped_up_balance":"100.00"}]}"#;
        let snapshot = fetch(FakeHttp::default().with(ENDPOINT, 200, body)).unwrap();
        assert!(snapshot.can_make_calls);
        assert!(snapshot.notes.is_empty());
        let balance = &snapshot.balances[0];
        assert_eq!(
            balance.total,
            Money {
                currency: "CNY".into(),
                amount: "110.00".into()
            }
        );
        assert_eq!(
            balance.parts[0],
            (
                BalancePart::Paid,
                Money {
                    currency: "CNY".into(),
                    amount: "100.00".into()
                }
            )
        );
        assert_eq!(balance.parts[1].0, BalancePart::Promo);
    }

    #[test]
    fn usd_comes_first_and_unavailable_is_noted() {
        let body = r#"{"is_available":false,"balance_infos":[
            {"currency":"CNY","total_balance":"0","granted_balance":"0","topped_up_balance":"0"},
            {"currency":"USD","total_balance":"0","granted_balance":"0","topped_up_balance":"0"}]}"#;
        let snapshot = fetch(FakeHttp::default().with(ENDPOINT, 200, body)).unwrap();
        assert_eq!(snapshot.balances[0].total.currency, "USD");
        assert!(!snapshot.can_make_calls);
        assert_eq!(snapshot.notes, vec![Note::CannotMakeCalls]);
    }

    #[test]
    fn maps_errors() {
        assert_eq!(
            fetch(FakeHttp::default().with(ENDPOINT, 401, "")),
            Err(FetchError::Unauthorized)
        );
        assert_eq!(
            fetch(FakeHttp::default().with(ENDPOINT, 500, "")),
            Err(FetchError::Http { status: 500 })
        );
        assert_eq!(
            fetch(FakeHttp::default().with(ENDPOINT, 200, "<html>")),
            Err(FetchError::Decoding)
        );
        assert!(matches!(
            fetch(FakeHttp::default().failing(ENDPOINT)),
            Err(FetchError::Network(_))
        ));
    }
}
