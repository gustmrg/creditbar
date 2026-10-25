//! Holiday calendar: cached on disk, refreshed from Apple once a day, bundled fallback.

use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use status_core::holidays::{self, HolidaySchedule};
use status_core::PricingSnapshot;
use tauri::{AppHandle, Manager};

const CACHE_FILE: &str = "holiday-schedule.json";

#[derive(Serialize, Deserialize)]
struct Cache {
    etag: Option<String>,
    schedule: HolidaySchedule,
}

pub struct Holidays {
    schedule: RwLock<HolidaySchedule>,
    etag: RwLock<Option<String>>,
    cache_path: Option<PathBuf>,
}

impl Holidays {
    pub fn load(app: &AppHandle) -> Self {
        let cache_path = app
            .path()
            .app_cache_dir()
            .ok()
            .map(|dir| dir.join(CACHE_FILE));
        let cache = cache_path
            .as_ref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|data| serde_json::from_slice::<Cache>(&data).ok())
            .filter(|cache| cache.schedule.newest_covered_year().is_some());
        let (schedule, etag) = match cache {
            Some(cache) => (cache.schedule, cache.etag),
            None => (HolidaySchedule::bundled(), None),
        };
        Holidays {
            schedule: RwLock::new(schedule),
            etag: RwLock::new(etag),
            cache_path,
        }
    }

    pub fn snapshot_now(&self) -> PricingSnapshot {
        PricingSnapshot::new(Utc::now(), &self.schedule.read().unwrap())
    }

    pub fn with_schedule<R>(&self, f: impl FnOnce(&HolidaySchedule) -> R) -> R {
        f(&self.schedule.read().unwrap())
    }

    /// Conditional GET; on any failure the current data stays in effect.
    async fn refresh(&self, client: &reqwest::Client) -> Result<(), String> {
        let mut request = client
            .get(holidays::CALENDAR_URL)
            .timeout(Duration::from_secs(20));
        if let Some(etag) = self.etag.read().unwrap().clone() {
            request = request.header("If-None-Match", etag);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        match response.status().as_u16() {
            304 => return Ok(()),
            200 => {}
            status => return Err(format!("HTTP {status}")),
        }
        let etag = response
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await.map_err(|e| e.to_string())?;
        let parsed = HolidaySchedule::parse_ics(&body, Utc::now()).map_err(|e| e.to_string())?;
        if parsed.is_regression_from(&self.schedule.read().unwrap()) {
            return Ok(());
        }

        if let Some(path) = &self.cache_path {
            let cache = Cache {
                etag: etag.clone(),
                schedule: parsed.clone(),
            };
            let _ = fs::create_dir_all(path.parent().unwrap())
                .and_then(|_| fs::write(path, serde_json::to_vec(&cache).unwrap()));
        }
        *self.schedule.write().unwrap() = parsed;
        *self.etag.write().unwrap() = etag;
        Ok(())
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        let mut every_day =
            tokio::time::interval(Duration::from_secs(holidays::REFRESH_INTERVAL_SECS));
        loop {
            every_day.tick().await;
            if let Err(error) = app.state::<Holidays>().refresh(&client).await {
                eprintln!("holiday calendar refresh failed: {error}");
            }
        }
    });
}
