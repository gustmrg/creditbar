//! State per provider: key, latest snapshot, refresh. API keys live in the OS keyring
//! (Keychain, Credential Manager, Secret Service) and never reach the panel; it only learns
//! whether a key exists.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use creditbar_core::peak::PeakStatus;
use creditbar_core::providers::{self, FetchError, Provider, ProviderInfo, ProviderSnapshot};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::i18n::{t, tf};

const KEYRING_SERVICE: &str = "dev.gustmrg.creditbar";
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// A quota this far used turns the tray icon amber.
const ATTENTION_USED_PERCENT: f64 = 90.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub info: ProviderInfo,
    pub has_key: bool,
    pub refreshing: bool,
    /// The last successful snapshot; kept while refreshing or after a failure.
    pub snapshot: Option<ProviderSnapshot>,
    pub error: Option<String>,
    pub suggests_replacing_key: bool,
    pub last_refreshed: Option<DateTime<Utc>>,
    /// Where now falls in the provider's peak hours, if it has them. Computed per view, so the
    /// panel reloads when `next_transition` passes.
    pub peak: Option<PeakStatus>,
}

struct Slot {
    provider: Arc<dyn Provider>,
    key: Option<String>,
    /// Bumped by every request and key change; a response from an older generation is dropped.
    generation: u64,
    view: ProviderView,
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
                let provider: Arc<dyn Provider> = Arc::from(provider);
                let view = ProviderView {
                    info: *provider.info(),
                    has_key: false,
                    refreshing: false,
                    snapshot: None,
                    error: None,
                    suggests_replacing_key: false,
                    last_refreshed: None,
                    peak: None,
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

    /// A saved key whose last refresh failed, an account that can't make API calls, or a quota
    /// nearly used up.
    pub fn needs_attention(&self) -> bool {
        self.slots.lock().unwrap().iter().any(|s| {
            s.view.has_key
                && (s.view.error.is_some()
                    || s.view.snapshot.as_ref().is_some_and(|snapshot| {
                        !snapshot.can_make_calls
                            || snapshot
                                .windows
                                .iter()
                                .any(|w| w.used_percent >= ATTENTION_USED_PERCENT)
                    }))
        })
    }

    pub fn views(&self) -> Vec<ProviderView> {
        let now = Utc::now();
        self.slots
            .lock()
            .unwrap()
            .iter()
            .map(|s| ProviderView {
                peak: s.view.info.peak_hours.map(|hours| hours.status(now)),
                ..s.view.clone()
            })
            .collect()
    }

    fn with_slot<R>(&self, id: &str, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
        self.slots
            .lock()
            .unwrap()
            .iter_mut()
            .find(|s| s.provider.info().id == id)
            .map(f)
    }

    fn ids(&self) -> Vec<&'static str> {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.provider.info().id)
            .collect()
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit("providers", app.state::<Balances>().views());
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
                    && s.view.last_refreshed.is_none_or(|at| {
                        (Utc::now() - at).to_std().unwrap_or_default() >= REFRESH_INTERVAL
                    })
            })
            .map(|s| s.provider.info().id)
            .collect();
        slots
    };
    for id in stale {
        tauri::async_runtime::spawn(refresh(app.clone(), id.to_owned(), false));
    }
}

fn error_text(error: &FetchError, provider: &str) -> String {
    match error {
        FetchError::Unauthorized => t("error.unauthorized"),
        FetchError::Http { status } => tf(
            "error.http",
            &[("provider", provider), ("status", &status.to_string())],
        ),
        FetchError::Network(detail) => tf(
            "error.network",
            &[("provider", provider), ("detail", detail)],
        ),
        FetchError::Decoding => tf("error.decoding", &[("provider", provider)]),
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

    let result = provider.fetch(&balances.http, &key, Utc::now()).await;

    let current = balances.with_slot(&provider_id, |slot| {
        if slot.generation != generation {
            return false;
        }
        slot.view.refreshing = false;
        match result {
            Ok(snapshot) => {
                slot.view.snapshot = Some(snapshot);
                slot.view.error = None;
                slot.view.suggests_replacing_key = false;
                slot.view.last_refreshed = Some(Utc::now());
            }
            Err(error) => {
                slot.view.suggests_replacing_key = error.suggests_replacing_key();
                slot.view.error = Some(error_text(&error, provider.info().name));
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
        return Err(t("error.keyEmpty"));
    }
    let id = provider_id.clone();
    let stored = key.clone();
    blocking(move || {
        keyring_entry(&id)?
            .set_password(&stored)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| tf("error.keySave", &[("detail", &e)]))?;

    app.state::<Balances>().with_slot(&provider_id, |slot| {
        slot.key = Some(key);
        slot.view.has_key = true;
        slot.view.snapshot = None;
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
    .await
    .map_err(|e| tf("error.keyRemove", &[("detail", &e)]))?;

    app.state::<Balances>().with_slot(&provider_id, |slot| {
        slot.key = None;
        slot.generation += 1;
        slot.view.has_key = false;
        slot.view.refreshing = false;
        slot.view.snapshot = None;
        slot.view.error = None;
        slot.view.suggests_replacing_key = false;
        slot.view.last_refreshed = None;
    });
    emit(&app);
    Ok(())
}
