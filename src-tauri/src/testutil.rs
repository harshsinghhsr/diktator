//! Helpers for tests that need real models (see scripts/fetch-test-models.sh).

use crate::catalog::{model_file, ModelId};
use std::path::PathBuf;

pub fn models_root() -> Option<PathBuf> {
    match std::env::var_os("DIKTATOR_TEST_MODELS").map(PathBuf::from) {
        Some(p) if p.is_dir() => Some(p),
        _ => {
            eprintln!("skipped: DIKTATOR_TEST_MODELS not set");
            None
        }
    }
}

/// English clip shipped inside the Canary archive:
/// "Ask not what your country can do for you. Ask what you can do for your country."
pub fn en_wav(root: &std::path::Path) -> PathBuf {
    model_file(root, ModelId::Canary180mFlash, "sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/test_wavs/en.wav")
}

/// Reads a WAV and resamples to 16 kHz mono f32.
pub fn read_wav_16k(path: &std::path::Path) -> Vec<f32> {
    let wave = sherpa_onnx::Wave::read(path.to_str().unwrap()).expect("read wav");
    let rate = wave.sample_rate();
    if rate == 16_000 {
        return wave.samples().to_vec();
    }
    sherpa_onnx::LinearResampler::create(rate, 16_000).expect("resampler").resample(wave.samples(), true)
}
