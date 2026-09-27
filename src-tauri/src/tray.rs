//! Menu-bar (macOS) / notification-area (Windows) icon and menu.

use crate::controller::Event;
use crate::ui;
use crate::AppState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let paste = MenuItem::with_id(app, "paste_last", "Paste last dictation", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Diktator", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&settings, &paste, &separator, &quit])?;
    let icon =
        app.default_window_icon().cloned().ok_or(tauri::Error::InvalidIcon(std::io::Error::other("no bundle icon")))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Diktator")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => ui::open_settings(app),
            "paste_last" => app.state::<AppState>().controller.send(Event::PasteLast),
            "quit" => {
                // hub.shutdown() can block up to ~26 s on the first Metal shader
                // compile; run it off the main thread so Quit never freezes the UI.
                let app = app.clone();
                std::thread::spawn(move || {
                    app.state::<AppState>().hub.shutdown();
                    app.exit(0);
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
