//! Performs balance requests described by providers.

use std::time::Duration;

use crate::providers::{Balance, BalanceError, BalanceProvider};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Fetches a provider's balance. Reuse one `client` for the whole app.
///
/// reqwest drops the `Authorization` header on cross-host redirects, so a key never leaks
/// to another domain.
pub async fn fetch_balance(
    client: &reqwest::Client,
    provider: &dyn BalanceProvider,
    api_key: &str,
) -> Result<Balance, BalanceError> {
    let request = provider.balance_request(api_key);
    let mut builder = client.get(&request.url).timeout(REQUEST_TIMEOUT);
    for (name, value) in &request.headers {
        builder = builder.header(*name, value);
    }

    let network = |e: reqwest::Error| BalanceError::Network(e.to_string());
    let response = builder.send().await.map_err(network)?;
    let status = response.status().as_u16();
    let body = response.bytes().await.map_err(network)?;
    provider.parse_balance(status, &body)
}
