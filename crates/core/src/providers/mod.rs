//! Credit providers.
//!
//! Every provider turns an API key into the same [`ProviderSnapshot`], so the panel renders all
//! of them with one card and no provider-specific branches. What a figure *means* travels as a
//! kind (`BalancePart::Paid`, `WindowKind::KeyLimit`, …) that the UI translates.
//!
//! Providers talk to the network only through [`HttpClient`], which keeps them testable with
//! canned responses (see `tests` in each provider module).

pub mod deepseek;
pub mod moonshot;
pub mod openrouter;

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// MARK: - Descriptor

/// Static facts about a provider: identity, links, branding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    /// Stable identifier, used as the keyring account and in settings.
    pub id: &'static str,
    pub name: &'static str,
    /// Where the user manages billing.
    pub dashboard_url: &'static str,
    /// Where the user creates an API key.
    pub keys_url: &'static str,
    pub key_placeholder: &'static str,
    /// Accent colour for the provider's row, `#RRGGBB`.
    pub color: &'static str,
}

// MARK: - Snapshot

/// Everything a provider reports, in a shape the UI renders generically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    /// Money left on the account, one entry per currency, the preferred one first. Empty when
    /// the provider doesn't reveal the account balance to this key.
    pub balances: Vec<AccountBalance>,
    /// Whether the account can make API calls right now.
    pub can_make_calls: bool,
    /// Quotas that reset on a schedule.
    pub windows: Vec<UsageWindow>,
    pub details: Vec<Detail>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Money {
    /// ISO 4217 code, e.g. `CNY`, `USD`.
    pub currency: String,
    /// Decimal string as the provider reported it; only ever displayed.
    pub amount: String,
}

impl Money {
    pub fn usd(amount: f64) -> Self {
        Money {
            currency: "USD".into(),
            amount: amount.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountBalance {
    pub total: Money,
    /// How the total splits, when the provider says.
    pub parts: Vec<(BalancePart, Money)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BalancePart {
    /// Money the user paid in.
    Paid,
    /// Granted, voucher or promotional credit.
    Promo,
}

/// A spending allowance that resets, e.g. an OpenRouter key's monthly limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub kind: WindowKind,
    pub used: Money,
    pub limit: Money,
    /// 0–100.
    pub used_percent: f64,
    pub reset: Option<ResetPeriod>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowKind {
    /// A spending cap set on the API key itself, independent of the account balance.
    KeyLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetPeriod {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub kind: DetailKind,
    pub value: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DetailKind {
    SpentToday,
    SpentThisWeek,
    SpentThisMonth,
    /// Everything this API key has spent.
    SpentWithKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Note {
    /// The account can't make API calls (no balance, or the key's limit is used up).
    CannotMakeCalls,
    /// The account owes money.
    Deficit { amount: Money },
    /// The key has no spending limit, so there is no quota to show.
    NoKeyLimit,
    /// The account balance needs a different kind of key than the one saved.
    BalanceNeedsManagementKey,
    /// The account balance request failed while the rest succeeded.
    BalanceUnavailable,
}

// MARK: - Fetching

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FetchError {
    #[error("the API key is invalid or expired")]
    Unauthorized,
    #[error("the provider returned HTTP {status}")]
    Http { status: u16 },
    #[error("couldn't reach the provider: {0}")]
    Network(String),
    #[error("the provider returned an unexpected response")]
    Decoding,
}

impl FetchError {
    /// The UI offers "Replace API key" prominently for these.
    pub fn suggests_replacing_key(&self) -> bool {
        matches!(self, FetchError::Unauthorized)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// The only way providers reach the network. Implemented for `reqwest::Client` (feature
/// `http`) and by canned responses in tests.
pub trait HttpClient: Send + Sync {
    /// A GET. `Err` is a transport failure; HTTP error statuses come back as responses.
    fn get<'a>(
        &'a self,
        url: &'a str,
        headers: Vec<(&'static str, String)>,
    ) -> BoxFuture<'a, Result<HttpResponse, String>>;
}

pub trait Provider: Send + Sync {
    fn info(&self) -> &'static ProviderInfo;

    /// Fetches the snapshot for `api_key`. `now` anchors computed reset times.
    fn fetch<'a>(
        &'a self,
        http: &'a dyn HttpClient,
        api_key: &'a str,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<ProviderSnapshot, FetchError>>;
}

/// `Authorization: Bearer` plus JSON, which every provider so far uses.
pub(crate) fn bearer_headers(api_key: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {api_key}")),
        ("Accept", "application/json".to_owned()),
    ]
}

/// GET that must succeed: 401/403 mean a bad key, other non-200s are errors.
pub(crate) async fn get_ok(
    http: &dyn HttpClient,
    url: &str,
    api_key: &str,
) -> Result<Vec<u8>, FetchError> {
    let response = http
        .get(url, bearer_headers(api_key))
        .await
        .map_err(FetchError::Network)?;
    match response.status {
        200 => Ok(response.body),
        401 | 403 => Err(FetchError::Unauthorized),
        status => Err(FetchError::Http { status }),
    }
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, FetchError> {
    serde_json::from_slice(body).map_err(|_| FetchError::Decoding)
}

/// Every provider the app knows about, in display order.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(deepseek::DeepSeek),
        Box::new(moonshot::Moonshot),
        Box::new(openrouter::OpenRouter),
    ]
}

pub fn by_id(id: &str) -> Option<Box<dyn Provider>> {
    all().into_iter().find(|p| p.info().id == id)
}

#[cfg(test)]
pub(crate) mod testing {
    //! Canned HTTP responses keyed by URL.

    use std::collections::HashMap;

    use super::{BoxFuture, HttpClient, HttpResponse};

    #[derive(Default)]
    pub struct FakeHttp {
        responses: HashMap<String, Result<HttpResponse, String>>,
    }

    impl FakeHttp {
        pub fn with(mut self, url: &str, status: u16, body: &str) -> Self {
            let response = HttpResponse {
                status,
                body: body.as_bytes().to_vec(),
            };
            self.responses.insert(url.to_owned(), Ok(response));
            self
        }

        pub fn failing(mut self, url: &str) -> Self {
            self.responses
                .insert(url.to_owned(), Err("connection refused".into()));
            self
        }
    }

    impl HttpClient for FakeHttp {
        fn get<'a>(
            &'a self,
            url: &'a str,
            headers: Vec<(&'static str, String)>,
        ) -> BoxFuture<'a, Result<HttpResponse, String>> {
            assert!(
                headers
                    .iter()
                    .any(|(name, value)| *name == "Authorization" && value.starts_with("Bearer ")),
                "every request carries the key"
            );
            let response = self
                .responses
                .get(url)
                .cloned()
                .unwrap_or_else(|| panic!("unexpected GET {url}"));
            Box::pin(async move { response })
        }
    }

    /// Runs a provider future to completion (providers only await the fake client).
    pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut future = std::pin::pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
        }
    }
}
