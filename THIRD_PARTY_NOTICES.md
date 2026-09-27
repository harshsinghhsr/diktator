# Third-party notices

Diktator is MIT-licensed (see [LICENSE](LICENSE)). It builds on the open-source software and models below, which keep their own licenses.

## Models (downloaded by the user at runtime, not bundled)

| Model | License | Source |
| --- | --- | --- |
| NVIDIA Parakeet TDT 0.6B v2 | CC-BY-4.0 | <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2> |
| NVIDIA Parakeet TDT 0.6B v3 | CC-BY-4.0 | <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3> |
| NVIDIA Canary 180M Flash | CC-BY-4.0 | <https://huggingface.co/nvidia/canary-180m-flash> |
| Qwen2.5 1.5B Instruct | Apache-2.0 | <https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct> |
| Qwen3.5 2B | Apache-2.0 | <https://huggingface.co/Qwen/Qwen3.5-2B> |
| Silero VAD | MIT | <https://github.com/snakers4/silero-vad> |

The NVIDIA models are used in the int8 ONNX conversions published by the sherpa-onnx project. CC-BY-4.0 attribution: "Parakeet TDT 0.6B v2/v3 and Canary 180M Flash by NVIDIA, licensed under CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0/>). Converted to ONNX and quantized by the k2-fsa/sherpa-onnx project."

## Libraries compiled into the app

| Library | License |
| --- | --- |
| Tauri and its plugins (autostart, single-instance) | MIT or Apache-2.0 |
| sherpa-onnx | Apache-2.0 |
| ONNX Runtime (linked statically by sherpa-onnx) | MIT |
| llama.cpp / ggml (through the `llama-cpp-2` crate) | MIT |
| global-hotkey | MIT or Apache-2.0 |
| cpal | Apache-2.0 |
| enigo | MIT |
| rtrb | MIT or Apache-2.0 |
| reqwest | MIT or Apache-2.0 |
| objc2, the objc2 framework crates, dispatch2 | MIT (objc2); Zlib, Apache-2.0 or MIT (the others) |
| windows (windows-rs) | MIT or Apache-2.0 |
| tauri-nspanel | MIT or Apache-2.0 |
| React | MIT |
| Bricolage Grotesque font (via Fontsource) | SIL Open Font License 1.1 |

This table lists the direct dependencies. Their transitive dependencies are listed in `src-tauri/Cargo.lock` and `package-lock.json`, and each one ships its full license text in its source. To generate a complete report, run [`cargo about`](https://github.com/EmbarkStudios/cargo-about) in `src-tauri` and a tool such as `npx license-checker` at the repository root.
