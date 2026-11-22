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

/// Searches every account on its server too; the hits then show up in the search view.
#[tauri::command]
pub async fn search_server(state: AppState<'_>, query: String) -> Result<usize> {
    state.search_server(&query).await
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
pub fn mark_junk(state: AppState<'_>, ids: Vec<i64>, junk: bool, threads: bool) -> Result<()> {
    state.mark_junk(&ids, junk, threads)
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
    open_file(&app, &path)
}

fn open_file(app: &AppHandle, path: &std::path::Path) -> Result<()> {
    crate::service::check_openable(path)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Saves an attachment to the Downloads folder without a dialog.
#[tauri::command]
pub async fn download_attachment(state: AppState<'_>, message_id: i64, index: u32) -> Result<Download> {
    let dir = dirs::download_dir().ok_or_else(|| AppError::Other("no Downloads folder".into()))?;
    let app = state.inner().clone();
    tokio::task::spawn_blocking(move || app.download_attachment(message_id, index, &dir))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

/// Checks every file of the list on disk; one on an offline network share can take a while.
#[tauri::command]
pub async fn list_downloads(state: AppState<'_>) -> Result<Vec<Download>> {
    let app = state.inner().clone();
    tokio::task::spawn_blocking(move || app.list_downloads())
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

#[tauri::command]
pub fn remove_download(state: AppState<'_>, id: i64) -> Result<()> {
    state.remove_download(id)
}

#[tauri::command]
pub fn clear_downloads(state: AppState<'_>) -> Result<()> {
    state.clear_downloads()
}

#[tauri::command]
pub fn open_download(app: AppHandle, state: AppState<'_>, id: i64) -> Result<()> {
    open_file(&app, &state.download_file(id)?)
}

/// Opens Explorer with the downloaded file selected.
#[tauri::command]
pub fn reveal_download(app: AppHandle, state: AppState<'_>, id: i64) -> Result<()> {
    let path = state.download_file(id)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Drags an attachment out of the window as a file, from a copy in the temp folder.
#[tauri::command]
pub async fn drag_attachment(
    window: tauri::Window,
    state: AppState<'_>,
    message_id: i64,
    index: u32,
) -> Result<()> {
    let path = state.attachment_temp_file(message_id, index)?;
    drag_out(window, path).await
}

/// Drags a downloaded file out of the window.
#[tauri::command]
pub async fn drag_download(window: tauri::Window, state: AppState<'_>, id: i64) -> Result<()> {
    let path = state.download_file(id)?;
    drag_out(window, path).await
}

/// Runs a native drag of `path` until it is dropped or cancelled. Only ever copies, so dropping
/// a download into another folder leaves it in Downloads. The page names a message or a download,
/// never a path, so it can't drag out any other file.
#[cfg(windows)]
async fn drag_out(window: tauri::Window, path: std::path::PathBuf) -> Result<()> {
    const PREVIEW: &[u8] = include_bytes!("../assets/drag-file.png");
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = drag::start_drag(
                &target,
                drag::DragItem::Files(vec![path]),
                drag::Image::Raw(PREVIEW.to_vec()),
                |_, _| {},
                drag::Options::default(),
            );
            let _ = tx.send(result);
        })
        .map_err(|e| AppError::Other(e.to_string()))?;
    rx.await
        .map_err(|e| AppError::Other(e.to_string()))?
        .map_err(|e| AppError::Other(format!("could not drag the file: {e}")))
}

#[cfg(not(windows))]
async fn drag_out(_window: tauri::Window, _path: std::path::PathBuf) -> Result<()> {
    Err(AppError::Other(
        "dragging files out is only built for Windows".into(),
    ))
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultMailStatus {
    supported: bool,
    is_default: bool,
    ask: bool,
}

#[tauri::command]
pub fn default_mail_status(state: AppState<'_>) -> Result<DefaultMailStatus> {
    Ok(DefaultMailStatus {
        supported: crate::default_mail::supported(),
        is_default: crate::default_mail::is_default(),
        ask: state.ask_default_mail()?,
    })
}

#[tauri::command]
pub fn set_default_mail_ask(state: AppState<'_>, value: bool) -> Result<()> {
    state.set_ask_default_mail(value)
}

#[tauri::command]
pub fn open_default_apps(app: AppHandle) -> Result<()> {
    app.opener()
        .open_url(crate::default_mail::settings_uri(), None::<&str>)
        .map_err(|e| AppError::Other(format!("could not open the Windows settings: {e}")))
}

#[tauri::command]
pub fn take_mailto(pending: State<'_, crate::PendingMailto>) -> Option<crate::default_mail::Mailto> {
    pending.0.lock().unwrap().take()
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
pub async fn mcp_connect_claude_desktop(mcp: McpState<'_>) -> Result<()> {
    mcp.connect_claude_desktop().await
}

#[tauri::command]
pub fn mcp_command_line(mcp: McpState<'_>) -> Result<String> {
    mcp.claude_command_line()
}
