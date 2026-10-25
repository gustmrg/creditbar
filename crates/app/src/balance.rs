//! Balance state per provider. API keys live in the OS keyring (Keychain, Credential Manager,
//! Secret Service) and never reach the panel; it only learns whether a key exists.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use status_core::providers::{self, Balance, BalanceProvider};
use tauri::{AppHandle, Emitter, Manager};

const KEYRING_SERVICE: &str = "dev.gustmrg.deepseekstatus";
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceView {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    pub has_key: bool,
    pub refreshing: bool,
    /// The last successful balance; kept while refreshing or after a failure.
    pub balance: Option<Balance>,
    pub error: Option<String>,
    pub suggests_replacing_key: bool,
    pub last_refreshed: Option<DateTime<Utc>>,
}

struct Slot {
    provider: Arc<dyn BalanceProvider>,
    key: Option<String>,
    /// Bumped by every request and key change; a response from an older generation is dropped.
    generation: u64,
    view: BalanceView,
}

pub struct Balances {
    slots: Mutex<Vec<Slot>>,
    http: reqwest::Client,
}

impl Balances {
    pub fn new() -> Self {
        let slots = providers::all()
            .into_iter()
            .map(|provider| {
                let provider: Arc<dyn BalanceProvider> = Arc::from(provider);
                let view = BalanceView {
                    provider_id: provider.id(),
                    display_name: provider.display_name(),
                    has_key: false,
                    refreshing: false,
                    balance: None,
                    error: None,
                    suggests_replacing_key: false,
                    last_refreshed: None,
                };
                Slot {
                    provider,
                    key: None,
                    generation: 0,
                    view,
                }
            })
            .collect();
        Balances {
            slots: Mutex::new(slots),
            http: reqwest::Client::new(),
        }
    }

    pub fn views(&self) -> Vec<BalanceView> {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.view.clone())
            .collect()
    }

    fn with_slot<R>(&self, id: &str, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
        self.slots
            .lock()
            .unwrap()
            .iter_mut()
            .find(|s| s.provider.id() == id)
            .map(f)
    }

    fn ids(&self) -> Vec<&'static str> {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.provider.id())
            .collect()
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit("balances", app.state::<Balances>().views());
}

fn keyring_entry(provider_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, provider_id).map_err(|e| e.to_string())
}

/// Keyring calls may block (unlock prompts, D-Bus), so they run off the async workers.
async fn blocking<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .expect("keyring task panicked")
}

/// Loads the saved keys, then refreshes every 5 minutes.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for id in app.state::<Balances>().ids() {
            let key = blocking(move || keyring_entry(id).ok()?.get_password().ok()).await;
            app.state::<Balances>().with_slot(id, |slot| {
                slot.view.has_key = key.is_some();
                slot.key = key;
            });
        }
        emit(&app);

        let mut every = tokio::time::interval(REFRESH_INTERVAL);
        loop {
            every.tick().await;
            for id in app.state::<Balances>().ids() {
                refresh(app.clone(), id.to_owned(), false).await;
            }
        }
    });
}

/// Refreshes balances older than the refresh interval (called when the panel opens).
pub fn refresh_if_stale(app: &AppHandle) {
    let stale: Vec<&'static str> = {
        let slots = app
            .state::<Balances>()
            .slots
            .lock()
            .unwrap()
            .iter()
            .filter(|s| {
                s.view.has_key
                    && !s.view.refreshing
                    && s.view.last_refreshed.map_or(true, |at| {
                        (Utc::now() - at).to_std().unwrap_or_default() >= REFRESH_INTERVAL
                    })
            })
            .map(|s| s.provider.id())
            .collect();
        slots
    };
    for id in stale {
        tauri::async_runtime::spawn(refresh(app.clone(), id.to_owned(), false));
    }
}

/// `force` supersedes a request in flight (a new key must be checked right away).
pub async fn refresh(app: AppHandle, provider_id: String, force: bool) {
    let balances = app.state::<Balances>();
    let started = balances.with_slot(&provider_id, |slot| {
        let key = slot.key.clone()?;
        if slot.view.refreshing && !force {
            return None;
        }
        slot.generation += 1;
        slot.view.refreshing = true;
        Some((slot.provider.clone(), key, slot.generation))
    });
    let Some(Some((provider, key, generation))) = started else {
        return;
    };
    emit(&app);

    let result = status_core::http::fetch_balance(&balances.http, provider.as_ref(), &key).await;

    let current = balances.with_slot(&provider_id, |slot| {
        if slot.generation != generation {
            return false;
        }
        slot.view.refreshing = false;
        match result {
            Ok(balance) => {
                slot.view.balance = Some(balance);
                slot.view.error = None;
                slot.view.suggests_replacing_key = false;
                slot.view.last_refreshed = Some(Utc::now());
            }
            Err(error) => {
                slot.view.suggests_replacing_key = error.suggests_replacing_key();
                slot.view.error = Some(error.to_string());
            }
        }
        true
    });
    if current == Some(true) {
        emit(&app);
    }
}

pub async fn save_key(app: AppHandle, provider_id: String, key: String) -> Result<(), String> {
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err("Please enter an API key.".into());
    }
    let id = provider_id.clone();
    let stored = key.clone();
    blocking(move || {
        keyring_entry(&id)?
            .set_password(&stored)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Couldn't save the API key: {e}"))?;

    app.state::<Balances>().with_slot(&provider_id, |slot| {
        slot.key = Some(key);
        slot.view.has_key = true;
        slot.view.balance = None;
        slot.view.error = None;
        slot.view.last_refreshed = None;
    });
    refresh(app, provider_id, true).await;
    Ok(())
}

pub async fn remove_key(app: AppHandle, provider_id: String) -> Result<(), String> {
    let id = provider_id.clone();
    blocking(move || match keyring_entry(&id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    })
    .await?;

    app.state::<Balances>().with_slot(&provider_id, |slot| {
        slot.key = None;
        slot.generation += 1;
        slot.view.has_key = false;
        slot.view.refreshing = false;
        slot.view.balance = None;
        slot.view.error = None;
        slot.view.suggests_replacing_key = false;
        slot.view.last_refreshed = None;
    });
    emit(&app);
    Ok(())
}
