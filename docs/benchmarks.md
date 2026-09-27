# Benchmarks

All numbers below were measured on an Apple M1 with 8 GB of RAM, macOS, release builds, with the default models (Parakeet TDT 0.6B v2 and Qwen2.5 1.5B Q4_K_M) unless stated. Treat them as a baseline for spotting regressions, not as guarantees for other machines.

## End-to-end latency

The number that matters is how long you wait after you stop talking. Test clips were streamed through voice detection, pause splitting and the live worker in real time, as a microphone would deliver them.

| Dictation | Wait after you stop, before | Wait after you stop, now |
| --- | --- | --- |
| 26.6 s | about 5 s | 1.4 s |
| 56 s | about 9–10 s | 1.25 s |

"Before" transcribed and rewrote everything after key-up. Moving that work into the time you are speaking makes the wait roughly constant: transcribing the last segment (0.4–0.7 s) plus rewriting the last sentences (about 0.85 s).

Reproduce (needs the test models; see [development](development.md)):

```bash
cd src-tauri
DIKTATOR_TEST_MODELS=$PWD/../test-models DIKTATOR_LIVE_LLM=1 DIKTATOR_LIVE_WAV=/path/to/clip.wav \
  cargo test --release --lib live::tests::transcribes -- --nocapture
```

Each dictation in the app also logs one line with `segments`, `tail_asr_ms`, `tail_rewrite_ms` and `after_stop_ms`.

## Rewrite quality and speed

`src-tauri/evals/rewrite_cases.jsonl` holds 72 short dictations: questions and commands that must not be answered or obeyed, filler-heavy speech, stutters and self-corrections. `evals/long_cases.jsonl` holds six 70–110-word dictations.

```bash
cd src-tauri
cargo run --release --example rewrite_eval -- --models ../test-models            # default model
cargo run --release --example rewrite_eval -- --gguf /path/to/model.gguf         # any GGUF
cargo run --release --example rewrite_eval -- --gguf model.gguf --cases evals/long_cases.jsonl
```

It reports how many outputs answered or obeyed the dictation (must be 0), how many dropped required phrases, how often the guard fell back to rule cleanup, how many fillers were removed, and latency percentiles. It exits with code 1 if anything was answered.

Results for the default model:

| Set | Answered | Missing phrases | Guard fallbacks | Latency p50 / p95 |
| --- | --- | --- | --- | --- |
| 72 short cases | 0 | 0 | 1 | 304 / 969 ms |
| 6 long cases | 0 | 0 | 0 | 2.6 / 2.9 s (whole text at once) |

Alternatives on the same sets:

| Model | Short p50 | Long p50 | Cleans long dictations? |
| --- | --- | --- | --- |
| Qwen2.5 1.5B (default) | 304 ms | 2.8 s | Yes |
| Llama 3.2 1B | 224 ms | 1.8 s | Partly; dropped phrases in 2 cases |
| Qwen2.5 0.5B | 189 ms | 1.6 s | No, returns the input almost unchanged |
| Gemma 3 270M | 140 ms | 1.0 s | No, returns the input almost unchanged |
| SmolLM2 360M | 174 ms | 1.3 s | No; dropped content and looped once |
| SmolLM2 1.7B | 359 ms | 2.2 s | Worse than Qwen2.5 1.5B |

The long-case timings rewrite the whole text in one request. In the app, most of it is rewritten while you speak.

## Speech recognition

`asr_bench` times offline recognition on WAV files (16 kHz mono):

```bash
cd src-tauri
cargo run --release --example asr_bench -- --kind parakeet --dir <model dir> a.wav b.wav
```

| Model | 12.6 s | 26.6 s | 56 s | Per 8 s segment |
| --- | --- | --- | --- | --- |
| Parakeet TDT 0.6B v2 | 1.06 s | 2.31 s | 5.14 s | 0.73 s |
| Canary 180M Flash | 1.78 s | 7.60 s | 29.0 s | not measured |
| Moonshine v2 base | not supported | not supported | not supported | 0.22 s |

The test clips were generated with macOS text-to-speech (`say`), so they measure speed well and accuracy only roughly.

## Cold starts

When the app has been idle for a few minutes, the OS pages models out of memory and llama.cpp's GPU buffers expire. Before the key-down warm-up, the first dictation after a break took 2–2.5 s longer. The warm-up now reloads both models while you start talking, so that cost overlaps your speech.
