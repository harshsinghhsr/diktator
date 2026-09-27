# Privacy

Diktator is built so that what you say never leaves your computer.

## What happens to your voice and text

| Data | Where it goes | How long it lives |
| --- | --- | --- |
| Microphone audio | Memory only, processed on this device | Discarded when the dictation ends |
| Transcript and rewritten text | Memory only | The last dictation is kept in memory for **Paste last dictation**, and cleared when the app quits |
| Pasted text | The system clipboard, briefly | Your previous clipboard is restored after the paste |
| Settings | `settings.json` in the app's config folder | Until you delete it |
| Logs | The terminal or system log | Timings and character counts only, never text |

Transcripts and rewrites are never written to disk. The pasted text is marked so clipboard managers skip it (`org.nspasteboard.TransientType` on macOS; `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` and `CanUploadToCloudClipboard` on Windows).

## Network access

The only code that uses the network is `src-tauri/src/download.rs`, and it runs only when you download a model from Settings:

- URLs are pinned in `src-tauri/src/catalog.rs` to fixed GitHub release assets and fixed Hugging Face commits.
- Every file is checked against a pinned SHA-256 hash before it is used.
- Nothing about you or your dictations is sent; these are plain file downloads.

After the models are downloaded, Diktator works with no network connection at all. There is no account, no analytics, no telemetry, no crash reporting and no update check.

The settings window's content security policy only allows loading from the app itself, and fonts are bundled rather than loaded from a web font service.

## Permissions

| Permission | Why | Platform |
| --- | --- | --- |
| Microphone | To hear you | macOS, Windows |
| Accessibility | To press the paste shortcut in other apps | macOS |

The global shortcut uses the operating system's hotkey registration, which needs no extra permission.

## Checking this yourself

- `rg -n "reqwest::" src-tauri/src` should match only `download.rs`.
- `rg -n "info!|warn!|error!|debug!" src-tauri/src` should show no log line that formats transcript or rewrite text.
- Turn off the network after downloading models: everything keeps working.

Found something that contradicts this page? Please report it; see [SECURITY.md](../SECURITY.md).
