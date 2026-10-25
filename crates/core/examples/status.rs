//! Prints the current pricing snapshot as JSON, and the DeepSeek balance when
//! `DEEPSEEK_API_KEY` is set.
//!
//!     cargo run -p creditbar-core --example status

use chrono::Utc;
use creditbar_core::{pricing, providers, HolidaySchedule, PricingSnapshot};

#[tokio::main]
async fn main() {
    let schedule = HolidaySchedule::bundled();
    let snapshot = PricingSnapshot::new(Utc::now(), &schedule);
    println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
    println!(
        "countdown: {}",
        pricing::compact_countdown(snapshot.seconds_until_transition())
    );

    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        let provider = providers::by_id("deepseek").unwrap();
        let client = reqwest::Client::new();
        match creditbar_core::http::fetch_balance(&client, provider.as_ref(), &key).await {
            Ok(balance) => println!("{}", serde_json::to_string_pretty(&balance).unwrap()),
            Err(error) => eprintln!("balance: {error}"),
        }
    }
}
