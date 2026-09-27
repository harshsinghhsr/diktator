//! Offline speech recognition through sherpa-onnx (CPU, ONNX Runtime).
//! Models: NVIDIA Canary-180M-Flash, Parakeet-TDT-0.6B v2 (English), v3 (25 languages).

use crate::catalog::{is_installed, model_file, ModelId};
use crate::settings::SpeechModel;
use anyhow::{anyhow, bail, Result};
use sherpa_onnx::{OfflineCanaryModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::Path;

pub struct SpeechRecognizer {
    inner: OfflineRecognizer,
    model: SpeechModel,
}

pub fn default_threads() -> i32 {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    (cores / 2).clamp(1, 4) as i32
}

impl SpeechRecognizer {
    pub fn load(root: &Path, model: SpeechModel, language: &str, threads: i32) -> Result<Self> {
        let id = ModelId::from(model);
        let info = id.info();
        if !is_installed(root, id) {
            bail!("The speech model \"{}\" is not downloaded yet.", info.name);
        }
        let path = |i: usize| model_file(root, id, info.files[i]).to_string_lossy().into_owned();
        let mut config = OfflineRecognizerConfig::default();
        match model {
            SpeechModel::Canary180mFlash => {
                // files: encoder, decoder, tokens
                config.model_config.canary = OfflineCanaryModelConfig {
                    encoder: Some(path(0)),
                    decoder: Some(path(1)),
                    src_lang: Some(language.to_string()),
                    tgt_lang: Some(language.to_string()),
                    use_pnc: true,
                };
                config.model_config.tokens = Some(path(2));
            }
            SpeechModel::ParakeetTdtV2 | SpeechModel::ParakeetTdtV3 => {
                // files: encoder, decoder, joiner, tokens
                config.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(path(0)),
                    decoder: Some(path(1)),
                    joiner: Some(path(2)),
                };
                config.model_config.tokens = Some(path(3));
                config.model_config.model_type = Some("nemo_transducer".into());
            }
        }
        config.model_config.num_threads = threads;
        config.model_config.provider = Some("cpu".into());
        let inner = OfflineRecognizer::create(&config)
            .ok_or_else(|| anyhow!("Could not load the speech model \"{}\". Try re-downloading it.", info.name))?;
        Ok(Self { inner, model })
    }

    pub fn model(&self) -> SpeechModel {
        self.model
    }

    pub fn transcribe(&self, samples_16k: &[f32]) -> Result<String> {
        if samples_16k.is_empty() {
            return Ok(String::new());
        }
        let stream = self.inner.create_stream();
        stream.accept_waveform(crate::audio::SAMPLE_RATE as i32, samples_16k);
        self.inner.decode(&stream);
        let result = stream.get_result().ok_or_else(|| anyhow!("speech recognition returned no result"))?;
        Ok(result.text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    #[test]
    fn missing_model_gives_a_readable_error() {
        let root = std::env::temp_dir().join("diktator-asr-none");
        let err = SpeechRecognizer::load(&root, SpeechModel::ParakeetTdtV2, "en", 2).err().unwrap();
        assert!(err.to_string().contains("not downloaded"), "{err}");
    }

    #[test]
    fn threads_are_bounded() {
        let t = default_threads();
        assert!((1..=4).contains(&t));
    }

    fn check(model: SpeechModel) {
        let Some(root) = testutil::models_root() else { return };
        if !crate::catalog::is_installed(&root, model.into()) {
            eprintln!("skipped: {model:?} not in test models");
            return;
        }
        let rec = SpeechRecognizer::load(&root, model, "en", default_threads()).unwrap();
        let samples = testutil::read_wav_16k(&testutil::en_wav(&root));
        let started = std::time::Instant::now();
        let text = rec.transcribe(&samples).unwrap();
        eprintln!("{model:?}: {:?} in {:?}", text, started.elapsed());
        let lower = text.to_lowercase();
        assert!(lower.contains("what your country can do for you"), "{text}");
        assert!(text.ends_with('.') || text.ends_with('!'), "model punctuates: {text}");
        assert_eq!(rec.transcribe(&[]).unwrap(), "");
    }

    #[test]
    fn canary_transcribes_english() {
        check(SpeechModel::Canary180mFlash);
    }

    #[test]
    fn parakeet_v2_transcribes_english() {
        check(SpeechModel::ParakeetTdtV2);
    }
}
