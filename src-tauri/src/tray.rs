//! The system tray icon, its menu, and the mini panel (today's releases, sync now, quick search).

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const PANEL: (f64, f64) = (372.0, 520.0);

/// Shows the main window, recreating it if it was released to save memory.
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    let window = match app.get_webview_window("main") {
        Some(w) => Some(w),
        None => app.config().app.windows.first().and_then(|cfg| WebviewWindowBuilder::from_config(app, cfg).ok()).and_then(|b| b.build().ok()),
    };
    if let Some(w) = window {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(route) = route {
            let _ = app.emit_to("main", "navigate", route);
            // A freshly created window isn't listening yet: leave the route for it to pick up.
            *app.state::<crate::PendingRoute>().0.lock().unwrap() = Some(route.to_string());
        }
    }
}

fn toggle_panel(app: &AppHandle, at: PhysicalPosition<f64>) {
    if let Some(panel) = app.get_webview_window("tray") {
        if panel.is_visible().unwrap_or(false) {
            let _ = panel.hide();
            return;
        }
        place(&panel, at);
        let _ = panel.show();
        let _ = panel.set_focus();
        let _ = app.emit_to("tray", "tray:opened", ());
        return;
    }
    let built = WebviewWindowBuilder::new(app, "tray", WebviewUrl::App("index.html#/tray".into()))
        .title("Marquee")
        .inner_size(PANEL.0, PANEL.1)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .visible(false)
        .build();
    if let Ok(panel) = built {
        place(&panel, at);
        let _ = panel.show();
        let _ = panel.set_focus();
    }
}

/// Puts the panel above (or below) the click, kept inside the monitor it's on.
fn place(panel: &tauri::WebviewWindow, at: PhysicalPosition<f64>) {
    let scale = panel.scale_factor().unwrap_or(1.0);
    let (w, h) = (PANEL.0 * scale, PANEL.1 * scale);
    let mut x = at.x - w / 2.0;
    let mut y = at.y - h - 12.0 * scale;
    if let Ok(Some(monitor)) = panel.monitor_from_point(at.x, at.y) {
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
        if y < my {
            y = at.y + 12.0 * scale; // taskbar is at the top
        }
        x = x.clamp(mx + 8.0, (mx + mw - w - 8.0).max(mx + 8.0));
        y = y.clamp(my + 8.0, (my + mh - h - 8.0).max(my + 8.0));
    }
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Marquee").build(app)?;
    let search = MenuItemBuilder::with_id("search", "Quick search").build(app)?;
    let sync = MenuItemBuilder::with_id("sync", "Sync now").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&open, &search, &sync]).separator().item(&quit).build()?;

    let mut builder = TrayIconBuilder::with_id("marquee").tooltip("Marquee").menu(&menu).show_menu_on_left_click(false);
    // A simpler mark than the app icon, so it stays legible at 16 px.
    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")) {
        builder = builder.icon(icon);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, None),
            "search" => show_main(app, Some("/search")),
            "sync" => crate::sync_now(app.clone()),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } => toggle_panel(tray.app_handle(), position),
            TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => show_main(tray.app_handle(), None),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
pub fn tray_hide(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("tray") {
        let _ = panel.hide();
    }
}

#[tauri::command]
pub fn open_main(app: AppHandle, route: Option<String>) {
    tray_hide(app.clone());
    show_main(&app, route.as_deref());
}
