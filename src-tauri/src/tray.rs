//! The notification area icon. Closing the window hides it there, so sync and IDLE keep running.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::service::App;

/// Menu and tooltip texts in the language the frontend shows, sent by it on start.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayTexts {
    pub open: String,
    pub compose: String,
    pub sync: String,
    pub quit: String,
    /// Contains `{{count}}`.
    pub unread: String,
    pub hint_title: String,
    pub hint_body: String,
}

impl Default for TrayTexts {
    fn default() -> Self {
        TrayTexts {
            open: "Open redpanda".into(),
            compose: "New email".into(),
            sync: "Check for new email".into(),
            quit: "Quit".into(),
            unread: "{{count}} unread".into(),
            hint_title: "redpanda keeps running".into(),
            hint_body: "New email still arrives. Quit from the icon in the notification area.".into(),
        }
    }
}

struct Tray {
    icon: TrayIcon,
    open: MenuItem<Wry>,
    compose: MenuItem<Wry>,
    sync: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    texts: Mutex<TrayTexts>,
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let texts = TrayTexts::default();
    let open = MenuItem::with_id(app, "open", &texts.open, true, None::<&str>)?;
    let compose = MenuItem::with_id(app, "compose", &texts.compose, true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", &texts.sync, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &compose,
            &sync,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("redpanda")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "compose" => {
                show_main(app);
                let _ = app.emit("ui:compose", ());
            }
            "sync" => {
                if let Some(state) = app.try_state::<Arc<App>>() {
                    state.sync_now(None);
                }
            }
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;
    app.manage(Tray {
        icon,
        open,
        compose,
        sync,
        quit,
        texts: Mutex::new(texts),
    });
    update_tooltip(app);
    Ok(())
}

pub fn set_texts(app: &AppHandle, texts: TrayTexts) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    for (item, text) in [
        (&tray.open, &texts.open),
        (&tray.compose, &texts.compose),
        (&tray.sync, &texts.sync),
        (&tray.quit, &texts.quit),
    ] {
        if let Err(e) = item.set_text(text) {
            tracing::warn!("could not set a tray menu text: {e}");
        }
    }
    if let Ok(mut t) = tray.texts.lock() {
        *t = texts;
    }
    update_tooltip(app);
}

/// Shows the number of unread inbox messages, called after every mail change.
pub fn update_tooltip(app: &AppHandle) {
    let (Some(tray), Some(state)) = (app.try_state::<Tray>(), app.try_state::<Arc<App>>()) else {
        return;
    };
    let unread = state.inbox_unread().unwrap_or(0);
    let tooltip = match tray.texts.lock() {
        Ok(t) if unread > 0 => format!("redpanda: {}", t.unread.replace("{{count}}", &unread.to_string())),
        _ => "redpanda".to_string(),
    };
    if let Err(e) = tray.icon.set_tooltip(Some(tooltip)) {
        tracing::warn!("could not set the tray tooltip: {e}");
    }
}

/// Brings the window back, only ever on the user's request (tray click, second start).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hides the window instead of closing it. The first time a notification says so.
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let first = app
        .try_state::<Arc<App>>()
        .and_then(|s| s.take_tray_hint().ok())
        .unwrap_or(false);
    if !first {
        return;
    }
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let Ok(texts) = tray.texts.lock().map(|t| t.clone()) else {
        return;
    };
    if let Err(e) = app
        .notification()
        .builder()
        .title(texts.hint_title)
        .body(texts.hint_body)
        .show()
    {
        tracing::warn!("could not show the tray hint: {e}");
    }
}

pub fn quit_app(app: &AppHandle) {
    if let Err(e) = app.save_window_state(StateFlags::all() - StateFlags::DECORATIONS) {
        tracing::warn!("could not save the window state: {e}");
    }
    app.exit(0);
}
