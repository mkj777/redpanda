//! Thin Tauri command wrappers around [`App`].

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::accounts::discovery::DiscoveryResult;
use crate::error::{AppError, Result};
use crate::model::*;
use crate::service::{AccountView, App, AppSettings, FlagKind, FolderList, NewAccountInput, OAuthStatus};

type AppState<'a> = State<'a, Arc<App>>;

fn opener(app: &AppHandle) -> impl FnOnce(&str) -> Result<()> + '_ {
    move |url: &str| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::Other(format!("could not open the browser: {e}")))
    }
}

#[tauri::command]
pub fn list_accounts(state: AppState<'_>) -> Result<Vec<AccountView>> {
    state.list_accounts()
}

#[tauri::command]
pub async fn discover(state: AppState<'_>, email: String) -> Result<DiscoveryResult> {
    Ok(state.discover(&email).await)
}

#[tauri::command]
pub fn oauth_status(state: AppState<'_>) -> OAuthStatus {
    state.oauth_status()
}

#[tauri::command]
pub async fn add_account(app: AppHandle, state: AppState<'_>, input: NewAccountInput) -> Result<AccountView> {
    state.add_account(input, opener(&app)).await
}

#[tauri::command]
pub async fn reauth_account(
    app: AppHandle,
    state: AppState<'_>,
    account_id: String,
    password: Option<String>,
) -> Result<()> {
    state.reauth_account(&account_id, password, opener(&app)).await
}

#[tauri::command]
pub fn cancel_oauth(state: AppState<'_>) {
    state.cancel_oauth()
}

#[tauri::command]
pub fn update_account(
    state: AppState<'_>,
    account_id: String,
    display_name: String,
    color: String,
) -> Result<()> {
    state.update_account(&account_id, &display_name, &color)
}

#[tauri::command]
pub fn remove_account(state: AppState<'_>, account_id: String) -> Result<()> {
    state.remove_account(&account_id)
}

#[tauri::command]
pub fn list_folders(state: AppState<'_>) -> Result<FolderList> {
    state.list_folders()
}

#[tauri::command]
pub fn list_messages(
    state: AppState<'_>,
    view: View,
    offset: u32,
    limit: u32,
    threads: bool,
) -> Result<MessagePage> {
    state.list_messages(&view, offset, limit, threads)
}

#[tauri::command]
pub fn open_folder(state: AppState<'_>, folder_id: i64) -> Result<()> {
    state.open_folder(folder_id)
}

#[tauri::command]
pub fn sync_now(state: AppState<'_>, account_id: Option<String>) {
    state.sync_now(account_id.as_deref())
}

#[tauri::command]
pub async fn load_older(state: AppState<'_>, folder_id: i64) -> Result<bool> {
    state.load_older(folder_id).await
}

#[tauri::command]
pub async fn get_message(state: AppState<'_>, id: i64) -> Result<MessageDetail> {
    state.get_message(id).await
}

#[tauri::command]
pub fn get_thread(state: AppState<'_>, id: i64) -> Result<Vec<MessageSummary>> {
    state.get_thread(id)
}

#[tauri::command]
pub fn set_flags(
    state: AppState<'_>,
    ids: Vec<i64>,
    flag: FlagKind,
    value: bool,
    threads: bool,
) -> Result<()> {
    state.set_flags(&ids, flag, value, threads)
}

#[tauri::command]
pub fn move_messages(state: AppState<'_>, ids: Vec<i64>, target_folder_id: i64, threads: bool) -> Result<()> {
    state.move_messages(&ids, target_folder_id, threads)
}

#[tauri::command]
pub fn delete_messages(state: AppState<'_>, ids: Vec<i64>, threads: bool) -> Result<()> {
    state.delete_messages(&ids, threads)
}

#[tauri::command]
pub fn archive_messages(state: AppState<'_>, ids: Vec<i64>, threads: bool) -> Result<()> {
    state.archive_messages(&ids, threads)
}

#[tauri::command]
pub async fn create_archive_folder(state: AppState<'_>, account_id: String) -> Result<()> {
    state.create_archive_folder(&account_id).await
}

#[tauri::command]
pub fn allow_remote_content(state: AppState<'_>, sender: String) -> Result<()> {
    state.allow_remote_content(&sender)
}

#[tauri::command]
pub fn list_subscriptions(state: AppState<'_>) -> Result<Vec<Subscription>> {
    state.list_subscriptions()
}

