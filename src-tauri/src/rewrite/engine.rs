//! Embedded llama.cpp rewrite engine. `LlamaContext` borrows `LlamaModel`,
//! so both live on one worker thread that owns them for the engine's life;
//! callers talk to it over a channel. Dropping `RewriteEngine` ends the thread.

use super::prompt::{self, Mode};
use anyhow::{anyhow, Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;

const N_CTX: u32 = 2048;

/// llama.cpp's backend is process-global: `LlamaBackend::init` fails if one
/// already exists, and dropping it frees llama.cpp for everyone. Keep exactly
/// one for the life of the process so engines can be replaced (model switch)
/// while an old one is still finishing a request.
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
static BACKEND_INIT: Mutex<()> = Mutex::new(());

fn backend() -> Result<&'static LlamaBackend> {
    let _guard = BACKEND_INIT.lock().unwrap();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let mut b = LlamaBackend::init()?;
    b.void_logs();
    Ok(BACKEND.get_or_init(|| b))
}
const N_BATCH: u32 = 1024;

struct Job {
    mode: Mode,
    raw: String,
    reply: mpsc::Sender<Result<String>>,
}

pub struct RewriteEngine {
    tx: Option<mpsc::Sender<Job>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for RewriteEngine {
    /// Closes the channel and waits for the worker to free the model and
    /// context. ggml's Metal backend asserts at process exit if a model is
    /// still alive, so an engine must never outlive `main` or `app.exit`.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl RewriteEngine {
    /// Loads the model on a dedicated thread. Returns once the model is
    /// loaded, the Natural prefix is cached and one warm-up request has run
    /// (the first generation after load is otherwise ~2x slower).
    pub fn start(model_path: PathBuf, n_threads: i32) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let handle = thread::Builder::new()
            .name("rewrite-llm".into())
            .spawn(move || worker(model_path, n_threads, rx, ready_tx))
            .context("spawn rewrite thread")?;
        ready_rx.recv().context("rewrite thread died during load")??;
        Ok(Self { tx: Some(tx), worker: Some(handle) })
    }

    /// Blocking. Returns the model's output line, unguarded.
    pub fn rewrite(&self, mode: Mode, raw: &str) -> Result<String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .as_ref()
            .ok_or_else(|| anyhow!("rewrite engine stopped"))?
            .send(Job { mode, raw: raw.to_string(), reply })
            .map_err(|_| anyhow!("rewrite thread stopped"))?;
        rx.recv().map_err(|_| anyhow!("rewrite thread stopped"))?
    }
}

fn worker(model_path: PathBuf, n_threads: i32, rx: mpsc::Receiver<Job>, ready: mpsc::Sender<Result<()>>) {
    let setup = (|| -> Result<(&'static LlamaBackend, LlamaModel)> {
        let backend = backend()?;
        // Offload every layer to Metal on macOS; ignored by CPU-only builds.
        let params = LlamaModelParams::default().with_n_gpu_layers(99);
        let model = LlamaModel::load_from_file(backend, &model_path, &params)
            .with_context(|| format!("load {}", model_path.display()))?;
        Ok((backend, model))
    })();
    let (backend, model) = match setup {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_BATCH)
        .with_n_threads(n_threads)
        .with_n_threads_batch(n_threads);
    let mut ctx = match model.new_context(backend, ctx_params) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e.into()));
            return;
        }
    };
    let mut cache = PrefixCache::default();
    let warm = cache
        .ensure(&model, &mut ctx, Mode::Natural)
        .and_then(|len| generate(&model, &mut ctx, len, "okay so this is a quick warm up"));
    if let Err(e) = warm {
        let _ = ready.send(Err(e));
        return;
    }
    let _ = ready.send(Ok(()));

    while let Ok(job) = rx.recv() {
        let result = cache
            .ensure(&model, &mut ctx, job.mode)
            .and_then(|prefix_len| generate(&model, &mut ctx, prefix_len, &job.raw));
        let _ = job.reply.send(result);
    }
}

#[derive(Default)]
struct PrefixCache {
    mode: Option<Mode>,
    len: usize,
}

impl PrefixCache {
    /// Leaves exactly the prefix for `mode` in sequence 0 and returns its
    /// token count. Reuses the cached prefix when possible; recurrent/hybrid
    /// models (e.g. Qwen3.5) cannot drop a partial sequence, so they
    /// re-evaluate it (~0.5 s).
    fn ensure(&mut self, model: &LlamaModel, ctx: &mut LlamaContext, mode: Mode) -> Result<usize> {
        if self.mode == Some(mode) {
            let trimmed = ctx.clear_kv_cache_seq(Some(0), Some(self.len as u32), None)?;
            if trimmed {
                return Ok(self.len);
            }
        }
        // Cleared before the rebuild: if decode_tokens below fails, the cache
        // must not claim `mode` is still cached when the KV cache holds nothing.
        self.mode = None;
        ctx.clear_kv_cache();
        let tokens = model.str_to_token(&prompt::prefix(mode), AddBos::Always)?;
        decode_tokens(ctx, &tokens, 0, false)?;
        self.mode = Some(mode);
        self.len = tokens.len();
        Ok(self.len)
    }
}

/// Decodes `tokens` from position `start` in chunks of N_BATCH, requesting
/// logits for the final token when `want_last_logits`.
fn decode_tokens(ctx: &mut LlamaContext, tokens: &[LlamaToken], start: usize, want_last_logits: bool) -> Result<()> {
    let mut batch = LlamaBatch::new(N_BATCH as usize, 1);
    let last = tokens.len().saturating_sub(1);
    for (chunk_i, chunk) in tokens.chunks(N_BATCH as usize).enumerate() {
        batch.clear();
        for (j, tok) in chunk.iter().enumerate() {
            let i = chunk_i * N_BATCH as usize + j;
            batch.add(*tok, (start + i) as i32, &[0], want_last_logits && i == last)?;
        }
        ctx.decode(&mut batch)?;
    }
    Ok(())
}

fn generate(model: &LlamaModel, ctx: &mut LlamaContext, prefix_len: usize, raw: &str) -> Result<String> {
    let suffix = model.str_to_token(&prompt::suffix(raw), AddBos::Never)?;
    if prefix_len + suffix.len() * 3 + 16 > N_CTX as usize {
        return Err(anyhow!("dictation too long for the rewrite model"));
    }
    decode_tokens(ctx, &suffix, prefix_len, true)?;
    let first_pos = prefix_len + suffix.len();
    // Rewrites are never much longer than the input.
    let max_new = suffix.len() * 2 + 16;
    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut out = String::new();
    let mut batch = LlamaBatch::new(1, 1);
    let mut logits_idx = (suffix.len() - 1) as i32;
    for step in 0..max_new {
        let tok = sampler.sample(ctx, logits_idx);
        if model.is_eog_token(tok) {
            break;
        }
        let piece = model.token_to_piece(tok, &mut decoder, false, None)?;
        if let Some(i) = piece.find('\n') {
            out.push_str(&piece[..i]);
            break;
        }
        out.push_str(&piece);
        batch.clear();
        batch.add(tok, (first_pos + step) as i32, &[0], true)?;
        ctx.decode(&mut batch)?;
        logits_idx = 0;
    }
    Ok(out.trim().to_string())
}
