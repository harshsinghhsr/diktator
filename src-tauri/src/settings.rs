//! User settings, stored as pretty JSON at `<app_config_dir>/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechModel {
    Canary180mFlash,
    ParakeetTdtV2,
    ParakeetTdtV3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingModel {
    /// Rule-based cleanup only, no LLM.
    Lightweight,
    /// Qwen2.5-1.5B-Instruct.
    Balanced,
    /// Qwen3.5-2B.
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteMode {
    Natural,
    Professional,
    Concise,
    /// Insert the speech-to-text output untouched.
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteChord {
    /// Cmd+V on macOS, Ctrl+V on Windows.
    Standard,
    /// Ctrl+Shift+V (Windows terminals).
    CtrlShiftV,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasteOverride {
    /// Lower-case macOS bundle id (e.g. `com.googlecode.iterm2`) or Windows exe name (e.g. `windowsterminal.exe`).
    pub app: String,
    pub chord: PasteChord,
}

/// Languages Canary-180M-Flash accepts. Parakeet v2 is English-only; v3 auto-detects.
pub const CANARY_LANGUAGES: [&str; 4] = ["en", "es", "de", "fr"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Modifiers then one key, e.g. "cmd+shift+f" (see `hotkey::parse_shortcut`).
    pub shortcut: String,
    /// cpal device name; `None` = system default input.
    pub microphone: Option<String>,
    pub speech_model: SpeechModel,
    pub writing_model: WritingModel,
    pub mode: RewriteMode,
    /// Only used by Canary.
    pub language: String,
    /// Silence that ends a *toggle* dictation. 0 = never auto-stop.
    pub auto_stop_ms: u32,
    pub launch_at_login: bool,
    pub paste_overrides: Vec<PasteOverride>,
    pub onboarding_done: bool,
}

pub fn default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd+shift+f"
    } else {
        "ctrl+shift+f"
    }
}

fn default_paste_overrides() -> Vec<PasteOverride> {
    if !cfg!(target_os = "windows") {
        return Vec::new(); // macOS terminals accept Cmd+V
    }
    ["windowsterminal.exe", "alacritty.exe", "wezterm-gui.exe"]
        .iter()
        .map(|app| PasteOverride { app: app.to_string(), chord: PasteChord::CtrlShiftV })
        .collect()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: default_shortcut().to_string(),
            microphone: None,
            speech_model: SpeechModel::ParakeetTdtV2,
            writing_model: WritingModel::Balanced,
            mode: RewriteMode::Natural,
            language: "en".to_string(),
            auto_stop_ms: 700,
            launch_at_login: false,
            paste_overrides: default_paste_overrides(),
            onboarding_done: false,
        }
    }
}

impl Settings {
    /// Never fails: a missing file gives defaults; an unreadable or invalid one
    /// is copied to `settings.json.bak` and replaced by defaults in memory.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Settings::default();
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(s) if s.validate().is_ok() => s,
            Ok(_) | Err(_) => {
                log::warn!("settings file invalid; using defaults (backup kept)");
                let _ = std::fs::copy(path, path.with_extension("json.bak"));
                Settings::default()
            }
        }
    }

    /// Validates, then writes atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        self.validate().map_err(anyhow::Error::msg)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Error strings are shown to the user as-is.
    pub fn validate(&self) -> Result<(), String> {
        let hotkey = crate::hotkey::parse_shortcut(&self.shortcut)?;
        if hotkey == crate::hotkey::parse_shortcut("esc")? {
            return Err("Esc is reserved for cancelling a dictation.".into());
        }
        if !CANARY_LANGUAGES.contains(&self.language.as_str()) {
            return Err(format!("Language must be one of {}.", CANARY_LANGUAGES.join(", ")));
        }
        if self.auto_stop_ms != 0 && !(300..=3000).contains(&self.auto_stop_ms) {
            return Err("Auto-stop must be off or between 300 and 3000 ms.".into());
        }
        if self.paste_overrides.iter().any(|o| o.app.trim().is_empty()) {
            return Err("Paste overrides need an app name.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("diktator-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_are_valid() {
        let s = Settings::default();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.speech_model, SpeechModel::ParakeetTdtV2);
        assert_eq!(s.writing_model, WritingModel::Balanced);
        assert_eq!(s.mode, RewriteMode::Natural);
        assert_eq!(s.auto_stop_ms, 700);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("settings.json");
        let s = Settings { mode: RewriteMode::Concise, microphone: Some("USB Mic".into()), ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tmp_dir("missing");
        assert_eq!(Settings::load(&dir.join("nope.json")), Settings::default());
    }

    #[test]
    fn corrupt_file_gives_defaults_and_keeps_backup() {
        let dir = tmp_dir("corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(dir.join("settings.json.bak").exists());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = tmp_dir("partial");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"mode":"raw","future_field":1}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.mode, RewriteMode::Raw);
        assert_eq!(s.shortcut, default_shortcut());
    }

    #[test]
    fn enum_wire_names_are_stable() {
        let json = serde_json::to_string(&(
            SpeechModel::Canary180mFlash,
            WritingModel::Max,
            RewriteMode::Professional,
            PasteChord::CtrlShiftV,
        ))
        .unwrap();
        assert_eq!(json, r#"["canary180m_flash","max","professional","ctrl_shift_v"]"#);
    }

    #[test]
    fn validation_rejects_bad_values() {
        let s = Settings { shortcut: "cmd+shift+notakey".into(), ..Settings::default() };
        assert!(s.validate().is_err());

        let s = Settings { shortcut: "esc".into(), ..Settings::default() };
        assert!(s.validate().is_err(), "Esc alone is reserved for cancel");

        let s = Settings { language: "xx".into(), ..Settings::default() };
        assert!(s.validate().is_err());

        let mut s = Settings { auto_stop_ms: 50, ..Settings::default() };
        assert!(s.validate().is_err());
        s.auto_stop_ms = 0; // 0 = auto-stop off
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn save_refuses_invalid_settings() {
        let dir = tmp_dir("invalid");
        let s = Settings { language: "xx".into(), ..Settings::default() };
        assert!(s.save(&dir.join("settings.json")).is_err());
        assert!(!dir.join("settings.json").exists());
    }
}
