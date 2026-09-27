//! Silero VAD via sherpa-onnx, and the toggle-mode endpoint rule.

use crate::audio::{SAMPLE_RATE, WINDOW};
use anyhow::{anyhow, Result};
use std::path::Path;

pub struct Vad {
    inner: sherpa_onnx::VoiceActivityDetector,
}

impl Vad {
    pub fn new(model: &Path) -> Result<Vad> {
        let silero = sherpa_onnx::SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: 0.5,
            // Short hangover: the Endpointer adds the user's auto-stop delay on top.
            min_silence_duration: 0.1,
            min_speech_duration: 0.1,
            window_size: WINDOW as i32,
            max_speech_duration: 30.0,
        };
        let config = sherpa_onnx::VadModelConfig {
            silero_vad: silero,
            ten_vad: Default::default(),
            sample_rate: SAMPLE_RATE as i32,
            num_threads: 1,
            provider: Some("cpu".into()),
            debug: false,
        };
        let inner = sherpa_onnx::VoiceActivityDetector::create(&config, 30.0)
            .ok_or_else(|| anyhow!("could not load the voice detection model"))?;
        Ok(Vad { inner })
    }

    /// Feeds one 512-sample 16 kHz window; true while speech is detected.
    pub fn accept(&self, window: &[f32]) -> bool {
        self.inner.accept_waveform(window);
        // We only need the live flag; drop queued segments so memory stays flat.
        while self.inner.front().is_some() {
            self.inner.pop();
        }
        self.inner.detected()
    }
}

/// Ends a toggle-mode dictation once speech was heard and then followed by
/// `auto_stop_ms` of continuous silence.
pub struct Endpointer {
    limit: u32,
    heard: bool,
    silent: u32,
    fired: bool,
}

impl Endpointer {
    pub fn new(auto_stop_ms: u32) -> Self {
        let window_ms = (WINDOW as u32 * 1000) / SAMPLE_RATE; // 32
        Self { limit: auto_stop_ms.div_ceil(window_ms), heard: false, silent: 0, fired: false }
    }

    pub fn update(&mut self, speech: bool) -> bool {
        if speech {
            self.heard = true;
            self.silent = 0;
            return false;
        }
        if self.limit == 0 || !self.heard || self.fired {
            return false;
        }
        self.silent += 1;
        if self.silent >= self.limit {
            self.fired = true;
            return true;
        }
        false
    }

    pub fn heard_speech(&self) -> bool {
        self.heard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpointer_needs_speech_before_silence() {
        let mut e = Endpointer::new(700); // 22 windows of 32 ms
        for _ in 0..100 {
            assert!(!e.update(false), "silence alone never ends a dictation");
        }
        assert!(!e.heard_speech());
        assert!(!e.update(true));
        let mut fired_at = None;
        for i in 0..40 {
            if e.update(false) {
                fired_at = Some(i);
                break;
            }
        }
        assert_eq!(fired_at, Some(21), "fires on the 22nd silent window");
        assert!(!e.update(false), "fires only once");
    }

    #[test]
    fn speech_resets_the_silence_count() {
        let mut e = Endpointer::new(320); // 10 windows
        e.update(true);
        for _ in 0..9 {
            assert!(!e.update(false));
        }
        assert!(!e.update(true));
        for _ in 0..9 {
            assert!(!e.update(false));
        }
        assert!(e.update(false));
    }

    #[test]
    fn zero_disables_auto_stop() {
        let mut e = Endpointer::new(0);
        e.update(true);
        for _ in 0..1000 {
            assert!(!e.update(false));
        }
        assert!(e.heard_speech());
    }

    #[test]
    fn silero_detects_speech_in_a_real_clip() {
        let Some(root) = crate::testutil::models_root() else { return };
        let vad = Vad::new(&crate::catalog::model_file(&root, crate::catalog::ModelId::SileroVad, "silero_vad.onnx"))
            .unwrap();
        let mut samples = crate::testutil::read_wav_16k(&crate::testutil::en_wav(&root));
        samples.extend(std::iter::repeat_n(0.0, 16_000 * 2)); // 2 s of silence
        let mut ep = Endpointer::new(700);
        let (mut speech_windows, mut ended) = (0, false);
        for w in samples.as_chunks::<{ crate::audio::WINDOW }>().0 {
            let s = vad.accept(w);
            speech_windows += s as usize;
            ended |= ep.update(s);
        }
        assert!(speech_windows > 30, "{speech_windows}");
        assert!(ended, "the trailing silence ends the dictation");
    }
}
