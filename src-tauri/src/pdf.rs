//! Saves a message as a PDF.
//!
//! The mail is written to a page of its own and rendered in a hidden WebView2 window, whose
//! engine prints it to the file. That keeps the layout of HTML mail, which a text-only PDF
//! writer could not.

use crate::error::{AppError, Result};
use crate::model::{Address, MessageDetail};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn address(a: &Address) -> String {
    match a.name.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{} &lt;{}&gt;", esc(n), esc(&a.email)),
        None => esc(&a.email),
    }
}

fn list(label: &str, addrs: &[Address]) -> String {
    if addrs.is_empty() {
        return String::new();
    }
    let all: Vec<String> = addrs.iter().map(address).collect();
    format!("<tr><th>{label}</th><td>{}</td></tr>", all.join(", "))
}

/// The page that gets printed: a header block with the addresses, then the body.
pub fn document(msg: &MessageDetail, allow_remote: bool) -> String {
    let img = if allow_remote { "data: https: http:" } else { "data:" };
    let date = chrono::DateTime::from_timestamp(msg.summary.date, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default();
    let subject = if msg.summary.subject.is_empty() { "(no subject)" } else { &msg.summary.subject };
    let body = match (&msg.html, &msg.text) {
        (Some(h), _) => h.clone(),
        (None, t) => format!("<pre>{}</pre>", esc(t.as_deref().unwrap_or(""))),
    };
    let from = msg.summary.from.as_ref().map(address).unwrap_or_default();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {img}; style-src 'unsafe-inline'; font-src data:">
<style>
body{{margin:0;font:13px/1.5 "Segoe UI",system-ui,sans-serif;color:#1c1917;overflow-wrap:anywhere}}
h1{{font-size:18px;margin:0 0 10px}}
table.head{{border-collapse:collapse;margin-bottom:12px}}
table.head th{{text-align:left;color:#666;font-weight:normal;padding:1px 12px 1px 0;vertical-align:top}}
hr{{border:0;border-top:1px solid #ccc;margin:0 0 14px}}
img{{max-width:100%;height:auto}}
table{{max-width:100%}}
pre{{white-space:pre-wrap;font:inherit}}
</style></head><body>
<h1>{}</h1>
<table class="head">
<tr><th>From</th><td>{from}</td></tr>
{}{}<tr><th>Date</th><td>{date}</td></tr>
</table><hr>
{body}
</body></html>"#,
        esc(subject),
        list("To", &msg.summary.to),
        list("Cc", &msg.cc),
    )
}

#[cfg(windows)]
pub async fn export(app: &tauri::AppHandle, html: String, out: &std::path::Path) -> Result<()> {
    use std::sync::{Arc, Mutex};
    use tauri::webview::PageLoadEvent;
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment6, ICoreWebView2_2, ICoreWebView2_7,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{Interface, HSTRING};

    let id = uuid::Uuid::new_v4();
    let page = std::env::temp_dir().join(format!("redpanda-print-{id}.html"));
    std::fs::write(&page, html).map_err(|e| AppError::Other(e.to_string()))?;
    let url = url::Url::from_file_path(&page).map_err(|_| AppError::Other("bad temp path".into()))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<std::result::Result<(), String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let out_path = out.to_string_lossy().to_string();
    let label = format!("print-{id}");

    let tx_load = tx.clone();
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .visible(false)
        .inner_size(800.0, 1100.0)
        .on_page_load(move |win, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let tx = tx_load.clone();
            let path = out_path.clone();
            let _ = win.with_webview(move |wv| unsafe {
                let run = || -> windows::core::Result<()> {
                    let core = wv.controller().CoreWebView2()?;
                    let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
                    let settings = env.cast::<ICoreWebView2Environment6>()?.CreatePrintSettings()?;
                    // A4 in inches.
                    settings.SetPageWidth(8.27)?;
                    settings.SetPageHeight(11.69)?;
                    settings.SetMarginTop(0.6)?;
                    settings.SetMarginBottom(0.6)?;
                    settings.SetMarginLeft(0.6)?;
                    settings.SetMarginRight(0.6)?;
                    settings.SetShouldPrintBackgrounds(true)?;
                    settings.SetShouldPrintHeaderAndFooter(false)?;
                    let done = tx.clone();
                    let handler = PrintToPdfCompletedHandler::create(Box::new(move |res, ok| {
                        if let Some(t) = done.lock().unwrap().take() {
                            let _ = t.send(if res.is_ok() && ok {
                                Ok(())
                            } else {
                                Err("printing failed".into())
                            });
                        }
                        Ok(())
                    }));
                    core.cast::<ICoreWebView2_7>()?.PrintToPdf(&HSTRING::from(path.as_str()), &settings, &handler)
                };
                if let Err(e) = run() {
                    if let Some(t) = tx.lock().unwrap().take() {
                        let _ = t.send(Err(e.to_string()));
                    }
                }
            });
        })
        .build();
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            let _ = std::fs::remove_file(&page);
            return Err(AppError::Other(e.to_string()));
        }
    };

    let result = tokio::time::timeout(std::time::Duration::from_secs(30), rx).await;
    let _ = window.destroy();
    let _ = std::fs::remove_file(&page);
    match result {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(m))) => Err(AppError::Other(m)),
        _ => Err(AppError::Other("PDF export timed out".into())),
    }
}

#[cfg(not(windows))]
pub async fn export(_app: &tauri::AppHandle, _html: String, _out: &std::path::Path) -> Result<()> {
    Err(AppError::Other("PDF export is only available on Windows".into()))
}
