//! Snap layouts for the custom title bar.
//!
//! The main window has no native frame, so the page draws the title bar. Windows 11 only offers
//! the snap layout flyout when a window answers `WM_NCHITTEST` with `HTMAXBUTTON`, and the webview
//! covers the whole client area and cannot give that answer. An invisible native child window
//! therefore sits over the HTML maximize button. It owns the mouse there: it maximizes or
//! restores the window itself and reports hover and press to the page, which draws the button.

use serde::Deserialize;
#[cfg(not(windows))]
use tauri::{AppHandle, WebviewWindow};

/// The maximize button in logical pixels, measured by the page. `right` is the distance from
/// the right edge, so the overlay follows the button while the window is resized.
#[derive(Clone, Copy, Debug, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct ButtonRect {
    pub right: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(windows)]
pub use imp::{install, place, set_rect};

#[cfg(not(windows))]
pub fn install(_app: &AppHandle, _window: &WebviewWindow) {}
#[cfg(not(windows))]
pub fn place() {}
#[cfg(not(windows))]
pub fn set_rect(_rect: Option<ButtonRect>) {}

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, WebviewWindow};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, NULL_BRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, FindWindowExW, GetClientRect, IsZoomed, LoadCursorW, PostMessageW,
        RegisterClassExW, SetWindowPos, ShowWindow, HTMAXBUTTON, HWND_TOP, IDC_ARROW, SC_MAXIMIZE,
        SC_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, WINDOW_EX_STYLE, WM_NCHITTEST,
        WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
        WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_SYSCOMMAND, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
    };

    use super::ButtonRect;

    /// Event the page listens to, with "none", "hover" or "press".
    const EVENT: &str = "titlebar:maximize";

    struct Overlay {
        app: AppHandle,
        parent: isize,
        hwnd: isize,
        rect: Option<ButtonRect>,
        hover: bool,
        press: bool,
    }

    static OVERLAY: Mutex<Option<Overlay>> = Mutex::new(None);

    /// Creates the overlay over the main window. Runs on the main thread, which owns the window.
    pub fn install(app: &AppHandle, window: &WebviewWindow) {
        let Ok(parent) = window.hwnd() else { return };
        unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                // Never paints, so the HTML button shows through.
                hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
                lpszClassName: w!("REDPANDA_SNAP_OVERLAY"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("REDPANDA_SNAP_OVERLAY"),
                None,
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                Some(parent),
                None,
                Some(instance.into()),
                None,
            ) else {
                tracing::warn!("could not create the snap layout overlay");
                return;
            };
            // Above the webview, but below Tauri's resize strip so the top edge still resizes.
            let strip = FindWindowExW(Some(parent), None, w!("TAURI_DRAG_RESIZE_BORDERS"), None).ok();
            let _ = SetWindowPos(hwnd, Some(strip.unwrap_or(HWND_TOP)), 0, 0, 0, 0, SWP_NOACTIVATE);
            *OVERLAY.lock().unwrap() = Some(Overlay {
                app: app.clone(),
                parent: parent.0 as isize,
                hwnd: hwnd.0 as isize,
                rect: None,
                hover: false,
                press: false,
            });
        }
    }

    pub fn set_rect(rect: Option<ButtonRect>) {
        if let Some(o) = OVERLAY.lock().unwrap().as_mut() {
            o.rect = rect;
        }
        place();
    }

    /// Moves the overlay onto the button, after a resize, a DPI change or a new measurement.
    pub fn place() {
        let Some((parent, hwnd, rect)) = OVERLAY
            .lock()
            .unwrap()
            .as_ref()
            .map(|o| (o.parent, o.hwnd, o.rect))
        else {
            return;
        };
        let (parent, hwnd) = (HWND(parent as _), HWND(hwnd as _));
        unsafe {
            let Some(r) = rect else {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return;
            };
            let scale = GetDpiForWindow(parent) as f64 / 96.0;
            let mut client = RECT::default();
            if GetClientRect(parent, &mut client).is_err() {
                return;
            }
            let x = client.right - ((r.right + r.width) * scale).round() as i32;
            let (y, width, height) = (
                (r.top * scale).round() as i32,
                (r.width * scale).round() as i32,
                (r.height * scale).round() as i32,
            );
            let _ = SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
    }

    /// Updates the hover and press state and tells the page when it changed.
    fn update(hover: bool, press: bool) {
        let changed = {
            let mut guard = OVERLAY.lock().unwrap();
            let Some(o) = guard.as_mut() else { return };
            let changed = o.hover != hover || o.press != press;
            (o.hover, o.press) = (hover, press);
            changed.then(|| o.app.clone())
        };
        if let Some(app) = changed {
            let state = if press {
                "press"
            } else if hover {
                "hover"
            } else {
                "none"
            };
            let _ = app.emit(EVENT, state);
        }
    }

    fn pressed() -> bool {
        OVERLAY.lock().unwrap().as_ref().is_some_and(|o| o.press)
    }

    unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            // The one answer that makes Windows 11 show the snap layout flyout.
            WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
            WM_NCMOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = unsafe { TrackMouseEvent(&mut track) };
                update(true, pressed());
                LRESULT(0)
            }
            WM_NCMOUSELEAVE => {
                update(false, false);
                LRESULT(0)
            }
            // Handled here instead of DefWindowProc, whose caption button tracking would swallow the release.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
                update(true, true);
                LRESULT(0)
            }
            WM_NCLBUTTONUP => {
                let was_pressed = pressed();
                update(true, false);
                if was_pressed {
                    unsafe { toggle_maximize() };
                }
                LRESULT(0)
            }
            WM_NCRBUTTONDOWN | WM_NCRBUTTONUP => LRESULT(0),
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    unsafe fn toggle_maximize() {
        let Some(parent) = OVERLAY.lock().unwrap().as_ref().map(|o| HWND(o.parent as _)) else {
            return;
        };
        let command = if unsafe { IsZoomed(parent) }.as_bool() {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        };
        let _ = unsafe { PostMessageW(Some(parent), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0)) };
    }
}
