//! Platform-independent core of DeepSeek Status.
//!
//! - [`pricing`]: DeepSeek's peak / off-peak schedule (always evaluated in Beijing time).
//! - [`holidays`]: the mainland China holiday calendar that makes whole days off-peak.
//! - [`providers`]: account balance providers behind a common [`providers::BalanceProvider`] trait.
//! - [`http`]: performs a provider's balance request (feature `http`).
//!
//! Nothing here touches UI, storage or platform APIs; the app shells (tray, panel, keyring)
//! live in other crates and call into this one.

pub mod holidays;
#[cfg(feature = "http")]
pub mod http;
pub mod pricing;
pub mod providers;

pub use holidays::HolidaySchedule;
pub use pricing::{PricePeriod, PricingSnapshot};
pub use providers::{Balance, BalanceError, BalanceProvider};
