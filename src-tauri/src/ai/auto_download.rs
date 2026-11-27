//! Idle auto-download scheduler (docs/AI_MODEL_AUTO_DOWNLOAD.md).
//!
//! Pre-downloads every downloadable model once the device is idle on Wi-Fi
//! with the hotspot off. OHOS-only by construction: the network and idle
//! conditions are reported by the ArkTS side through `ohos_integration`, and
//! off OHOS both preconditions stay `false`, so the spawned loop exits after
//! its first tick.
//!
//! All downloads reuse the lazy-init pipeline (`download_and_verify_model` +
//! the `ai-model-download-*` events), so a running session is serialized
//! against feature-triggered downloads and manual Settings downloads on the
//! same `ai_init_lock`, and the Settings rows light up with progress without
//! any extra wiring.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::ai::model_registry::{build_info, registry};
use crate::ai::models;

/// Scheduler tick; short enough to react to state pushes within one idle
/// threshold window, long enough to keep the loop free.
const TICK: Duration = Duration::from_secs(30);

/// How long the app must stay backgrounded / window-inactive before a
/// session may start. Screen-off bypasses the threshold entirely.
const IDLE_THRESHOLD_SECS: f64 = 120.0;

/// Hard ceiling for one model download inside a session. Generous enough
/// for a slow CDN pushing the largest asset (~350 MB CLIP), but finite so a
/// dead socket cannot wedge the session (observed on the suspend round).
const PER_FILE_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum AutoDownloadState {
    /// Setting off (or not an OHOS build).
    Disabled,
    /// Wi-Fi down / disconnected, or the device hotspot is on.
    BlockedNetwork,
    /// An AI task is running 鈥?never compete for the CPU with user work.
    BlockedBusy,
    /// Conditions otherwise fine, waiting for the idle threshold.
    WaitingIdle,
    /// Nothing left to download.
    UpToDate,
    /// A session is downloading the pending set.
    Running,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub state: AutoDownloadState,
    pub done: usize,
    pub total: usize,
    pub current: Option<&'static str>,
}

static TASK_HOLD: AtomicBool = AtomicBool::new(false);
static SESSION_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_STATUS: std::sync::Mutex<SessionStatus> = std::sync::Mutex::new(SessionStatus {
    state: AutoDownloadState::WaitingIdle,
    done: 0,
    total: 0,
    current: None,
});

fn publish(app: &AppHandle, status: SessionStatus) {
    *LAST_STATUS.lock().unwrap() = status;
    let _ = app.emit("ai-auto-download", status);
}

/// Latest scheduler verdict for the Settings view (event-replay for late
/// subscribers: the page emits this on mount).
#[tauri::command]
pub fn get_auto_download_status() -> SessionStatus {
    *LAST_STATUS.lock().unwrap()
}

pub(crate) fn pending_entries(
    app: &AppHandle,
) -> Vec<&'static crate::ai::model_registry::RegistryEntry> {
    let Ok(models_dir) = models::get_models_dir(app) else {
        return Vec::new();
    };
    // Bundled entries are already loadable from the HAP; corrupt files are
    // re-downloaded as part of the session (hash mismatch auto-replace).
    registry()
        .iter()
        .filter(|entry| {
            entry.asset.is_some()
                && matches!(
                    build_info(entry, &models_dir).status,
                    crate::ai::model_registry::AiModelStatus::NotInstalled
                        | crate::ai::model_registry::AiModelStatus::Corrupt,
                )
        })
        .collect()
}

fn ai_tasks_running(app: &AppHandle) -> bool {
    app.try_state::<crate::app::state::AppState>()
        .is_some_and(|state| !state.active_ai_tasks.lock().unwrap().is_empty())
}

/// Session-wide "may we keep going" check, re-evaluated between files: the
/// user coming back (idle reset), losing Wi-Fi, enabling the hotspot or
/// switching the setting off all stop the session after the current file.
/// The idle re-check is what the MOR-M1 round proved missing: without it a
/// session that started while backgrounded kept downloading in the
/// foreground after the user returned.
fn conditions_still_ok(app: &AppHandle) -> bool {
    crate::ohos_integration::auto_download_idle_seconds() >= IDLE_THRESHOLD_SECS
        && crate::ohos_integration::auto_download_network_ready()
        && crate::app::settings::load_settings(app.clone())
            .map(|settings| settings.auto_download_models)
            .unwrap_or(false)
}

