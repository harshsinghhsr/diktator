# Models

Diktator uses three kinds of model, all running on your device. None ships inside the installer: you download what you pick from Settings, and each download is checked against a pinned SHA-256 hash before use. The catalog lives in `src-tauri/src/catalog.rs`.

## Catalog

| Model | Kind | Tier | Languages | Download | RAM (approx.) | License |
| --- | --- | --- | --- | --- | --- | --- |
| Silero VAD | Voice detection | Required | Any | 0.6 MB | 20 MB | MIT |
| NVIDIA Canary 180M Flash | Speech | Lightweight | English, Spanish, German, French | 154 MB | 0.4 GB | CC-BY-4.0 |
| NVIDIA Parakeet TDT 0.6B v2 | Speech | Balanced (default) | English | 482 MB | 1 GB | CC-BY-4.0 |
| NVIDIA Parakeet TDT 0.6B v3 | Speech | Multilingual | 25 European languages, auto-detected | 487 MB | 1 GB | CC-BY-4.0 |
| Qwen2.5 1.5B Instruct (Q4_K_M) | Writing | Balanced (default) | Most major languages | 1.1 GB | 1.5 GB | Apache-2.0 |
| Qwen3.5 2B (Q4_K_M) | Writing | Max quality | Most major languages | 1.4 GB | 1.8 GB | Apache-2.0 |

The **Lightweight** writing option downloads nothing: it uses the rule-based cleanup in `cleanup.rs` only.

Models are stored in the app's data folder:

- macOS: `~/Library/Application Support/com.diktator.app/models/`
- Windows: `%APPDATA%\com.diktator.app\models\`

## How each model is used

- **Voice detection** (Silero, through sherpa-onnx) runs on every 32 ms of audio. It decides where the pauses are, which drives both hands-free auto-stop and the background transcription of finished segments.
- **Speech recognition** (NVIDIA Parakeet or Canary, int8 ONNX through sherpa-onnx, CPU) turns each segment into punctuated text.
- **Writing** (Qwen, GGUF through llama.cpp, Metal GPU on macOS) rewrites the text in your chosen style. The prompt is a fixed few-shot prefix whose KV cache is computed once per style and reused, so each request only processes the new text.

## How the defaults were chosen

Speech models were compared on the same clips, split the way pauses would split a real dictation (Apple M1):

| Model | Whole 56 s clip | Per 8 s segment | Notes |
| --- | --- | --- | --- |
| Parakeet TDT 0.6B v2 | 5.1 s | 0.73 s | Most accurate; the default |
| Moonshine v2 base | not supported | 0.22 s | English only; segments only; similar accuracy on this clip |
| Moonshine v2 tiny | not supported | 0.13 s | Noticeably more errors |
| Canary 180M Flash | 29 s | not measured | Slows down sharply on long audio |

Because Diktator transcribes segments while you speak, Parakeet's per-segment cost is what you actually wait for, and its accuracy wins.

Writing models were compared with the eval in `src-tauri/evals/` (see [benchmarks](benchmarks.md)). SmolLM2 (135M, 360M, 1.7B), Qwen2.5 0.5B, Qwen3 0.6B, Qwen3.5 0.8B, Gemma 3 (270M, 1B) and Llama 3.2 1B were all tried. The models under 1B were 2–3× faster but returned long dictations almost unchanged. Qwen2.5 1.5B was the only one that reliably removed fillers and applied self-corrections without answering or dropping content.

## Adding a model

1. Add an entry to `CATALOG` in `src-tauri/src/catalog.rs`: a pinned URL (a release asset or a Hugging Face `resolve/<commit>` URL), its SHA-256, size and the files it unpacks to.
2. For a speech model, add its sherpa-onnx configuration to `asr.rs`. For a writing model, map it in `writing_model_id`.
3. Measure it. Speech: `cargo run --release --example asr_bench -- --kind parakeet --dir <model dir> clip.wav`. Writing: `cargo run --release --example rewrite_eval -- --gguf <file.gguf>`, plus the long cases (`--cases evals/long_cases.jsonl`).
4. Add its license to `THIRD_PARTY_NOTICES.md`.

Writing models need standard attention layers to benefit from the cached prompt prefix. Hybrid or recurrent architectures (Qwen3.5, LFM2) currently rebuild the prefix on every request, which costs a few hundred milliseconds.