#[tauri::command]
pub async fn unsubscribe(
    app: AppHandle,
    state: AppState<'_>,
    message_id: i64,
    page: bool,
) -> Result<crate::unsubscribe::Method> {
    state.unsubscribe(message_id, page, opener(&app)).await
}

#[tauri::command]
pub fn save_attachment(state: AppState<'_>, message_id: i64, index: u32, path: String) -> Result<()> {
    state.save_attachment(message_id, index, &path)
}

#[tauri::command]
pub async fn export_pdf(
    app: AppHandle,
    state: AppState<'_>,
    message_id: i64,
    path: String,
    allow_remote: bool,
) -> Result<()> {
    let msg = state.get_message(message_id).await?;
    crate::pdf::export(
        &app,
        crate::pdf::document(&msg, allow_remote),
        std::path::Path::new(&path),
    )
    .await
}

#[tauri::command]
pub fn open_attachment(app: AppHandle, state: AppState<'_>, message_id: i64, index: u32) -> Result<()> {
    let path = state.attachment_temp_file(message_id, index)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::Other(e.to_string()))
}

#[tauri::command]
pub async fn send_message(state: AppState<'_>, draft: Draft) -> Result<()> {
    state.send(draft).await
}

#[tauri::command]
pub fn save_draft(state: AppState<'_>, draft: Draft) -> Result<i64> {
    state.save_draft(&draft)
}

#[tauri::command]
pub fn list_drafts(state: AppState<'_>) -> Result<Vec<Draft>> {
    state.list_drafts()
}

#[tauri::command]
pub fn get_draft(state: AppState<'_>, id: i64) -> Result<Draft> {
    state.get_draft(id)
}

#[tauri::command]
pub fn delete_draft(state: AppState<'_>, id: i64) -> Result<()> {
    state.delete_draft(id)
}

#[tauri::command]
pub fn suggest_contacts(state: AppState<'_>, prefix: String) -> Result<Vec<Address>> {
    state.suggest_contacts(&prefix)
}

/// The page reports where it drew the maximize button, so the snap layout overlay can sit on it.
#[tauri::command]
pub fn set_maximize_rect(app: AppHandle, rect: Option<crate::titlebar::ButtonRect>) -> Result<()> {
    app.run_on_main_thread(move || crate::titlebar::set_rect(rect))
        .map_err(|e| AppError::Other(e.to_string()))
}

#[tauri::command]
pub fn get_app_settings(state: AppState<'_>) -> Result<AppSettings> {
    state.app_settings()
}

#[tauri::command]
pub fn set_close_to_tray(state: AppState<'_>, value: bool) -> Result<()> {
    state.set_close_to_tray(value)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, state: AppState<'_>, value: bool) -> Result<()> {
    state.set_autostart(value)?;
    crate::apply_autostart(&app, value);
    Ok(())
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> crate::update::UpdateStatus {
    crate::update::status(&app)
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<crate::update::UpdateInfo>> {
    crate::update::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    crate::update::install(&app).await
}

/// The frontend sends the tray texts in its language, on start and when the language changes.
#[tauri::command]
pub fn set_tray_texts(app: AppHandle, texts: crate::tray::TrayTexts) {
    crate::tray::set_texts(&app, texts)
}

type McpState<'a> = State<'a, Arc<crate::mcp::Mcp>>;

#[tauri::command]
pub async fn mcp_status(mcp: McpState<'_>) -> Result<crate::mcp::McpStatus> {
    Ok(mcp.status().await)
}

#[tauri::command]
pub async fn set_mcp_enabled(mcp: McpState<'_>, value: bool) -> Result<crate::mcp::McpStatus> {
    mcp.set_enabled(value).await?;
    Ok(mcp.status().await)
}

#[tauri::command]
pub async fn set_mcp_port(mcp: McpState<'_>, port: u16) -> Result<crate::mcp::McpStatus> {
    mcp.set_port(port).await?;
    Ok(mcp.status().await)
}

/// Only on the user's request in the settings, to copy it by hand.
#[tauri::command]
pub fn mcp_reveal_key(mcp: McpState<'_>) -> Result<String> {
    mcp.key()
}

#[tauri::command]
pub async fn mcp_rotate_key(mcp: McpState<'_>) -> Result<crate::mcp::McpStatus> {
    mcp.rotate_key()?;
    Ok(mcp.status().await)
}

#[tauri::command]
pub async fn mcp_connect_claude(mcp: McpState<'_>) -> Result<()> {
    mcp.connect_claude().await
}

#[tauri::command]
pub fn mcp_command_line(mcp: McpState<'_>) -> Result<String> {
    mcp.claude_command_line()
}
