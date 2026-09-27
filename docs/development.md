# Development

## Prerequisites

| Tool | Version | Check |
| --- | --- | --- |
| Rust (stable) | 1.85 or newer | `rustc --version` |
| Node.js and npm | Node 22 | `node --version` |
| CMake | 3.21 or newer | `cmake --version` |
| macOS: Xcode Command Line Tools | | `xcode-select -p` |
| Windows: Visual Studio 2022 Build Tools (C++ workload) and LLVM | | `where clang` |

Plan for about 10 GB of free disk space: the Rust build directory reaches 3–8 GB, and the test models take about 2 GB. The first build downloads prebuilt sherpa-onnx libraries and compiles llama.cpp, so it needs a network connection and takes a few minutes.

## Run

```bash
npm ci
npm run tauri dev
```

The app starts in the menu bar or tray and opens its setup window on first run. Dictation logs appear in the terminal.

On macOS, when running from a terminal, the Accessibility and microphone permissions apply to the terminal app, not to Diktator.

## Project layout

```text
src/                 React UI: settings window, overlay, typed IPC (ipc.ts)
src-tauri/src/       Rust core (see docs/architecture.md)
src-tauri/examples/  rewrite_eval and asr_bench benchmark tools
src-tauri/evals/     rewrite evaluation cases
scripts/             helper scripts (test model download)
docs/                documentation
```

## Test

```bash
cd src-tauri
cargo test                                       # unit tests; model tests are skipped
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cd .. && npm run build                           # TypeScript type check and frontend build
```

Tests that need real models print `skipped: DIKTATOR_TEST_MODELS not set` and pass. To run them for real, download the test models (about 1.9 GB) once:

```bash
scripts/fetch-test-models.sh                     # into ./test-models
cd src-tauri && DIKTATOR_TEST_MODELS=$PWD/../test-models cargo test
```

Global hotkeys, OS permissions and pasting into other apps can't be exercised by automated tests; use the [manual testing checklist](manual-testing.md) for changes in those areas.

### Windows code on macOS

The Windows clipboard code can't be built on macOS as part of the whole app, because the native dependencies don't cross-compile. CI builds and tests on Windows. For a quick local type check of `insert/`, copy `src-tauri/src/insert/` and `target_app.rs` into a scratch crate with `enigo`, `log`, `serde` and the `windows` crate (same features as `src-tauri/Cargo.toml`), stub `settings::{PasteChord, PasteOverride}`, and run `cargo check --target x86_64-pc-windows-msvc`.

## Benchmarks and evals

See [benchmarks.md](benchmarks.md). Run `rewrite_eval` after any change to `cleanup.rs` or `rewrite/`: the answered count must stay 0.

## Conventions

- **Privacy rules.** Network access only in `download.rs`. Never log, persist or send transcript or rewrite text; log lengths and timings.
- **Threads.** Never block the Tauri main thread for more than about 150 ms; slow Tauri commands are `async`. Pasteboard, keyboard-layout and hotkey registration calls must run on the main thread.
- **Errors.** `anyhow::Result` inside modules; `Result<T, String>` at the Tauri command boundary, with messages written for the user. No `unwrap()` on anything the user or the OS controls.
- **Formatting.** `cargo fmt` (config in `src-tauri/rustfmt.toml`) and clippy with warnings as errors.
- **Tests.** Pure logic (state machine, parsers, segmenting, guard) gets unit tests. Model-backed tests use `testutil::models_root()` so they skip cleanly without models.

## Release

1. Update the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and move the `Unreleased` section of `CHANGELOG.md` under the new version.
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. `.github/workflows/release.yml` builds a macOS `.dmg` and a Windows installer into a draft GitHub release. Review it and publish.

macOS signing and notarization happen when these repository secrets are set: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. Without them the build is unsigned. Windows signing is not configured yet.