/// Hold/release the dataTransfer continuous task through the ArkTS bridge.
/// Held only while backgrounded: it exempts the process from the background
/// freezer (without it the scheduler itself dies on phones), but it shows a
/// system notification, so the 2in1 foreground/unfocused path never holds it.
fn hold_continuous_task(hold: bool) {
    if TASK_HOLD.swap(hold, Ordering::AcqRel) == hold {
        return;
    }
    let payload = serde_json::json!({ "op": "auto_download_task", "hold": hold });
    match crate::ohos_integration::dispatch_bridge_event(&payload.to_string()) {
        Ok(()) => log::info!(
            "[auto-download] continuous task {}requested",
            if hold { "" } else { "release " }
        ),
        Err(e) => log::warn!("[auto-download] continuous task bridge failed: {e}"),
    }
}

/// Spawn the scheduler loop; called once from the tauri setup.
pub fn spawn(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if !cfg!(target_env = "ohos") {
            log::info!("[auto-download] not an OpenHarmony build, scheduler idle");
            return;
        }
        log::info!("[auto-download] scheduler running (tick {TICK:?})");
        loop {
            tick(&app_handle).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick(app: &AppHandle) {
    if SESSION_RUNNING.load(Ordering::Acquire) {
        return;
    }

    let enabled = crate::app::settings::load_settings(app.clone())
        .map(|settings| settings.auto_download_models)
        .unwrap_or(false);
    let network = crate::ohos_integration::auto_download_network_ready();
    let backgrounded = crate::ohos_integration::auto_download_backgrounded();
    let idle = crate::ohos_integration::auto_download_idle_seconds();

    if !enabled {
        hold_continuous_task(false);
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::Disabled,
                done: 0,
                total: 0,
                current: None,
            },
        );
        return;
    }
    if !network {
        hold_continuous_task(false);
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::BlockedNetwork,
                done: 0,
                total: 0,
                current: None,
            },
        );
        return;
    }
    if ai_tasks_running(app) {
        hold_continuous_task(false);
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::BlockedBusy,
                done: 0,
                total: 0,
                current: None,
            },
        );
        return;
    }

    let pending = pending_entries(app);
    if pending.is_empty() {
        hold_continuous_task(false);
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::UpToDate,
                done: 0,
                total: 0,
                current: None,
            },
        );
        return;
    }

    if idle < IDLE_THRESHOLD_SECS {
        // Backgrounded + conditions otherwise fine: hold the continuous task
        // NOW so the freezer cannot kill the scheduler before the threshold
        // elapses (phone path). Foreground/unfocused (2in1) needs no task.
        hold_continuous_task(backgrounded);
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::WaitingIdle,
                done: 0,
                total: pending.len(),
                current: None,
            },
        );
        return;
    }

    run_session(app, pending).await;
}

async fn run_session(
    app: &AppHandle,
    pending: Vec<&'static crate::ai::model_registry::RegistryEntry>,
) {
    SESSION_RUNNING.store(true, Ordering::Release);
    // Freezer escape for the backgrounded case; harmless elsewhere.
    hold_continuous_task(true);
    let total = pending.len();
    let mut done = 0usize;
    log::info!("[auto-download] session start: {total} model(s) pending");

    for entry in pending {
        let Some(asset) = entry.asset else { continue };
        if !conditions_still_ok(app) {
            log::info!("[auto-download] conditions changed, stopping after {done}/{total}");
            break;
        }
        publish(
            app,
            SessionStatus {
                state: AutoDownloadState::Running,
                done,
                total,
                current: Some(entry.name),
            },
        );
        let Ok(models_dir) = models::get_models_dir(app) else {
            break;
        };
        let state = app.state::<crate::app::state::AppState>();
        // Per-file lock: user-triggered downloads and AI inits interleave
        // between files instead of waiting for the whole session. The
        // timeout bounds a hung download (the suspend round parked a dead
        // socket inside reqwest for eight minutes) so one stalled file can
        // never wedge SESSION_RUNNING forever.
        let result = {
            let _guard = state.ai_init_lock.lock().await;
            tokio::time::timeout(PER_FILE_TIMEOUT, async {
                models::download_and_verify_model(app, &models_dir, asset).await
            })
            .await
            .unwrap_or_else(|elapsed| {
                Err(anyhow::anyhow!(
                    "model download timed out after {elapsed:?}"
                ))
            })
        };
        match result {
            Ok(()) => log::info!("[auto-download] fetched {}", entry.name),
            Err(e) => log::warn!("[auto-download] {} failed: {e}", entry.name),
        }
        done += 1;
    }

    hold_continuous_task(false);
    SESSION_RUNNING.store(false, Ordering::Release);
    log::info!("[auto-download] session end: {done}/{total} fetched");
    publish(
        app,
        SessionStatus {
            state: AutoDownloadState::UpToDate,
            done,
            total,
            current: None,
        },
    );
}
