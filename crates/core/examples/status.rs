//! Prints the current DeepSeek pricing snapshot as JSON, and each provider's snapshot for which
//! an API key is set in the environment: `DEEPSEEK_API_KEY`, `MOONSHOT_API_KEY`,
//! `OPENROUTER_API_KEY`.
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

    let client = reqwest::Client::new();
    for provider in providers::all() {
        let variable = format!("{}_API_KEY", provider.info().id.to_uppercase());
        let Ok(key) = std::env::var(&variable) else {
            continue;
        };
        match provider.fetch(&client, &key, Utc::now()).await {
            Ok(snapshot) => println!(
                "{}: {}",
                provider.info().name,
                serde_json::to_string_pretty(&snapshot).unwrap()
            ),
            Err(error) => eprintln!("{}: {error}", provider.info().name),
        }
    }
}
