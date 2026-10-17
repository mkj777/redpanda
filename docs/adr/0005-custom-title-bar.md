# 0005: Custom title bar

Status: accepted

## Context

The default Windows title bar sits above the app in system colors and does not follow redpanda's light and dark themes. The goal is a title bar that belongs to the app and still behaves like the native one: drag to move, drag to a screen edge to snap, double-click to maximize, right-click for the system menu, resizing from the top edge, and the Windows 11 snap layout flyout on the maximize button.

Tauri can remove the frame (`decorations: false`), and with the default shadow Windows 11 keeps the 1 px border, the rounded corners and resizing from all edges. Two parts are not covered:

- Dragging. `data-tauri-drag-region` moves the window from JavaScript and has no system menu. WebView2 can instead treat elements with the CSS property `app-region: drag` as a real caption, and wry turns that support on (`IsNonClientRegionSupportEnabled`).
- Snap layouts. Windows shows the flyout only when a window answers `WM_NCHITTEST` with `HTMAXBUTTON`. The webview is a child window covering the whole client area, so neither the page nor a subclassed main window can give that answer (tauri#4531, winit#3884).

## Decision

- `TitleBar.tsx` draws a 32 px bar: an empty `app-region: drag` area (no icon or app name, the bar only gives the content room at the top), then minimize, maximize and close buttons, 46 px wide, with the system glyphs from Segoe Fluent Icons (Segoe MDL2 Assets on Windows 10) and the Windows 11 hover colors.
- `titlebar.rs` creates an invisible native child window (`WS_CHILD | WS_CLIPSIBLINGS`, null brush, never paints) over the HTML maximize button. It answers `HTMAXBUTTON`, maximizes or restores through `WM_SYSCOMMAND` and reports hover and press to the page (`titlebar:maximize`), because the page no longer sees the mouse there.
- The page measures the button and reports its rectangle in logical pixels, anchored to the right edge. Rust converts it with the window's DPI and moves the overlay on every resize and scale change, so no sizes are duplicated between CSS and Rust.
- The overlay sits below Tauri's resize strip (`TAURI_DRAG_RESIZE_BORDERS`), so the top edge above the button still resizes the window.
- The window state plugin no longer restores decorations, or a state saved before this change would bring the frame back.

This is the technique all published plugins converged on (tauri-plugin-frame, -decorum, -window-controls, -snap-layout). It is about 200 lines, so it lives in the repo instead of adding a young dependency.

## Consequences

- The title bar follows the app theme and shows the inactive state when the window loses focus.
- The maximize button's look depends on the overlay's events instead of CSS `:hover`. If the overlay cannot be created, the button falls back to a normal click and hover.
- Windows 10 has no snap layout flyout; the overlay still maximizes and restores.
- Snapping into a zone needs the window's minimum width to fit it. With `minWidth: 760`, halves and quarters stop working on small or highly scaled screens, as they did with the native frame.
