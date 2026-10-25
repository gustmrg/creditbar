//! DeepSeek: `GET /user/balance` with `Authorization: Bearer <key>`.
//! The query itself costs nothing.

use serde::Deserialize;

use super::{
    check_status, Amount, AmountKind, Balance, BalanceError, BalanceProvider, BalanceRequest,
};

pub const ENDPOINT: &str = "https://api.deepseek.com/user/balance";

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

impl BalanceProvider for DeepSeek {
    fn id(&self) -> &'static str {
        "deepseek"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek"
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
        Ok(Balance {
            is_available: response.is_available,
            amounts: response
                .balance_infos
                .into_iter()
                .map(|info| Amount {
                    currency: info.currency,
                    kind: AmountKind::AccountBalance,
                    total: info.total_balance,
                    granted: Some(info.granted_balance),
                    topped_up: Some(info.topped_up_balance),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "is_available": true,
        "balance_infos": [{
            "currency": "CNY",
            "total_balance": "110.00",
            "granted_balance": "10.00",
            "topped_up_balance": "100.00"
        }]
    }"#;

    #[test]
    fn request_sends_bearer_key() {
        let request = DeepSeek.balance_request("sk-test");
        assert_eq!(request.url, ENDPOINT);
        assert!(request
            .headers
            .contains(&("Authorization", "Bearer sk-test".to_owned())));
    }

    #[test]
    fn parses_balance() {
        let balance = DeepSeek.parse_balance(200, SAMPLE.as_bytes()).unwrap();
        assert!(balance.is_available);
        assert_eq!(
            balance.amounts,
            vec![Amount {
                currency: "CNY".into(),
                kind: AmountKind::AccountBalance,
                total: "110.00".into(),
                granted: Some("10.00".into()),
                topped_up: Some("100.00".into()),
            }]
        );
    }

    #[test]
    fn maps_errors() {
        assert_eq!(
            DeepSeek.parse_balance(401, b""),
            Err(BalanceError::Unauthorized)
        );
        assert_eq!(
            DeepSeek.parse_balance(403, b""),
            Err(BalanceError::Unauthorized)
        );
        assert_eq!(
            DeepSeek.parse_balance(500, b""),
            Err(BalanceError::Http { status: 500 })
        );
        assert_eq!(
            DeepSeek.parse_balance(200, b"<html>"),
            Err(BalanceError::Decoding)
        );
    }
}
