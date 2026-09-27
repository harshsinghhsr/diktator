//! Tauri commands: the whole frontend ↔ backend surface (see docs/architecture.md).

use crate::catalog::{self, ModelId, ModelView};
use crate::controller::Event;
use crate::download::{self, Progress, Status};
use crate::models::EngineStatus;
use crate::permissions::{self, PermissionStatus};
use crate::settings::Settings;
use crate::{audio, AppState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command(async)]
pub fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
    // Held for the whole body (all sync work, no .await) so two concurrent
    // saves can't interleave read-old / side-effect / write.
    let _guard = state.save_lock.lock().unwrap();
    settings.validate()?;
    let old = state.settings.lock().unwrap().clone();
    if settings.shortcut != old.shortcut {
        state.controller.hotkeys().set_shortcut(&settings.shortcut)?;
    }
    if settings.launch_at_login != old.launch_at_login {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let changed = if settings.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        changed.map_err(|e| format!("Could not change launch at login: {e}"))?;
    }
    settings.save(&state.settings_path).map_err(|e| format!("{e:#}"))?;
    *state.settings.lock().unwrap() = settings.clone();
    state.hub.sync(&settings);
    state.controller.send(Event::Settings(Box::new(settings)));
    Ok(())
}

#[tauri::command(async)]
pub fn list_microphones() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn model_catalog(state: State<AppState>) -> Vec<ModelView> {
    catalog::views(state.hub.root())
}

#[tauri::command]
pub fn download_model(app: AppHandle, state: State<AppState>, id: ModelId) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.downloads.lock().unwrap();
        if active.contains_key(&id) {
            return Ok(());
        }
        active.insert(id, cancel.clone());
    }
    let root = state.hub.root().to_path_buf();
    std::thread::spawn(move || {
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let result = download::install(&root, id, &cancel, &mut |p: Progress| {
            if p.status != Status::Downloading || last_emit.elapsed() >= Duration::from_millis(150) {
                last_emit = Instant::now();
                let _ = app.emit("download-progress", &p);
            }
        });
        let state = app.state::<AppState>();
        state.downloads.lock().unwrap().remove(&id);
        match result {
            Ok(()) => {
                let settings = state.settings.lock().unwrap().clone();
                state.hub.sync(&settings);
            }
            Err(e) => {
                let cancelled = e.downcast_ref::<download::Cancelled>().is_some();
                let mut p = Progress::new(id, 0, 0, if cancelled { Status::Cancelled } else { Status::Error });
                if !cancelled {
                    log::warn!("download {id:?} failed: {e:#}");
                    p.error = Some(format!("{e:#}"));
                }
                let _ = app.emit("download-progress", &p);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_download(state: State<AppState>, id: ModelId) {
    if let Some(flag) = state.downloads.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command(async)]
pub fn delete_model(state: State<AppState>, id: ModelId) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    let in_use = id == ModelId::SileroVad
        || id == ModelId::from(settings.speech_model)
        || Some(id) == catalog::writing_model_id(settings.writing_model);
    if in_use {
        return Err("This model is in use. Choose a different one first.".into());
    }
    download::uninstall(state.hub.root(), id).map_err(|e| format!("{e:#}"))?;
    state.hub.sync(&settings);
    Ok(())
}

#[tauri::command]
pub fn permission_status() -> PermissionStatus {
    permissions::status()
}

#[tauri::command]
pub fn open_permission_pane(pane: String) -> Result<(), String> {
    permissions::open_pane(&pane)
}

#[tauri::command]
pub fn engine_status(state: State<AppState>) -> EngineStatus {
    state.hub.status()
}
