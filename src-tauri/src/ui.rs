//! Windows the user sees: the non-activating overlay pill and the settings
//! window, plus the Tauri implementation of `controller::Ui`.

use crate::controller::{Overlay, Ui};
use crate::settings::PasteOverride;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const OVERLAY: &str = "overlay";
pub const SETTINGS: &str = "settings";
pub(crate) const WIDTH: f64 = 260.0;
pub(crate) const HEIGHT: f64 = 64.0;
pub(crate) const BOTTOM_MARGIN: f64 = 96.0;
const MESSAGE_VISIBLE: Duration = Duration::from_secs(3);

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(OverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

pub fn overlay_position(monitor_pos: (i32, i32), monitor_size: (u32, u32), scale: f64) -> (i32, i32) {
    let x = monitor_pos.0 + ((monitor_size.0 as f64 - WIDTH * scale) / 2.0) as i32;
    let y = monitor_pos.1 + (monitor_size.1 as f64 - (HEIGHT + BOTTOM_MARGIN) * scale) as i32;
    (x, y)
}

pub fn create_overlay(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::{CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};
        let panel = PanelBuilder::<_, OverlayPanel>::new(app, OVERLAY)
            .url(WebviewUrl::App("overlay.html".into()))
            .title("Diktator")
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize { width: WIDTH, height: HEIGHT }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| w.decorations(false).transparent(true).focusable(false))
            .collection_behavior(CollectionBehavior::new().can_join_all_spaces().full_screen_auxiliary())
            .build()?;
        panel.hide();
    }
    #[cfg(not(target_os = "macos"))]
    {
        WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into()))
            .title("Diktator")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()?;
    }
    Ok(())
}

fn place_overlay(app: &AppHandle, window: &tauri::WebviewWindow) {
    let cursor = app.cursor_position().ok();
    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = cursor
        .and_then(|c| {
            monitors.iter().find(|m| {
                let (p, s) = (m.position(), m.size());
                c.x >= p.x as f64
                    && c.x < (p.x + s.width as i32) as f64
                    && c.y >= p.y as f64
                    && c.y < (p.y + s.height as i32) as f64
            })
        })
        .cloned()
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let (x, y) =
            overlay_position((m.position().x, m.position().y), (m.size().width, m.size().height), m.scale_factor());
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn show_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY) else { return };
    place_overlay(app, &window);
    let _ = window.show();
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        };
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

fn hide_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY) {
        let _ = window.hide();
    }
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    match WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings.html".into()))
        .title("Diktator Settings")
        .inner_size(560.0, 760.0)
        .min_inner_size(480.0, 560.0)
        .build()
    {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => log::error!("could not open settings: {e}"),
    }
}

pub struct TauriUi {
    app: AppHandle,
    /// Bumped on every overlay change, so a message's auto-hide timer only
    /// hides the overlay if nothing newer was shown in the meantime.
    generation: Arc<AtomicU64>,
}

impl TauriUi {
    pub fn new(app: AppHandle) -> Self {
        Self { app, generation: Arc::new(AtomicU64::new(0)) }
    }
}

impl Ui for TauriUi {
    fn run_on_main(&self, f: Box<dyn FnOnce() + Send>) {
        if let Err(e) = self.app.run_on_main_thread(f) {
            log::warn!("run on main thread: {e}");
        }
    }

    fn overlay(&self, state: Overlay) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.app.emit_to(OVERLAY, "overlay-state", &state);
        match state {
            Overlay::Hidden => hide_overlay(&self.app),
            Overlay::Listening | Overlay::Processing => show_overlay(&self.app),
            Overlay::Message(_) => {
                show_overlay(&self.app);
                let app = self.app.clone();
                let current = self.generation.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(MESSAGE_VISIBLE);
                    if current.load(Ordering::SeqCst) == generation {
                        let _ = app.emit_to(OVERLAY, "overlay-state", &Overlay::Hidden);
                        hide_overlay(&app);
                    }
                });
            }
        }
    }

    fn level(&self, value: f32) {
        let _ = self.app.emit_to(OVERLAY, "overlay-level", value);
    }

    fn insert(&self, text: &str, overrides: &[PasteOverride]) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            // Pasteboard promises and keyboard-layout lookups need the main thread.
            let (tx, rx) = std::sync::mpsc::channel();
            let text = text.to_string();
            let overrides = overrides.to_vec();
            self.app
                .run_on_main_thread(move || {
                    let _ = tx.send(crate::insert::deliver(&text, &overrides));
                })
                .map_err(|e| e.to_string())?;
            rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "Timed out while inserting text.".to_string())?
        }
        #[cfg(not(target_os = "macos"))]
        {
            crate::insert::deliver(text, overrides)
        }
    }

    fn open_settings(&self) {
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || open_settings(&app));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_horizontally_near_the_bottom() {
        // 1440x900 logical display at 2x → 2880x1800 physical.
        let (x, y) = overlay_position((0, 0), (2880, 1800), 2.0);
        assert_eq!(x, ((2880.0 - WIDTH * 2.0) / 2.0) as i32);
        assert_eq!(y, (1800.0 - (HEIGHT + BOTTOM_MARGIN) * 2.0) as i32);
        // Secondary monitor to the right, 1x.
        let (x2, _) = overlay_position((2880, 0), (1920, 1080), 1.0);
        assert_eq!(x2, 2880 + ((1920.0 - WIDTH) / 2.0) as i32);
    }
}
