//! Dictation controller: the single owner of dictation state. Every input
//! (shortcut, Esc, voice endpoint, settings) arrives as an `Event` on one
//! channel; the pure `step` function decides what it means.

use crate::audio::{self, Session};
use crate::hotkey::{HotkeyService, ShortcutEvent};
use crate::live::{Live, Segmenter};
use crate::models::ModelHub;
use crate::rewrite;
use crate::settings::{PasteOverride, Settings};
use crate::vad::{Endpointer, Vad};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Holding the shortcut at least this long means push-to-talk.
pub const HOLD: Duration = Duration::from_millis(300);
pub const MAX_RECORDING: Duration = Duration::from_secs(300);

/// Below this peak `audio::level` for a whole recording, the microphone is
/// treated as delivering no real signal (permission denied, wrong device, or
/// a device silently producing zeros) rather than the user just not speaking.
const SILENT_MIC_PEAK: f32 = 0.02;

#[cfg(target_os = "macos")]
const NO_MIC_SIGNAL_MESSAGE: &str = "No sound from the microphone. Check System Settings → Privacy → Microphone.";
#[cfg(not(target_os = "macos"))]
const NO_MIC_SIGNAL_MESSAGE: &str = "No sound from the microphone. Check Settings → Privacy → Microphone.";

