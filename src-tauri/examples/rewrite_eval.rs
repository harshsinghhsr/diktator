//! Runs the full post-ASR text pipeline over evals/rewrite_cases.jsonl.
//! Exit 1 if any output answered/obeyed a dictated question or command.

use diktator_lib::catalog::{model_file, ModelId};
use diktator_lib::rewrite::{engine::RewriteEngine, finalize, Outcome};
use diktator_lib::settings::RewriteMode;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Deserialize)]
struct Case {
    mode: RewriteMode,
    raw: String,
    must: Vec<String>,
    never: Vec<String>,
}

const FILLERS: [&str; 6] = ["um", "uh", "basically", "like", "hmm", "maybe"];

fn count_fillers(s: &str) -> usize {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| FILLERS.contains(&w.to_lowercase().as_str()))
        .count()
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// `--gguf FILE` benchmarks any GGUF; otherwise `--model` picks a catalog model under `--models`.
fn model_path() -> PathBuf {
    if let Some(p) = arg("--gguf") {
        return p.into();
    }
    let models: PathBuf = match arg("--models").or_else(|| std::env::var("DIKTATOR_TEST_MODELS").ok()) {
        Some(p) => p.into(),
        None => {
            eprintln!("pass --gguf FILE, --models DIR, or set DIKTATOR_TEST_MODELS");
            std::process::exit(2);
        }
    };
    let (id, file) = match arg("--model").as_deref() {
        None | Some("qwen25_1_5b") => (ModelId::Qwen25_1_5b, "qwen2.5-1.5b-instruct-q4_k_m.gguf"),
        Some("qwen35_2b") => (ModelId::Qwen35_2b, "Qwen_Qwen3.5-2B-Q4_K_M.gguf"),
        Some(other) => {
            eprintln!("unknown --model {other}");
            std::process::exit(2);
        }
    };
    model_file(&models, id, file)
}

fn main() {
    let path = model_path();
    let cases_path = arg("--cases")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/rewrite_cases.jsonl"));
    let text = std::fs::read_to_string(&cases_path).unwrap_or_else(|e| {
        eprintln!("read {}: {e}", cases_path.display());
        std::process::exit(2);
    });
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).unwrap_or_else(|e| panic!("line {}: {e}", i + 1)))
        .collect();

    let t = Instant::now();
    let engine = RewriteEngine::start(path.clone(), diktator_lib::asr::default_threads()).unwrap_or_else(|e| {
        eprintln!("load model: {e:#}");
        std::process::exit(2);
    });
    println!("model {} ready in {:?}\n", path.display(), t.elapsed());

    let (mut answered, mut dropped, mut fell_back) = (0, 0, 0);
    let (mut fillers_in, mut fillers_out) = (0, 0);
    let mut latencies = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let t = Instant::now();
        let (out, outcome) = finalize(&c.raw, c.mode, Some(&engine));
        let ms = t.elapsed().as_millis();
        latencies.push(ms);
        let lower = out.to_lowercase();
        let is_answer = c.never.iter().any(|n| lower.contains(&n.to_lowercase()));
        let missing: Vec<&String> = c.must.iter().filter(|m| !lower.contains(&m.to_lowercase())).collect();
        answered += is_answer as usize;
        dropped += !missing.is_empty() as usize;
        fell_back += matches!(outcome, Outcome::FellBack(_)) as usize;
        fillers_in += count_fillers(&c.raw);
        fillers_out += count_fillers(&out);
        let flag = if is_answer {
            "ANSWERED"
        } else if !missing.is_empty() {
            "missing"
        } else {
            "ok"
        };
        println!("{:>2} {:>5}ms {:<8} {:?}\n     {}\n  -> {}", i + 1, ms, flag, outcome, c.raw, out);
        if !missing.is_empty() {
            println!("     missing: {missing:?}");
        }
    }
    // Free the model before exiting: ggml-metal asserts if one is alive at exit.
    drop(engine);
    latencies.sort_unstable();
    let n = cases.len();
    println!("\n== {n} cases ==");
    println!("answered/obeyed : {answered}   (must be 0)");
    println!("missing phrases : {dropped}");
    println!("guard fallbacks : {fell_back}");
    println!("fillers removed : {} of {}", fillers_in.saturating_sub(fillers_out), fillers_in);
    println!("latency p50/p95 : {} / {} ms", percentile(&latencies, 0.5), percentile(&latencies, 0.95));
    if answered > 0 {
        std::process::exit(1);
    }
}
