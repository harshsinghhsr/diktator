//! Puts dictated text at the cursor of whatever app has focus.
//!
//! The text goes on the system clipboard, the paste shortcut is pressed, and
//! the user's own clipboard is put back afterwards. The clipboard entry is
//! offered lazily, so the OS tells us the moment the target app reads it; the
//! restore waits for that read (plus a short grace period) instead of guessing
//! with a fixed delay. If someone else writes to the clipboard in the meantime,
//! their content is left alone. Each platform module implements this with its
//! native clipboard API.

mod keys;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::settings::{PasteChord, PasteOverride};
use std::time::{Duration, Instant};

/// After the target app reads our text, wait this long before restoring: apps
/// often read several times (plain text, then rich text) within a few ms.
pub(crate) const GRACE_AFTER_READ: Duration = Duration::from_millis(250);
/// If nothing reads the text by then, the paste didn't happen; restore anyway.
pub(crate) const GIVE_UP_AFTER: Duration = Duration::from_secs(5);
/// How often a waiting transaction checks whether it is time to restore.
pub(crate) const CHECK_EVERY: Duration = Duration::from_millis(50);

/// When the clipboard can be handed back to the user.
pub(crate) fn restore_due(sent_at: Instant, first_read: Option<Instant>, now: Instant) -> bool {
    match first_read {
        Some(read) => now >= read + GRACE_AFTER_READ,
        None => now >= sent_at + GIVE_UP_AFTER,
    }
}

/// Why a clipboard paste did not go through.
#[derive(Debug)]
pub(crate) enum PasteError {
    /// We could not take the clipboard; typing the text is a fine substitute.
    Clipboard(String),
    /// The clipboard is set but the paste keystroke failed: report it rather
    /// than typing, which could insert the text twice.
    Keystroke(String),
}

/// Inserts `text` into the focused app, picking the paste shortcut from
/// `overrides`. Refuses, with a message for the user, when the OS would
/// silently drop synthetic input. macOS: call on the main thread.
pub fn deliver(text: &str, overrides: &[PasteOverride]) -> Result<(), String> {
    let target = crate::target_app::current();
    if target.secure_input {
        return Err(
            "A password field has focus, so macOS blocks typing. Use \"Paste last dictation\" from the menu bar."
                .into(),
        );
    }
    if target.elevated {
        return Err("The focused app runs as administrator, so Windows blocks typing into it.".into());
    }
    let chord = crate::target_app::choose_chord(target.app_id.as_deref(), overrides);
    insert_text(text, chord)
}

/// Pastes `text` at the cursor and returns once the paste shortcut was sent;
/// the clipboard is restored in the background. Falls back to typing the text
/// when the clipboard is unavailable. macOS: call on the main thread.
pub fn insert_text(text: &str, chord: PasteChord) -> Result<(), String> {
    let mut input =
        enigo::Enigo::new(&enigo::Settings::default()).map_err(|e| format!("Keyboard input is unavailable: {e}"))?;
    #[cfg(target_os = "macos")]
    let pasted = macos::paste(text, chord, &mut input);
    #[cfg(target_os = "windows")]
    let pasted = windows::paste(text, chord, &mut input);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let pasted: Result<(), PasteError> = Err(PasteError::Clipboard("unsupported platform".into()));
    match pasted {
        Ok(()) => Ok(()),
        Err(PasteError::Keystroke(e)) => Err(format!("The paste keystroke failed: {e}")),
        Err(PasteError::Clipboard(e)) => {
            log::warn!("clipboard unavailable ({e}); typing the text instead");
            use enigo::Keyboard;
            input.text(text).map_err(|e| format!("Typing the text failed: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_a_grace_period_after_the_first_read() {
        let t0 = Instant::now();
        let read = t0 + Duration::from_millis(40);
        assert!(!restore_due(t0, Some(read), read));
        assert!(!restore_due(t0, Some(read), read + GRACE_AFTER_READ - Duration::from_millis(1)));
        assert!(restore_due(t0, Some(read), read + GRACE_AFTER_READ));
    }

    #[test]
    fn gives_up_when_nothing_reads_the_text() {
        let t0 = Instant::now();
        assert!(!restore_due(t0, None, t0 + Duration::from_secs(1)));
        assert!(restore_due(t0, None, t0 + GIVE_UP_AFTER));
    }
}
