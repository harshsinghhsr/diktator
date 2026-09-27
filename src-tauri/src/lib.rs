pub mod asr;
pub mod audio;
pub mod catalog;
pub mod cleanup;
pub mod commands;
pub mod controller;
pub mod download;
pub mod hotkey;
pub mod insert;
pub mod live;
pub mod models;
pub mod permissions;
pub mod rewrite;
pub mod settings;
pub mod target_app;
#[cfg(test)]
pub mod testutil;
pub mod tray;
pub mod ui;
pub mod vad;

use catalog::ModelId;
use controller::Controller;
use models::ModelHub;
use settings::Settings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub hub: Arc<ModelHub>,
    pub controller: Controller,
    pub downloads: Mutex<HashMap<ModelId, Arc<AtomicBool>>>,
    /// Serializes `save_settings`: it's an async command, so two overlapping
    /// saves could otherwise interleave read-old / side-effect / write.
    pub save_lock: Mutex<()>,
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| ui::open_settings(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::list_microphones,
            commands::model_catalog,
            commands::download_model,
            commands::cancel_download,
            commands::delete_model,
            commands::permission_status,
            commands::open_permission_pane,
            commands::engine_status,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory); // menu-bar app, no Dock icon
                app.handle().plugin(tauri_nspanel::init())?;
            }
            let handle = app.handle().clone();
            let settings_path = handle.path().app_config_dir()?.join("settings.json");
            let models_root = handle.path().app_data_dir()?.join("models");
            std::fs::create_dir_all(&models_root)?;
            let settings = Settings::load(&settings_path);

            let status_app = handle.clone();
            let hub = ModelHub::new(models_root, move |s| {
                let _ = status_app.emit("engine-status", &s);
            });
            hub.sync(&settings);

            ui::create_overlay(&handle)?;
            let tauri_ui: Arc<dyn controller::Ui> = Arc::new(ui::TauriUi::new(handle.clone()));
            let controller = Controller::start(settings.clone(), hub.clone(), tauri_ui);
            let first_run = !settings.onboarding_done;
            app.manage(AppState {
                settings_path,
                settings: Mutex::new(settings),
                hub,
                controller,
                downloads: Mutex::default(),
                save_lock: Mutex::new(()),
            });
            tray::create(&handle)?;
            if first_run {
                ui::open_settings(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Diktator")
        .run(|app, event| {
            // Closing the settings window must not quit a tray app; only "Quit" (exit code set) does.
            if let tauri::RunEvent::ExitRequested { code: None, ref api, .. } = event {
                api.prevent_exit();
            }
            // Logout, restart, `osascript ... quit` and an installer quit go through
            // applicationWillTerminate -> LoopDestroyed -> Exit, never ExitRequested, so
            // shutdown must also happen here. Idempotent, so the tray Quit handler's own
            // call to `hub.shutdown()` is harmless.
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().hub.shutdown();
            }
            // Accessory apps get no Dock icon, so relaunching from Finder/Spotlight while
            // already running sends Reopen instead of a second process; without this the
            // relaunch is silently swallowed. `RunEvent::Reopen` only exists on macOS.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                ui::open_settings(app);
            }
        });
}
