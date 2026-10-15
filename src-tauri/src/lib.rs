pub mod accounts;
mod commands;
pub mod core;
pub mod error;
pub mod mime;
pub mod model;
pub mod send;
pub mod service;
pub mod store;
pub mod sync;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

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
    }
}

fn data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("redpanda")
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("REDPANDA_LOG")
                .unwrap_or_else(|_| "redpanda_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db = Db::open(&data_dir().join("redpanda.db"))?;
            let secrets = Secrets::load_or_create()?;
            let events = Arc::new(TauriEvents(app.handle().clone()));
            let core = Arc::new(Core::new(db, secrets, OAuthConfig::load(), events));
            let state = Arc::new(App::new(core));
            // Workers use tokio::spawn, so start them inside Tauri's runtime.
            tauri::async_runtime::block_on(async { state.sync.start_all() })?;
            app.manage(state);
            Ok(())
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
            commands::save_attachment,
            commands::open_attachment,
            commands::send_message,
            commands::save_draft,
            commands::list_drafts,
            commands::get_draft,
            commands::delete_draft,
            commands::suggest_contacts,
        ])
        .run(tauri::generate_context!())
        .expect("error while running redpanda");
}
