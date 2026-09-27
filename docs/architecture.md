# Architecture

Diktator is a [Tauri 2](https://tauri.app) app. A Rust core does all the work (hotkeys, audio, speech recognition, rewriting, pasting); two small React webviews provide the settings window and the floating overlay.

## A dictation, end to end

```text
 shortcut down ─► warm up models ─► microphone ─► voice detection ─► pause splitter
                                                                          │ segment
                                                                          ▼
                                          live worker: speech recognition → cleanup → rewrite
 shortcut up ──► stop microphone ─► transcribe the tail ─► rewrite the rest ─► paste at cursor
                                                                                 │
                                                          restore clipboard ◄────┘ once the app has read it
```

1. **Shortcut.** `hotkey.rs` registers the shortcut with the OS through `global-hotkey`. Holding it means push-to-talk; a quick tap starts hands-free mode, which ends on a second tap or after a pause. Esc is registered only while a dictation runs.
2. **Warm-up.** On key-down, `controller.rs` runs a throwaway decode on the speech model and a tiny rewrite, so models that the OS paged out while idle are back in memory by the time you stop talking.
3. **Audio.** `audio.rs` captures the microphone with `cpal`. The real-time callback only downmixes to mono and pushes into a lock-free ring buffer; a worker thread resamples to 16 kHz and hands out 32 ms windows.
4. **Voice detection.** `vad.rs` runs Silero VAD on each window. In hands-free mode, a configurable pause ends the dictation.
5. **Transcribe and rewrite while you talk.** `live.rs` cuts the audio at natural pauses and transcribes each segment in the background. Finished sentences are cleaned and rewritten as soon as the next sentence shows they were not corrected ("Thursday. No wait, Friday."). When you stop, only the last few seconds and sentences are left.
6. **Cleanup and rewrite.** `cleanup.rs` applies deterministic rules (fillers, stutters, lead-ins, capitalization). `rewrite/` then runs a small local LLM with a completion-style prompt, and a guard (`rewrite/guard.rs`) rejects any output that answers the text, invents content or drops too much, falling back to the rule-cleaned text.
7. **Paste.** `insert/` puts the text on the clipboard, presses the paste shortcut, and restores the user's clipboard after the target app has read the text (see below).

## Modules

Rust core, `src-tauri/src/`:

| Module | Responsibility |
| --- | --- |
| `lib.rs` | App setup: windows, tray, managed state, startup and shutdown |
| `controller.rs` | Owns dictation state; the pure `step` function is the state machine |
| `hotkey.rs` | Global shortcut and Esc, registered on the main thread |
| `audio.rs` | Microphone capture and resampling to 16 kHz mono |
| `vad.rs` | Silero voice detection and the hands-free pause rule |
| `live.rs` | Pause splitting, background transcription and rewriting during speech |
| `asr.rs` | Speech recognition with sherpa-onnx (Parakeet, Canary) |
| `cleanup.rs` | Rule-based cleanup that always runs |
| `rewrite/` | LLM prompt (`prompt.rs`), llama.cpp engine (`engine.rs`), output guard (`guard.rs`) |
| `insert/` | Clipboard paste with restore, per platform, and the paste keystroke |
| `target_app.rs` | Facts about the focused app: password field, elevated process, paste override |
| `models.rs` | `ModelHub`: loads and swaps models when settings or downloads change |
| `catalog.rs`, `download.rs` | Model catalog with pinned URLs and hashes; the only network code |
| `settings.rs` | Settings struct, validation, atomic save |
| `commands.rs` | Tauri commands called by the settings window |
| `ui.rs`, `tray.rs`, `permissions.rs` | Overlay and settings windows, tray menu, OS permission panes |

Frontend, `src/`: `settings/` (settings window), `overlay/` (the listening pill and waveform), `ipc.ts` (typed wrappers for commands and events).

## Threads

| Thread | Work |
| --- | --- |
| Main (Tauri event loop) | Windows, tray, hotkey registration, pasting on macOS |
| `controller` | The dictation state machine; the only owner of dictation state |
| `audio` | Resampling and per-window callbacks (VAD, level meter, pause splitter) |
| `live-dictation` | Per-dictation worker: segment transcription and rewrites |
| `rewrite-llm` | Owns the llama.cpp model and context; serves rewrite requests in order |
| loader, downloads | Model (re)loading and downloads, so the UI never waits on them |
| `clipboard-paste` (Windows) | Owns the clipboard for one paste and restores it |

All input reaches the controller through one channel of `Event`s. Shortcut events are timestamped where the OS delivers them, so a slow microphone start can't turn a tap into a hold.

## Dictation state machine

```text
Idle ──press──► Recording (undecided)
Recording (undecided) ──release after ≥ 300 ms──► process   (push-to-talk)
Recording (undecided) ──release before 300 ms──► Recording (hands-free)
Recording (hands-free) ──press, or pause detected──► process
Recording ──Esc──► cancel ──► Idle
Recording ──300 s──► process
Processing ──any key──► ignored
Processing ──done──► Idle
```

`controller::step` implements this as a pure function with unit tests.

## Pasting and the clipboard

Typing text key by key is slow and breaks on some layouts and apps, so Diktator pastes. To keep that from costing the user their clipboard:

1. It snapshots every clipboard item and format (text, rich text, images, files).
2. It offers the dictated text lazily: an `NSPasteboardItem` data provider on macOS, delayed rendering (`WM_RENDERFORMAT`) on Windows. The OS calls back the moment an app reads it.
3. It presses the paste shortcut. On macOS it looks up the key that types "v" in the current layout, so ⌘V works on Dvorak and other layouts.
4. 250 ms after the first read, or 5 s if nothing reads it, the snapshot is restored, unless another app has written to the clipboard in the meantime.

The dictated text is marked as transient so clipboard managers and Windows clipboard history don't keep it.

## Model loading

`ModelHub` holds the voice-detection model path, the speech recognizer and the rewrite engine. When settings change or a download finishes, it reloads what changed on a background thread; a newer request always wins over an older one. At exit it frees the rewrite model before the process ends, because llama.cpp's Metal backend aborts if a model is still alive at exit.

## Frontend ↔ backend contract

Commands (`invoke`): `get_settings`, `save_settings`, `list_microphones`, `model_catalog`, `download_model`, `cancel_download`, `delete_model`, `permission_status`, `open_permission_pane`, `engine_status`.

Events (`listen`): `overlay-state`, `overlay-level` (one per 32 ms audio window), `download-progress`, `engine-status`.

The TypeScript side of this contract is `src/ipc.ts`; keep the two in sync.

## Design decisions

- **Local only.** Privacy is the point of the project. The only network code is `download.rs`.
- **Parakeet for speech.** At about 0.09 s of CPU time per second of audio on an M1, with native punctuation, it matched larger models' accuracy at a fraction of the size. Transcribing during speech hides most of that cost.
- **A completion prompt, not a chat prompt.** Small chat models tend to *answer* a dictated question. Framing the task as continuing `Raw: … / Clean: …` pairs, plus the output guard, brought this to zero in the eval set.
- **Rules before the model.** Fillers, stutters and lead-ins are removed deterministically first. This costs nothing, helps the small model, and still gives clean text when no writing model is loaded.
- **Qwen2.5 1.5B as the default writing model.** In [benchmarks](benchmarks.md), smaller models were faster only because they barely edited the text.
