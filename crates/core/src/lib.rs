//! Platform-independent core of CreditBar.
//!
//! - [`pricing`]: DeepSeek's peak / off-peak schedule (always evaluated in Beijing time).
//! - [`holidays`]: the mainland China holiday calendar that makes whole days off-peak.
//! - [`peak`]: plain weekday peak hours, for providers without a holiday calendar (Z.AI).
//! - [`providers`]: credit providers, each turning an API key into a common [`ProviderSnapshot`].
//! - [`http`]: the reqwest-backed HTTP client providers use (feature `http`).
//!
//! Nothing here touches UI, storage or platform APIs; the app shells (tray, panel, keyring)
//! live in other crates and call into this one.

pub mod holidays;
#[cfg(feature = "http")]
pub mod http;
pub mod peak;
pub mod pricing;
pub mod providers;

pub use holidays::HolidaySchedule;
pub use pricing::{PricePeriod, PricingSnapshot};
pub use providers::{FetchError, Provider, ProviderSnapshot};
