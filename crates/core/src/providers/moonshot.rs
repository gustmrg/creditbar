//! Kimi (Moonshot AI), international platform: `GET /v1/users/me/balance`, amounts in USD.
//! Keys from the China platform (api.moonshot.cn) are separate and get a 401 here.

use serde::Deserialize;

use super::{
    check_status, Amount, AmountKind, Balance, BalanceError, BalanceProvider, BalanceRequest,
};

pub const ENDPOINT: &str = "https://api.moonshot.ai/v1/users/me/balance";

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

impl BalanceProvider for Moonshot {
    fn id(&self) -> &'static str {
        "moonshot"
    }

    fn display_name(&self) -> &'static str {
        "Kimi (Moonshot)"
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
        if response.code != 0 {
            return Err(BalanceError::Decoding);
        }
        let data = response.data;
        Ok(Balance {
            is_available: data.available_balance > 0.0,
            amounts: vec![Amount {
                currency: "USD".into(),
                kind: AmountKind::AccountBalance,
                total: data.available_balance.to_string(),
                granted: Some(data.voucher_balance.to_string()),
                topped_up: Some(data.cash_balance.to_string()),
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_balance() {
        let body = br#"{"code":0,"data":{"available_balance":49.58894,"voucher_balance":46.58893,"cash_balance":3.00001},"scode":"0x0","status":true}"#;
        let balance = Moonshot.parse_balance(200, body).unwrap();
        assert!(balance.is_available);
        assert_eq!(balance.amounts[0].total, "49.58894");
        assert_eq!(balance.amounts[0].granted.as_deref(), Some("46.58893"));
        assert_eq!(balance.amounts[0].topped_up.as_deref(), Some("3.00001"));
    }

    #[test]
    fn owing_money_is_unavailable() {
        let body =
            br#"{"code":0,"data":{"available_balance":0,"voucher_balance":0,"cash_balance":-1.5}}"#;
        assert!(!Moonshot.parse_balance(200, body).unwrap().is_available);
    }

    #[test]
    fn maps_errors() {
        assert_eq!(
            Moonshot.parse_balance(401, b""),
            Err(BalanceError::Unauthorized)
        );
        let failed =
            br#"{"code":5,"data":{"available_balance":0,"voucher_balance":0,"cash_balance":0}}"#;
        assert_eq!(
            Moonshot.parse_balance(200, failed),
            Err(BalanceError::Decoding)
        );
    }
}
