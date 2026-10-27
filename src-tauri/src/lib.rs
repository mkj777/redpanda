pub mod accounts;
mod commands;
pub mod core;
pub mod error;
pub mod mcp;
pub mod mime;
pub mod model;
pub mod send;
pub mod service;
pub mod store;
pub mod sync;
mod pdf;
mod titlebar;
pub(crate) mod tray;
pub mod unsubscribe;
mod update;

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_window_state::StateFlags;

use crate::accounts::oauth::OAuthConfig;
use crate::accounts::secrets::Secrets;
use crate::core::{Core, EventSink};
use crate::service::App;
use crate::store::Db;

struct TauriEvents(AppHandle);

impl EventSink for TauriEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Err(e) = self.0.emit(event, payload) {
            tracing::warn!("could not emit {event}: {e}");
        }
        match event {
            "mail:changed" => tray::update_tooltip(&self.0),
            "mcp:changed" => tray::set_mcp_checked(&self.0),
            _ => {}
        }
    }
}

/// The window is undecorated on purpose, a saved state from before must not bring the frame
/// back. Visibility is left to the start, so an autostart stays in the notification area.
pub(crate) fn window_state_flags() -> StateFlags {
    StateFlags::all() - StateFlags::DECORATIONS - StateFlags::VISIBLE
}

/// Passed by the Windows autostart entry.
const AUTOSTART_ARG: &str = "--autostart";

const DB_FILES: [&str; 3] = ["redpanda.db", "redpanda.db-wal", "redpanda.db-shm"];

/// Versions before 0.1.0 kept the database in `%LOCALAPPDATA%\redpanda`, which is also where the
/// installer puts the program. Moves it once into the app's own data folder.
fn move_legacy_db(old: &Path, new: &Path) -> std::io::Result<()> {
    if new.join(DB_FILES[0]).exists() || !old.join(DB_FILES[0]).exists() {
        return Ok(());
    }
    std::fs::create_dir_all(new)?;
    for name in DB_FILES {
        let from = old.join(name);
        if from.exists() {
            std::fs::rename(&from, new.join(name))?;
        }
    }
    tracing::info!("moved the database to the app data folder");
    Ok(())
}

/// Keeps the Windows autostart entry in line with the setting. Debug builds never register
/// themselves.
pub(crate) fn apply_autostart(app: &AppHandle, on: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    let launcher = app.autolaunch();
    let result = match (on, launcher.is_enabled().unwrap_or(false)) {
        // Enabling again also updates the path after a reinstall.
        (true, _) => launcher.enable(),
        (false, true) => launcher.disable(),
        (false, false) => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!("could not update the autostart entry: {e}");
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("REDPANDA_LOG")
                .unwrap_or_else(|_| "redpanda_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        // First, so a second start only brings the running window back and never opens the
        // database a second time.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show_main(app)
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            if let Some(old) = dirs::data_local_dir().map(|d| d.join("redpanda")) {
                move_legacy_db(&old, &dir)?;
            }
            let db = Db::open(&dir.join(DB_FILES[0]))?;
            let secrets = Secrets::load_or_create()?;
            let events = Arc::new(TauriEvents(app.handle().clone()));
            let core = Arc::new(Core::new(db, secrets, OAuthConfig::load(), events));
            let state = Arc::new(App::new(core));
            // Workers use tokio::spawn, so start them inside Tauri's runtime.
            tauri::async_runtime::block_on(async { state.sync.start_all() })?;
            let mcp = mcp::Mcp::new(state.clone());
            app.manage(state);
            app.manage(mcp.clone());
            // Only listens when the user switched it on in the settings.
            tauri::async_runtime::spawn(async move { mcp.start().await });
            if let Some(window) = app.get_webview_window("main") {
                titlebar::install(app.handle(), &window);
            }
            tray::install(app.handle())?;
            let settings = app.state::<Arc<App>>().app_settings()?;
            apply_autostart(app.handle(), settings.autostart);
            // Hidden in the config, so a start with Windows never flashes the window.
            if !std::env::args().any(|a| a == AUTOSTART_ARG) {
                tray::show_main(app.handle());
            }
            app.manage(update::Updates::default());
            update::spawn_checks(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => titlebar::place(),
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                let to_tray = app
                    .try_state::<Arc<App>>()
                    .and_then(|s| s.app_settings().ok())
                    .is_none_or(|s| s.close_to_tray);
                if to_tray {
                    api.prevent_close();
                    tray::hide_main(app);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::discover,
            commands::oauth_status,
            commands::add_account,
            commands::reauth_account,
            commands::cancel_oauth,
            commands::update_account,
            commands::remove_account,
            commands::list_folders,
            commands::list_messages,
            commands::open_folder,
            commands::sync_now,
            commands::load_older,
            commands::get_message,
            commands::get_thread,
            commands::set_flags,
            commands::move_messages,
            commands::delete_messages,
            commands::archive_messages,
            commands::create_archive_folder,
            commands::allow_remote_content,
            commands::list_subscriptions,
            commands::unsubscribe,
            commands::save_attachment,
            commands::open_attachment,
            commands::export_pdf,
            commands::send_message,
            commands::save_draft,
            commands::list_drafts,
            commands::get_draft,
            commands::delete_draft,
            commands::suggest_contacts,
            commands::set_maximize_rect,
            commands::get_app_settings,
            commands::set_close_to_tray,
            commands::set_autostart,
            commands::update_status,
            commands::check_for_update,
            commands::install_update,
            commands::set_tray_texts,
            commands::mcp_status,
            commands::set_mcp_enabled,
            commands::set_mcp_port,
            commands::mcp_reveal_key,
            commands::mcp_rotate_key,
            commands::mcp_connect_claude,
            commands::mcp_command_line,
        ])
        .run(tauri::generate_context!())
        .expect("error while running redpanda");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_database_moves_once() {
        let root = std::env::temp_dir().join(format!("redpanda-move-{}", uuid::Uuid::new_v4()));
        let (old, new) = (root.join("redpanda"), root.join("io.github.mkj777.redpanda"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("redpanda.db"), "db").unwrap();
        std::fs::write(old.join("redpanda.db-wal"), "wal").unwrap();
        std::fs::write(old.join("redpanda.exe"), "program").unwrap();

        move_legacy_db(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(new.join("redpanda.db")).unwrap(), "db");
        assert_eq!(
            std::fs::read_to_string(new.join("redpanda.db-wal")).unwrap(),
            "wal"
        );
        assert!(!old.join("redpanda.db").exists());
        assert!(old.join("redpanda.exe").exists(), "only database files move");

        // A database that is already in place is never replaced.
        std::fs::write(old.join("redpanda.db"), "stale").unwrap();
        move_legacy_db(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(new.join("redpanda.db")).unwrap(), "db");
        std::fs::remove_dir_all(root).unwrap();
    }
}