/// Pure decision so it can be unit-tested without a real audio session.
fn mic_looks_silent(peak_level: f32) -> bool {
    peak_level < SILENT_MIC_PEAK
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", content = "text", rename_all = "snake_case")]
pub enum Overlay {
    Hidden,
    Listening,
    Processing,
    Message(String),
}

pub trait Ui: Send + Sync {
    fn overlay(&self, state: Overlay);
    fn level(&self, value: f32);
    /// Inserts into the focused app. Implementations handle main-thread rules.
    fn insert(&self, text: &str, overrides: &[PasteOverride]) -> Result<(), String>;
    fn open_settings(&self);
    /// Runs `f` on the app's main thread (global shortcuts must be registered there).
    fn run_on_main(&self, f: Box<dyn FnOnce() + Send>);
}

pub enum Event {
    /// Timestamped where the hotkey callback fires, not where it's dequeued:
    /// tap-vs-hold must be measured against the moment the key event
    /// happened, not against whenever the worker gets around to reading it
    /// (it may be blocked opening the microphone).
    Shortcut(ShortcutEvent, Instant),
    /// Toggle-mode silence endpoint from recording session N.
    VadEndpoint(u64),
    Settings(Box<Settings>),
    PasteLast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording { pressed_at: Instant, toggle: bool },
    Processing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Pressed,
    Released,
    Escape,
    VadEndpoint,
    MaxDuration,
    ProcessingDone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Start,
    StopAndProcess,
    Cancel,
}

pub fn step(phase: &mut Phase, input: Input, now: Instant) -> Action {
    match (*phase, input) {
        (Phase::Idle, Input::Pressed) => {
            *phase = Phase::Recording { pressed_at: now, toggle: false };
            Action::Start
        }
        (Phase::Recording { .. }, Input::Escape) => {
            *phase = Phase::Idle;
            Action::Cancel
        }
        (Phase::Recording { pressed_at, toggle: false }, Input::Released) => {
            if now.duration_since(pressed_at) >= HOLD {
                *phase = Phase::Processing;
                Action::StopAndProcess
            } else {
                *phase = Phase::Recording { pressed_at, toggle: true };
                Action::None
            }
        }
        (Phase::Recording { toggle: true, .. }, Input::Pressed)
        | (Phase::Recording { toggle: true, .. }, Input::VadEndpoint)
        | (Phase::Recording { .. }, Input::MaxDuration) => {
            *phase = Phase::Processing;
            Action::StopAndProcess
        }
        (Phase::Processing, Input::ProcessingDone) => {
            *phase = Phase::Idle;
            Action::None
        }
        _ => Action::None,
    }
}

pub struct Controller {
    tx: Sender<Event>,
    hotkeys: Arc<HotkeyService>,
}

impl Controller {
    pub fn start(settings: Settings, hub: Arc<ModelHub>, ui: Arc<dyn Ui>) -> Controller {
        let (tx, rx) = mpsc::channel::<Event>();
        let keys_tx = tx.clone();
        let keys_hub = hub.clone();
        let hotkeys = Arc::new(HotkeyService::start(
            settings.shortcut.clone(),
            move |e| {
                let _ = keys_tx.send(Event::Shortcut(e, Instant::now()));
            },
            move |active| keys_hub.set_hotkey_active(active),
            {
                let ui = ui.clone();
                Arc::new(move |f| ui.run_on_main(f))
            },
        ));
        let worker = Worker {
            rx,
            tx: tx.clone(),
            hub,
            ui,
            hotkeys: hotkeys.clone(),
            settings,
            phase: Phase::Idle,
            session: None,
            live: None,
            segmented: Arc::new(AtomicUsize::new(0)),
            session_id: 0,
            heard: Arc::new(AtomicBool::new(false)),
            peak_level: Arc::new(Mutex::new(0.0)),
            last_text: None,
        };
        thread::Builder::new().name("controller".into()).spawn(move || worker.run()).expect("spawn controller thread");
        Controller { tx, hotkeys }
    }

    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn hotkeys(&self) -> &HotkeyService {
        &self.hotkeys
    }
}

struct Worker {
    rx: Receiver<Event>,
    tx: Sender<Event>,
    hub: Arc<ModelHub>,
    ui: Arc<dyn Ui>,
    hotkeys: Arc<HotkeyService>,
    settings: Settings,
    phase: Phase,
    session: Option<Session>,
    /// Transcribes and rewrites finished segments while the user keeps talking.
    live: Option<Live>,
    /// Samples already handed to `live` as segments; the rest is the tail.
    segmented: Arc<AtomicUsize>,
    session_id: u64,
    heard: Arc<AtomicBool>,
    /// Peak `audio::level` seen this session; used to tell a denied/silent
    /// microphone apart from the user simply not speaking.
    peak_level: Arc<Mutex<f32>>,
    /// Kept in memory only, for "Paste last dictation". Never written to disk.
    last_text: Option<String>,
}

impl Worker {
    fn run(mut self) {
        loop {
            let timeout = match self.phase {
                Phase::Recording { pressed_at, .. } => MAX_RECORDING.saturating_sub(pressed_at.elapsed()),
                _ => Duration::from_secs(3600),
            };
            // `now` is the moment the input actually happened, not the moment
            // it's dequeued: shortcut events carry their own timestamp so a
            // slow mic open (start_recording, below) can't turn a tap into a
            // hold by delaying when Released is measured.
            let (input, now) = match self.rx.recv_timeout(timeout) {
                Ok(Event::Shortcut(ShortcutEvent::Pressed, at)) => (Input::Pressed, at),
                Ok(Event::Shortcut(ShortcutEvent::Released, at)) => (Input::Released, at),
                Ok(Event::Shortcut(ShortcutEvent::Escape, at)) => (Input::Escape, at),
                Ok(Event::VadEndpoint(id)) if id == self.session_id => (Input::VadEndpoint, Instant::now()),
                Ok(Event::VadEndpoint(_)) => continue, // from an earlier session
                Ok(Event::Settings(s)) => {
                    self.settings = *s;
                    continue;
                }
                Ok(Event::PasteLast) => {
                    self.paste_last();
                    continue;
                }
                Err(RecvTimeoutError::Timeout) if matches!(self.phase, Phase::Recording { .. }) => {
                    (Input::MaxDuration, Instant::now())
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            match step(&mut self.phase, input, now) {
                Action::Start => self.start_recording(),
                Action::Cancel => self.cancel(),
                Action::StopAndProcess => {
                    self.finish();
                    step(&mut self.phase, Input::ProcessingDone, Instant::now());
                    self.drain_stale_events();
                }
                Action::None => {}
            }
        }
    }

    fn start_recording(&mut self) {
        let loaded = self.hub.snapshot();
        if loaded.asr.is_none() {
            self.phase = Phase::Idle;
            self.ui.overlay(Overlay::Message("Speech model not ready. Open Diktator Settings.".into()));
            self.ui.open_settings();
            return;
        }
        warm_up(&loaded, self.settings.mode);
        self.session_id += 1;
        let id = self.session_id;
        let Some(asr) = loaded.asr.clone() else { return };
        let live = Live::start(asr, loaded.llm.clone(), self.settings.mode);
        let feed = live.feed();
        self.live = Some(live);
        let segmented = Arc::new(AtomicUsize::new(0));
        self.segmented = segmented.clone();
        let mut segmenter = Segmenter::default();
        let vad = loaded
            .vad_model
            .as_deref()
            .and_then(|p| Vad::new(p).map_err(|e| log::warn!("voice detection unavailable: {e:#}")).ok());
        // Without a VAD we cannot tell silence from speech: assume speech.
        let heard = Arc::new(AtomicBool::new(vad.is_none()));
        self.heard = heard.clone();
        let peak_level = Arc::new(Mutex::new(0.0f32));
        self.peak_level = peak_level.clone();
        let mut endpointer = Endpointer::new(self.settings.auto_stop_ms);
        let tx = self.tx.clone();
        let ui = self.ui.clone();
        let on_window = Box::new(move |w: &[f32]| {
            if let Some(v) = &vad {
                let speech = v.accept(w);
                if speech {
                    heard.store(true, Ordering::Relaxed);
                }
                if endpointer.update(speech) {
                    let _ = tx.send(Event::VadEndpoint(id));
                }
                if let Some(segment) = segmenter.push(w, speech) {
                    segmented.fetch_add(segment.len(), Ordering::Relaxed);
                    feed.push(segment);
                }
            }
            let lvl = audio::level(w);
            let mut peak = peak_level.lock().unwrap();
            if lvl > *peak {
                *peak = lvl;
            }
            drop(peak);
            ui.level(lvl); // every 32 ms window: the overlay draws it as a scrolling waveform
        });
        match Session::start(self.settings.microphone.clone(), on_window) {
            Ok(s) => {
                self.session = Some(s);
                self.hotkeys.set_escape(true);
                self.ui.overlay(Overlay::Listening);
            }
            Err(e) => {
                self.phase = Phase::Idle;
                self.live = None;
                self.ui.overlay(Overlay::Message(format!("{e:#}")));
            }
        }
    }

    fn cancel(&mut self) {
        self.hotkeys.set_escape(false);
        drop(self.session.take());
        drop(self.live.take());
        self.ui.overlay(Overlay::Hidden);
        log::info!("dictation cancelled");
    }

    fn finish(&mut self) {
        self.hotkeys.set_escape(false);
        let Some(session) = self.session.take() else {
            self.ui.overlay(Overlay::Hidden);
            return;
        };
        self.ui.overlay(Overlay::Processing);
        match self.process(session) {
            Ok(()) => self.ui.overlay(Overlay::Hidden),
            Err(msg) => {
                log::warn!("dictation failed: {msg}");
                self.ui.overlay(Overlay::Message(msg));
            }
        }
    }

    fn process(&mut self, session: Session) -> Result<(), String> {
        let stopped = Instant::now();
        let live = self.live.take();
        let samples = session.stop().map_err(|e| format!("{e:#}"))?;
        let audio_ms = samples.len() as u64 * 1000 / audio::SAMPLE_RATE as u64;
        if !self.heard.load(Ordering::Relaxed) || samples.len() < (audio::SAMPLE_RATE / 4) as usize {
            log::info!("dictation: no speech in {audio_ms} ms of audio");
            if mic_looks_silent(*self.peak_level.lock().unwrap()) {
                return Err(NO_MIC_SIGNAL_MESSAGE.into());
            }
            return Ok(());
        }
        let live = live.ok_or("The speech model is not loaded yet.")?;
        // Segments were cut from the front of this same stream; the rest is the tail.
        let tail = samples.get(self.segmented.load(Ordering::Relaxed)..).unwrap_or(&[]).to_vec();
        let done = live.finish(tail).map_err(|e| format!("{e:#}"))?;
        if done.raw.is_empty() {
            log::info!("dictation: empty transcript ({audio_ms} ms audio)");
            return Ok(());
        }
        let t = Instant::now();
        let inserted = self.ui.insert(&done.text, &self.settings.paste_overrides);
        log::info!(
            "dictation: audio_ms={audio_ms} segments={} tail_asr_ms={} tail_rewrite_ms={} insert_ms={} after_stop_ms={} chars_in={} chars_out={} outcome={:?}",
            done.segments,
            done.tail_asr_ms,
            done.tail_rewrite_ms,
            t.elapsed().as_millis(),
            stopped.elapsed().as_millis(),
            done.raw.chars().count(),
            done.text.chars().count(),
            done.outcome,
        );
        self.last_text = Some(done.text);
        inserted
    }

    fn paste_last(&self) {
        if self.phase != Phase::Idle {
            return;
        }
        if let Some(text) = &self.last_text {
            if let Err(msg) = self.ui.insert(text, &self.settings.paste_overrides) {
                self.ui.overlay(Overlay::Message(msg));
            }
        }
    }

    /// Key presses made while we were busy are dropped, not replayed.
    fn drain_stale_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            if let Event::Settings(s) = ev {
                self.settings = *s;
            }
        }
    }
}

/// Models that sat idle get paged out (and llama.cpp's Metal buffers expire),
/// which made the first dictation after a break 2–3 s slower. Touch both on
/// key-down so they are warm again by the time the user stops talking.
fn warm_up(loaded: &crate::models::Loaded, mode: crate::settings::RewriteMode) {
    if let Some(asr) = loaded.asr.clone() {
        let _ = thread::Builder::new().name("warm-asr".into()).spawn(move || {
            let _ = asr.transcribe(&[0.0; audio::SAMPLE_RATE as usize / 2]);
        });
    }
    if let (Some(llm), Some(pm)) = (loaded.llm.clone(), rewrite::prompt_mode(mode)) {
        // Queued ahead of any real rewrite on the engine's own thread; also
        // re-caches this mode's prompt prefix if the mode changed.
        let _ = thread::Builder::new().name("warm-llm".into()).spawn(move || {
            let _ = llm.rewrite(pm, "Okay, sounds good.");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, base: Instant) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn hold_is_push_to_talk() {
        let t0 = Instant::now();
        let mut p = Phase::Idle;
        assert_eq!(step(&mut p, Input::Pressed, t0), Action::Start);
        assert_eq!(step(&mut p, Input::Released, at(900, t0)), Action::StopAndProcess);
        assert_eq!(p, Phase::Processing);
    }

    #[test]
    fn tap_toggles_and_second_press_stops() {
        let t0 = Instant::now();
        let mut p = Phase::Idle;
        step(&mut p, Input::Pressed, t0);
        assert_eq!(step(&mut p, Input::Released, at(120, t0)), Action::None);
        assert!(matches!(p, Phase::Recording { toggle: true, .. }));
        assert_eq!(step(&mut p, Input::Pressed, at(5_000, t0)), Action::StopAndProcess);
        assert_eq!(step(&mut p, Input::Released, at(5_100, t0)), Action::None, "release after stop is ignored");
    }

    #[test]
    fn silence_ends_only_toggle_dictations() {
        let t0 = Instant::now();
        let mut hold = Phase::Idle;
        step(&mut hold, Input::Pressed, t0);
        assert_eq!(step(&mut hold, Input::VadEndpoint, at(2_000, t0)), Action::None, "user is still holding the key");

        let mut toggle = Phase::Idle;
        step(&mut toggle, Input::Pressed, t0);
        step(&mut toggle, Input::Released, at(100, t0));
        assert_eq!(step(&mut toggle, Input::VadEndpoint, at(3_000, t0)), Action::StopAndProcess);
    }

    #[test]
    fn escape_cancels_and_is_ignored_when_idle() {
        let t0 = Instant::now();
        let mut p = Phase::Idle;
        assert_eq!(step(&mut p, Input::Escape, t0), Action::None);
        step(&mut p, Input::Pressed, t0);
        assert_eq!(step(&mut p, Input::Escape, at(50, t0)), Action::Cancel);
        assert_eq!(p, Phase::Idle);
    }

    #[test]
    fn processing_ignores_keys_until_done() {
        let t0 = Instant::now();
        let mut p = Phase::Processing;
        assert_eq!(step(&mut p, Input::Pressed, t0), Action::None);
        assert_eq!(step(&mut p, Input::Escape, t0), Action::None);
        assert_eq!(step(&mut p, Input::ProcessingDone, t0), Action::None);
        assert_eq!(p, Phase::Idle);
    }

    #[test]
    fn max_duration_stops_any_recording() {
        let t0 = Instant::now();
        let mut p = Phase::Idle;
        step(&mut p, Input::Pressed, t0);
        assert_eq!(step(&mut p, Input::MaxDuration, at(300_000, t0)), Action::StopAndProcess);
    }

    #[test]
    fn mic_looks_silent_below_threshold_only() {
        assert!(mic_looks_silent(0.0));
        assert!(mic_looks_silent(SILENT_MIC_PEAK - 0.001));
        assert!(!mic_looks_silent(SILENT_MIC_PEAK));
        assert!(!mic_looks_silent(0.5));
    }

    #[test]
    fn overlay_serialises_to_the_event_contract() {
        assert_eq!(serde_json::to_string(&Overlay::Hidden).unwrap(), r#"{"state":"hidden"}"#);
        assert_eq!(
            serde_json::to_string(&Overlay::Message("hi".into())).unwrap(),
            r#"{"state":"message","text":"hi"}"#
        );
    }
}
