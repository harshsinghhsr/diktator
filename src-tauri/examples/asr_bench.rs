//! Times offline speech recognition on WAV files for one model directory.
//! Usage: asr_bench --kind parakeet|canary|moonshine --dir MODEL_DIR [--threads N] WAV...
//! Prints load time, then per file: audio length, best-of-3 decode time, real-time factor, transcript.

use sherpa_onnx::{
    OfflineCanaryModelConfig, OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig, Wave,
};
use std::path::Path;
use std::time::{Duration, Instant};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// First file in `dir` whose name contains `part` (model archives name files differently).
fn find(dir: &Path, part: &str) -> Option<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    names.sort();
    names
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(part)))
        .map(|p| p.to_string_lossy().into_owned())
}

fn main() {
    let (Some(kind), Some(dir)) = (arg("--kind"), arg("--dir")) else {
        eprintln!("usage: asr_bench --kind parakeet|canary|moonshine --dir MODEL_DIR [--threads N] WAV...");
        std::process::exit(2);
    };
    let dir = Path::new(&dir);
    let threads: i32 = arg("--threads").and_then(|t| t.parse().ok()).unwrap_or_else(diktator_lib::asr::default_threads);
    let mut config = OfflineRecognizerConfig::default();
    match kind.as_str() {
        "parakeet" => {
            config.model_config.transducer = OfflineTransducerModelConfig {
                encoder: find(dir, "encoder"),
                decoder: find(dir, "decoder"),
                joiner: find(dir, "joiner"),
            };
            config.model_config.model_type = Some("nemo_transducer".into());
        }
        "canary" => {
            config.model_config.canary = OfflineCanaryModelConfig {
                encoder: find(dir, "encoder"),
                decoder: find(dir, "decoder"),
                src_lang: Some("en".into()),
                tgt_lang: Some("en".into()),
                use_pnc: true,
            };
        }
        "moonshine" => {
            config.model_config.moonshine = OfflineMoonshineModelConfig {
                encoder: find(dir, "encoder"),
                merged_decoder: find(dir, "decoder"),
                ..Default::default()
            };
        }
        other => {
            eprintln!("unknown --kind {other}");
            std::process::exit(2);
        }
    }
    config.model_config.tokens = find(dir, "tokens");
    config.model_config.num_threads = threads;
    config.model_config.provider = Some("cpu".into());

    let t = Instant::now();
    let Some(rec) = OfflineRecognizer::create(&config) else {
        eprintln!("could not load model from {}", dir.display());
        std::process::exit(2);
    };
    println!("{kind} {} threads={threads} load={:?}", dir.display(), t.elapsed());

    let wavs: Vec<String> = std::env::args().skip(1).filter(|a| a.ends_with(".wav")).collect();
    for wav in wavs {
        let Some(w) = Wave::read(&wav) else {
            eprintln!("cannot read {wav}");
            continue;
        };
        let audio = w.samples().len() as f64 / w.sample_rate() as f64;
        let (mut best, mut text) = (Duration::MAX, String::new());
        for _ in 0..3 {
            let t = Instant::now();
            let stream = rec.create_stream();
            stream.accept_waveform(w.sample_rate(), w.samples());
            rec.decode(&stream);
            best = best.min(t.elapsed());
            text = stream.get_result().map(|r| r.text).unwrap_or_default();
        }
        println!(
            "  {:>5.1}s audio  decode {:>5} ms  RTF {:.3}  | {}",
            audio,
            best.as_millis(),
            best.as_secs_f64() / audio,
            text.trim()
        );
    }
}
