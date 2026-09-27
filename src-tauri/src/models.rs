//! Keeps the voice-detection model path, speech recognizer and rewrite engine
//! in line with the user's settings. Loading runs on a background thread (one
//! at a time); the controller takes a cheap `snapshot` per dictation.

use crate::asr::{default_threads, SpeechRecognizer};
use crate::catalog::{is_installed, model_file, writing_model_id, ModelId};
use crate::rewrite::{engine::RewriteEngine, llm_supported};
use crate::settings::{Settings, SpeechModel};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadState {
    Missing,
    Loading,
    Ready,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineStatus {
    pub speech: LoadState,
    pub writing: LoadState,
    pub hotkey: bool,
}

#[derive(Clone, Default)]
pub struct Loaded {
    pub vad_model: Option<PathBuf>,
    pub asr: Option<Arc<SpeechRecognizer>>,
    pub llm: Option<Arc<RewriteEngine>>,
}

/// Canary depends on the language; Parakeet does not (empty string).
type AsrKey = (SpeechModel, String);

#[derive(Default)]
struct Slots {
    vad_model: Option<PathBuf>,
    asr: Option<(AsrKey, Arc<SpeechRecognizer>)>,
    llm: Option<(ModelId, Arc<RewriteEngine>)>,
}

pub struct ModelHub {
    root: PathBuf,
    slots: Mutex<Slots>,
    status: Mutex<EngineStatus>,
    on_status: Box<dyn Fn(EngineStatus) + Send + Sync>,
    loader: Mutex<()>,
    generation: AtomicU64,
    /// Set once, before exit, so a `sync` still queued behind `loader` bails
    /// out instead of starting a load after `shutdown` has run.
    closed: AtomicBool,
}

impl ModelHub {
    pub fn new(root: PathBuf, on_status: impl Fn(EngineStatus) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            root,
            slots: Mutex::default(),
            status: Mutex::new(EngineStatus { speech: LoadState::Missing, writing: LoadState::Missing, hotkey: false }),
            on_status: Box::new(on_status),
            loader: Mutex::new(()),
            generation: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn snapshot(&self) -> Loaded {
        let s = self.slots.lock().unwrap();
        Loaded {
            vad_model: s.vad_model.clone(),
            asr: s.asr.as_ref().map(|(_, a)| a.clone()),
            llm: s.llm.as_ref().map(|(_, l)| l.clone()),
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_hotkey_active(&self, active: bool) {
        self.update(|s| s.hotkey = active);
    }

    /// Unloads the writing model (joining its thread) before the process
    /// exits; ggml-metal aborts at exit if a model is still alive.
    ///
    /// A `Loaded` snapshot already handed to an in-flight dictation keeps its
    /// own `Arc<RewriteEngine>` alive until that dictation finishes; this
    /// only stops the hub from starting any *new* load from here on.
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _loading = self.loader.lock().unwrap();
        let engine = self.slots.lock().unwrap().llm.take();
        drop(engine);
    }

    fn update(&self, f: impl FnOnce(&mut EngineStatus)) {
        let snapshot = {
            let mut s = self.status.lock().unwrap();
            f(&mut s);
            s.clone()
        };
        (self.on_status)(snapshot);
    }

    /// Brings loaded models in line with `settings` on a background thread.
    /// If several syncs queue up, only the newest does the work.
    pub fn sync(self: &Arc<Self>, settings: &Settings) -> JoinHandle<()> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let hub = self.clone();
        let settings = settings.clone();
        thread::Builder::new()
            .name("model-loader".into())
            .spawn(move || {
                let _one_at_a_time = hub.loader.lock().unwrap();
                if hub.closed.load(Ordering::SeqCst) {
                    return;
                }
                if hub.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                hub.load_vad();
                hub.load_speech(&settings);
                hub.load_writing(&settings);
            })
            .expect("spawn model loader")
    }

    fn load_vad(&self) {
        let installed = is_installed(&self.root, ModelId::SileroVad);
        let path = model_file(&self.root, ModelId::SileroVad, "silero_vad.onnx");
        self.slots.lock().unwrap().vad_model = installed.then_some(path);
    }

    fn load_speech(&self, settings: &Settings) {
        let model = settings.speech_model;
        let lang = if model == SpeechModel::Canary180mFlash { settings.language.clone() } else { String::new() };
        let key: AsrKey = (model, lang.clone());
        if !is_installed(&self.root, model.into()) {
            self.slots.lock().unwrap().asr = None;
            self.update(|s| s.speech = LoadState::Missing);
            return;
        }
        if self.slots.lock().unwrap().asr.as_ref().is_some_and(|(k, _)| *k == key) {
            self.update(|s| s.speech = LoadState::Ready);
            return;
        }
        self.update(|s| s.speech = LoadState::Loading);
        let lang = if lang.is_empty() { "en".to_string() } else { lang };
        match SpeechRecognizer::load(&self.root, model, &lang, default_threads()) {
            Ok(r) => {
                self.slots.lock().unwrap().asr = Some((key, Arc::new(r)));
                self.update(|s| s.speech = LoadState::Ready);
            }
            Err(e) => {
                log::error!("speech model load failed: {e:#}");
                self.slots.lock().unwrap().asr = None;
                self.update(|s| s.speech = LoadState::Error);
            }
        }
    }

    fn load_writing(&self, settings: &Settings) {
        let Some(id) = writing_model_id(settings.writing_model) else {
            self.slots.lock().unwrap().llm = None;
            self.update(|s| s.writing = LoadState::Ready); // Lightweight = rules only
            return;
        };
        if !llm_supported() {
            log::warn!("this CPU lacks AVX2/FMA; using rule-based cleanup only");
            self.slots.lock().unwrap().llm = None;
            self.update(|s| s.writing = LoadState::Error);
            return;
        }
        if !is_installed(&self.root, id) {
            self.slots.lock().unwrap().llm = None;
            self.update(|s| s.writing = LoadState::Missing);
            return;
        }
        if self.slots.lock().unwrap().llm.as_ref().is_some_and(|(k, _)| *k == id) {
            self.update(|s| s.writing = LoadState::Ready);
            return;
        }
        self.slots.lock().unwrap().llm = None; // free the previous model first
        self.update(|s| s.writing = LoadState::Loading);
        match RewriteEngine::start(model_file(&self.root, id, id.info().files[0]), default_threads()) {
            Ok(engine) => {
                self.slots.lock().unwrap().llm = Some((id, Arc::new(engine)));
                self.update(|s| s.writing = LoadState::Ready);
            }
            Err(e) => {
                log::error!("writing model load failed: {e:#}");
                self.update(|s| s.writing = LoadState::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Settings, WritingModel};

    #[test]
    fn empty_root_reports_missing_and_lightweight_is_ready() {
        let root = std::env::temp_dir().join(format!("diktator-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let seen = Arc::new(Mutex::new(Vec::<EngineStatus>::new()));
        let seen2 = seen.clone();
        let hub = ModelHub::new(root, move |s| seen2.lock().unwrap().push(s));
        hub.sync(&Settings::default()).join().unwrap();
        let st = hub.status();
        assert_eq!(st.speech, LoadState::Missing);
        assert_eq!(st.writing, if llm_supported() { LoadState::Missing } else { LoadState::Error });
        assert!(hub.snapshot().asr.is_none());
        assert!(!seen.lock().unwrap().is_empty(), "status changes are reported");

        let s = Settings { writing_model: WritingModel::Lightweight, ..Settings::default() };
        hub.sync(&s).join().unwrap();
        assert_eq!(hub.status().writing, LoadState::Ready);
    }

    #[test]
    fn loads_real_models_when_present() {
        let Some(root) = crate::testutil::models_root() else { return };
        let hub = ModelHub::new(root, |_| {});
        hub.sync(&Settings::default()).join().unwrap(); // Parakeet v2 + Qwen2.5-1.5B
        let st = hub.status();
        assert_eq!(st.speech, LoadState::Ready);
        assert_eq!(st.writing, LoadState::Ready);
        let loaded = hub.snapshot();
        assert!(loaded.asr.is_some() && loaded.llm.is_some() && loaded.vad_model.is_some());
    }

    #[test]
    fn sync_after_shutdown_does_not_reload() {
        let Some(root) = crate::testutil::models_root() else { return };
        let hub = ModelHub::new(root, |_| {});
        hub.sync(&Settings::default()).join().unwrap();
        assert!(hub.snapshot().llm.is_some(), "sanity: writing model loads before shutdown");

        hub.shutdown();
        assert!(hub.snapshot().llm.is_none(), "shutdown unloads the writing model");

        hub.sync(&Settings::default()).join().unwrap();
        assert!(hub.snapshot().llm.is_none(), "sync after shutdown must not start a new load");
    }
}
