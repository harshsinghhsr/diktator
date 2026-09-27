# Changelog

All notable changes to Diktator are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0]

First public release.

### Added

- Global push-to-talk and hands-free dictation for macOS 13+ and Windows 10/11, with Esc to cancel.
- On-device speech recognition with NVIDIA Parakeet TDT 0.6B (v2 English, v3 multilingual) and Canary 180M Flash, through sherpa-onnx.
- On-device rewriting with Qwen2.5 1.5B or Qwen3.5 2B through llama.cpp, in Natural, Professional, Concise and Raw styles, with an output guard that falls back to rule-based cleanup.
- Rule-based cleanup of fillers, stutters, repeated phrases and spoken lead-ins.
- Transcription and rewriting while you speak, so the wait after a long dictation stays around 1.5 s on Apple Silicon.
- Model warm-up on key-down, removing the delay after the app sits idle.
- Clipboard paste that restores every clipboard format after the target app has read the text, and keeps dictations out of clipboard history.
- Settings window with a keycap shortcut recorder, model downloads with SHA-256 verification, and first-run setup.
- Floating overlay with a live audio waveform.
- Rewrite evaluation (`rewrite_eval`) and speech benchmark (`asr_bench`) tools.
