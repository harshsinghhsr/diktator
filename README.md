# Diktator: offline voice-to-text dictation for macOS and Windows

**Private, on-device dictation for macOS and Windows.** Hold a shortcut, speak, let go, and clean text appears wherever your cursor is: in Slack, Gmail, VS Code, Word, a terminal, anywhere you can type.

Speech recognition and rewriting run entirely on your computer. There is no account, no cloud service, and nothing you say or write leaves your device. Diktator is a free, open-source alternative to Wispr Flow, Superwhisper and built-in dictation, powered by local AI models (NVIDIA Parakeet speech-to-text and a Qwen language model).

<p align="center">
  <a href="docs/media/diktator-demo.mp4"><img src="docs/media/diktator-demo.gif" alt="Diktator demo: spoken words with filler and a self-correction become clean text, then dictation into Slack, Teams, Notes and VS Code" width="800"></a>
  <br>
  <sub><a href="docs/media/diktator-demo.mp4">Watch the full demo video</a> · <a href="https://github.com/harshsinghhsr/diktator/releases/latest">Download for macOS</a></sub>
</p>

- **Works in any app.** Text is pasted at the cursor, and your clipboard (including images and rich text) is put back afterwards.
- **Cleans up as it goes.** Removes "um", "uh", "you know", repeated words and false starts, and applies your self-corrections ("Thursday, no wait, Friday" becomes "Friday").
- **Stays quick on long dictations.** Speech is transcribed and cleaned while you are still talking, so the wait after you stop is about 1.5 s on an M1 for a 30-second or a one-minute dictation alike.
- **Writing styles.** Natural, Professional, Concise, or Raw (exactly what you said).
- **Private and offline.** The only network access is downloading the models you pick, from pinned URLs, verified by SHA-256.

## Install

Download the latest build from the [**Releases**](https://github.com/harshsinghhsr/diktator/releases/latest) page:

- **macOS 13 or later** (Apple Silicon and Intel): open the `.dmg` and drag Diktator to Applications. The app is not notarized yet, so on first launch macOS blocks it: open **System Settings → Privacy & Security** and click **Open Anyway**, or run `xattr -cr /Applications/Diktator.app` in Terminal.
- **Windows 10/11 (x64)**: no prebuilt installer yet; [build it from source](docs/development.md).

Or [build it from source](docs/development.md).

### First run

Diktator lives in the menu bar (macOS) or the notification area (Windows). On first launch its setup window walks you through:

1. **macOS only:** allow **Accessibility**, which lets Diktator paste into other apps.
2. Download the recommended models, about 1.6 GB once: voice detection, NVIDIA Parakeet speech recognition, and the Qwen2.5 writing model.
3. Dictate once. macOS or Windows asks for **microphone** access the first time.

## Use

| Action | What happens |
| --- | --- |
| Hold `⌘ ⇧ F` (macOS) / `Ctrl+Shift+F` (Windows), speak, release | Push-to-talk: the text appears when you let go |
| Tap the shortcut, speak, then tap again or pause | Hands-free: stops on the second tap or after a short silence |
| `Esc` while listening | Cancels; nothing is typed |
| Menu bar / tray → **Paste last dictation** | Pastes your last dictation again |

Change the shortcut, microphone, writing style, models and the pause length in **Settings** (menu bar / tray → Settings).

## Models

Models are downloaded on demand, never bundled with the app.

| Model | Role | Size | License |
| --- | --- | --- | --- |
| NVIDIA Parakeet TDT 0.6B v2 (default) | English speech recognition | 482 MB | CC-BY-4.0 |
| NVIDIA Parakeet TDT 0.6B v3 | 25 European languages | 487 MB | CC-BY-4.0 |
| NVIDIA Canary 180M Flash | English, Spanish, German, French | 154 MB | CC-BY-4.0 |
| Qwen2.5 1.5B Instruct (default) | Rewriting | 1.1 GB | Apache-2.0 |
| Qwen3.5 2B | Rewriting, higher quality | 1.4 GB | Apache-2.0 |
| Silero VAD | Voice detection | 0.6 MB | MIT |

A **Lightweight** option skips the writing model and uses built-in cleanup rules only. See [docs/models.md](docs/models.md) for how each model is used and how they were chosen.

## Performance

Measured on an Apple M1 with 8 GB of RAM, with the default models:

| | Result |
| --- | --- |
| Wait after you stop talking, 26 s dictation | 1.4 s |
| Wait after you stop talking, 56 s dictation | 1.25 s |
| Rewrite of a short sentence | ~0.3 s |
| Dictated questions answered instead of transcribed (72-case eval) | 0 |

Details and how to reproduce them: [docs/benchmarks.md](docs/benchmarks.md).

## Privacy

- Audio is processed in memory and discarded after each dictation.
- Transcripts and rewritten text are never written to disk and never logged; logs contain only timings and lengths.
- The pasted text is marked so clipboard managers and Windows clipboard history skip it.
- No analytics, telemetry or crash reporting.

The full picture, including exactly which code touches the network: [docs/privacy.md](docs/privacy.md).

## Known limitations

- **macOS:** while a password field has focus, macOS blocks synthetic typing; Diktator says so instead of pasting, and **Paste last dictation** works afterwards in a normal field.
- **Windows:** typing into apps running as administrator is blocked by the OS.
- **Windows:** the writing model needs a CPU with AVX2 (2013 or newer); older CPUs fall back to the cleanup rules. On Windows the writing model is currently built without AVX2 optimizations, so rewriting is slower than on a Mac.
- The first launch on a Mac takes about 30 s longer while the GPU compiles its shaders.
- Streaming (word-by-word) transcription and reading text around the cursor are not implemented yet.

## Documentation

- [Architecture](docs/architecture.md): how a dictation flows through the app
- [Models](docs/models.md): the model catalog and how models are chosen
- [Benchmarks](docs/benchmarks.md): latency and quality numbers, and how to measure them
- [Privacy](docs/privacy.md): what stays on the device and why
- [Development](docs/development.md): building, testing and releasing
- [Manual testing](docs/manual-testing.md): the checklist for things automated tests can't cover

## Contributing

Bug reports, fixes and ideas are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md). This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). To report a security issue privately, see [SECURITY.md](SECURITY.md).

## License

Diktator is released under the [MIT License](LICENSE). The models it downloads have their own licenses, and the NVIDIA speech models require attribution; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Built with [Tauri](https://tauri.app), [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) and [llama.cpp](https://github.com/ggml-org/llama.cpp).
