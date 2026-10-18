//! Updates from GitHub Releases. Release builds look for a new version shortly after the start
//! and then every few hours, since redpanda keeps running in the notification area.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_plugin_window_state::AppHandleExt;

use crate::error::{AppError, Result};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub version: String,
    /// Debug builds don't update themselves, they would replace a dev setup with a release.
    pub supported: bool,
    pub available: Option<UpdateInfo>,
}

/// The update found by the last check, kept until the user installs it.
#[derive(Default)]
pub struct Updates(Mutex<Option<Update>>);

fn supported() -> bool {
    !cfg!(debug_assertions)
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    UpdateStatus {
        version: app.package_info().version.to_string(),
        supported: supported(),
        available: pending(app),
    }
}

fn pending(app: &AppHandle) -> Option<UpdateInfo> {
    let updates = app.try_state::<Updates>()?;
    let update = updates.0.lock().ok()?;
    update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
    })
}

/// Asks GitHub for a newer release and emits `update:available` when there is one.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>> {
    if !supported() {
        return Ok(None);
    }
    let update = app
        .updater()
        .map_err(|e| AppError::Other(e.to_string()))?
        .check()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    let Some(update) = update else {
        return Ok(None);
    };
    tracing::info!(version = %update.version, "update available");
    if let Some(updates) = app.try_state::<Updates>() {
        if let Ok(mut slot) = updates.0.lock() {
            *slot = Some(update);
        }
    }
    let info = pending(app);
    if let Some(info) = &info {
        let _ = app.emit("update:available", info);
    }
    Ok(info)
}

pub fn spawn_checks(app: AppHandle) {
    if !supported() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if let Err(e) = check(&app).await {
                tracing::warn!("update check failed: {e}");
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

/// Downloads and installs the pending update. On Windows the installer closes redpanda and
/// starts the new version itself.
pub async fn install(app: &AppHandle) -> Result<()> {
    let update = app
        .try_state::<Updates>()
        .and_then(|u| u.0.lock().ok().and_then(|slot| slot.clone()))
        .ok_or_else(|| AppError::NotFound("update".into()))?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    // The Windows installer ends this process, so save first.
    if let Err(e) = app.save_window_state(crate::window_state_flags()) {
        tracing::warn!("could not save the window state: {e}");
    }
    update
        .install(bytes)
        .map_err(|e| AppError::Other(e.to_string()))?;
    app.restart()
}
