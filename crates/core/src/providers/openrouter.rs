//! OpenRouter: `GET /api/v1/key` describes the key making the request.
//!
//! The account balance (`/api/v1/credits`) needs a management key, which is too powerful to
//! keep in a status app. With a regular key we show what is left of the key's own limit or,
//! for a key without a limit, how much it has spent. Amounts are USD.

use serde::Deserialize;

use super::{
    check_status, Amount, AmountKind, Balance, BalanceError, BalanceProvider, BalanceRequest,
};

pub const ENDPOINT: &str = "https://openrouter.ai/api/v1/key";

pub struct OpenRouter;

#[derive(Deserialize)]
struct Response {
    data: Key,
}

#[derive(Deserialize)]
struct Key {
    limit_remaining: Option<f64>,
    usage: f64,
}

impl BalanceProvider for OpenRouter {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    fn display_name(&self) -> &'static str {
        "OpenRouter"
    }

    fn balance_request(&self, api_key: &str) -> BalanceRequest {
        BalanceRequest {
            url: ENDPOINT.to_owned(),
            headers: vec![
                ("Authorization", format!("Bearer {api_key}")),
                ("Accept", "application/json".to_owned()),
            ],
        }
    }

    fn parse_balance(&self, status: u16, body: &[u8]) -> Result<Balance, BalanceError> {
        check_status(status)?;
        let response: Response =
            serde_json::from_slice(body).map_err(|_| BalanceError::Decoding)?;
        let key = response.data;
        let amount = match key.limit_remaining {
            Some(remaining) => (AmountKind::KeyLimitRemaining, remaining),
            None => (AmountKind::KeySpent, key.usage),
        };
        Ok(Balance {
            is_available: key.limit_remaining.map_or(true, |r| r > 0.0),
            amounts: vec![Amount {
                currency: "USD".into(),
                kind: amount.0,
                total: amount.1.to_string(),
                granted: None,
                topped_up: None,
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_with_a_limit_reports_what_is_left() {
        let body = br#"{"data":{"label":"sk-or-v1-abc","limit":100,"limit_remaining":74.5,"limit_reset":"monthly","usage":25.5,"is_free_tier":false}}"#;
        let balance = OpenRouter.parse_balance(200, body).unwrap();
        assert!(balance.is_available);
        assert_eq!(balance.amounts[0].kind, AmountKind::KeyLimitRemaining);
        assert_eq!(balance.amounts[0].total, "74.5");
    }

    #[test]
    fn key_without_a_limit_reports_spending() {
        let body = br#"{"data":{"label":"x","limit":null,"limit_remaining":null,"usage":3.25}}"#;
        let balance = OpenRouter.parse_balance(200, body).unwrap();
        assert!(balance.is_available);
        assert_eq!(balance.amounts[0].kind, AmountKind::KeySpent);
        assert_eq!(balance.amounts[0].total, "3.25");
    }

    #[test]
    fn exhausted_limit_is_unavailable() {
        let body = br#"{"data":{"limit":10,"limit_remaining":0,"usage":10}}"#;
        assert!(!OpenRouter.parse_balance(200, body).unwrap().is_available);
        assert_eq!(
            OpenRouter.parse_balance(401, b""),
            Err(BalanceError::Unauthorized)
        );
    }
}
