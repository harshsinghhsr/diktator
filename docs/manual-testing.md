# Manual testing

Automated tests can't press global hotkeys in other apps, grant OS permissions, or check where pasted text lands. Run this checklist before a release, and the relevant sections for changes to hotkeys, audio, pasting or the UI. Use one Apple Silicon Mac and one Windows 10/11 PC where possible.

Install from `npm run tauri build`, or use `npm run tauri dev` for a quick pass. Each dictation logs one line such as `dictation: audio_ms=… segments=… tail_asr_ms=… tail_rewrite_ms=… after_stop_ms=…`.

When reporting, include the OS version, CPU, the `after_stop_ms` of three typical dictations, and every unchecked item with what happened.

## 1. First run and permissions

- [ ] The setup window opens on first launch. macOS: no Dock icon, a menu-bar icon is present. Windows: a tray icon is present.
- [ ] The status chips show **Shortcut on** right away, with no permission needed.
- [ ] **(macOS)** "Open Accessibility settings" opens System Settings. After allowing Diktator, the setup step shows as done within about 2 s.
- [ ] "Download models" fetches the three recommended models with live progress, then "Checking…" and "Unpacking…". The status chips then show Speech **ready** and Writing **ready**.
- [ ] Disconnect the network mid-download, then click Download again: it resumes rather than starting from 0%.
- [ ] Closing the settings window leaves the app running. Tray → Quit exits.
- [ ] **(macOS)** With Diktator running, open it again from Finder or Spotlight: the settings window opens.
- [ ] Quit from the tray while a model is still loading for the first time: the app exits without hanging.

## 2. Dictation basics (TextEdit / Notepad first)

- [ ] **Hold** the shortcut, say "hey rahul uh basically I was thinking maybe we can move the meeting tomorrow because I don't think I'll be able to join today", and release. The overlay shows "Listening" with a moving waveform, then "Writing it up…", and clean text appears at the cursor.
- [ ] The waveform follows your voice: flat when silent, large when you speak.
- [ ] A 30–60 s dictation: `after_stop_ms` stays around 1.5 s on Apple Silicon.
- [ ] **Tap** the shortcut, speak, stop talking: the text appears shortly after the pause.
- [ ] Tap, speak, tap again: stops immediately.
- [ ] Esc while listening: nothing is typed, the overlay hides, and Esc works normally in other apps afterwards.
- [ ] Say "what is the capital of France": you get "What is the capital of France?", not an answer.
- [ ] Say "call me at five, no wait, six": you get "…six", not "five".
- [ ] Raw mode inserts the untouched transcript; Professional and Concise visibly change the tone.
- [ ] Hold the shortcut and say nothing: nothing is typed.
- [ ] Press the shortcut while "Writing it up…" shows: nothing breaks and no second recording starts.
- [ ] After a few idle minutes, the first dictation is not noticeably slower than the next one.

## 3. The clipboard is never lost

- [ ] Copy some text, dictate, then paste: the **original** text pastes.
- [ ] Copy an image, dictate, paste into an image editor: the image is intact.
- [ ] Copy formatted text from a browser, dictate, paste into a rich editor: the formatting is intact.
- [ ] With a clipboard manager running (Maccy or Raycast on macOS, Win+V history on Windows), dictate: the dictation does not appear in its history.
- [ ] Copy something new right after a dictation is pasted: your new copy is kept.
- [ ] **(Windows)** Copy some text, dictate, press Ctrl+V twice a second or so apart: the first paste is the dictation (from the app), the second is your original text.

## 4. Real apps

Check each app where the text lands correctly at the cursor:

- [ ] Chrome (Gmail compose, a plain textarea)
- [ ] Safari (macOS) / Edge (Windows)
- [ ] Google Docs
- [ ] Slack
- [ ] Discord
- [ ] Notion
- [ ] VS Code (editor and integrated terminal)
- [ ] Cursor
- [ ] Microsoft Word
- [ ] Terminal.app / iTerm2 (macOS)
- [ ] Windows Terminal and PowerShell (Windows)
- [ ] A non-US keyboard layout (for example French AZERTY or Dvorak): pasting still works.
- [ ] **(Windows)** The overlay never takes focus: the text lands in the app you were typing in.

## 5. Edge cases

- [ ] **(macOS)** Start a dictation, then click into a password field before it finishes: the overlay says a password field has focus and nothing is typed. Paste last dictation works afterwards in a normal field.
- [ ] **(Windows)** Dictate into Notepad running as administrator: the overlay explains that Windows blocks it.
- [ ] Two monitors: the overlay appears on the monitor with the pointer.
- [ ] **(macOS)** A full-screen app: the overlay shows above it.
- [ ] Unplug the selected USB microphone: dictation falls back to the default microphone or shows a clear message.
- [ ] Deny microphone permission and dictate: the overlay says there is no sound from the microphone and where to fix it.
- [ ] Change the shortcut in Settings by pressing a new combo on the keycaps: it works immediately and the old one stops working.
- [ ] Pick a shortcut another app already uses: Settings says it couldn't be registered and the old shortcut keeps working.
- [ ] "Start Diktator when I log in": log out and back in, and Diktator is running.
- [ ] Canary 180M with Spanish selected: Spanish dictation works.
- [ ] Parakeet v3: a non-English dictation works.
- [ ] Lightweight writing: fillers are still removed, with no model delay.
- [ ] Turn off the network and restart: everything still works.

## 6. Resources

- [ ] Idle memory with the default models loaded: note it (expect about 2.5–3 GB).
- [ ] Idle CPU when not dictating: about 0%.
