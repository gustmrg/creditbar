//! Account balance providers.
//!
//! A provider only describes *what* to request and *how* to read the answer; performing the
//! request is [`crate::http::fetch_balance`]'s job. That keeps providers trivially testable
//! and object-safe, so the app can hold a `Vec<Box<dyn BalanceProvider>>`.

pub mod deepseek;
pub mod moonshot;
pub mod openrouter;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One balance request. All providers so far are a single authenticated GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    /// Whether the account can still make API calls. Prefer this over checking `total > 0`.
    pub is_available: bool,
    /// One entry per currency.
    pub amounts: Vec<Amount>,
}

/// Amounts are kept as the provider's decimal strings; they are only ever displayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Amount {
    /// ISO 4217 code, e.g. `CNY`, `USD`.
    pub currency: String,
    /// What `total` measures. Not every provider exposes the account balance to an API key.
    pub kind: AmountKind,
    pub total: String,
    /// Promotional credit, when the provider reports it separately.
    pub granted: Option<String>,
    /// Paid credit, when the provider reports it separately.
    pub topped_up: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AmountKind {
    /// Money left on the account.
    AccountBalance,
    /// What is left of this API key's own spending limit.
    KeyLimitRemaining,
    /// Spent so far with this API key; the provider reports nothing about what is left.
    KeySpent,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BalanceError {
    #[error("the API key is invalid or expired")]
    Unauthorized,
    #[error("the provider returned HTTP {status}")]
    Http { status: u16 },
    #[error("couldn't reach the provider: {0}")]
    Network(String),
    #[error("the provider returned an unexpected response")]
    Decoding,
}

impl BalanceError {
    /// The UI offers "Replace API key" prominently for these.
    pub fn suggests_replacing_key(&self) -> bool {
        matches!(self, BalanceError::Unauthorized)
    }
}

pub trait BalanceProvider: Send + Sync {
    /// Stable identifier, used as the keyring account and in settings.
    fn id(&self) -> &'static str;

    fn display_name(&self) -> &'static str;

    fn balance_request(&self, api_key: &str) -> BalanceRequest;

    /// Interprets the response. `status` is the HTTP status code.
    fn parse_balance(&self, status: u16, body: &[u8]) -> Result<Balance, BalanceError>;
}

/// The common status handling: 200 is success, 401/403 mean a bad key.
pub fn check_status(status: u16) -> Result<(), BalanceError> {
    match status {
        200 => Ok(()),
        401 | 403 => Err(BalanceError::Unauthorized),
        status => Err(BalanceError::Http { status }),
    }
}

/// Every provider the app knows about.
pub fn all() -> Vec<Box<dyn BalanceProvider>> {
    vec![
        Box::new(deepseek::DeepSeek),
        Box::new(moonshot::Moonshot),
        Box::new(openrouter::OpenRouter),
    ]
}

pub fn by_id(id: &str) -> Option<Box<dyn BalanceProvider>> {
    all().into_iter().find(|p| p.id() == id)
}
