//! Pressing the paste shortcut (⌘V / Ctrl+V, or Ctrl+Shift+V for terminals).

use crate::settings::PasteChord;
use enigo::{Direction, Enigo, Key, Keyboard};
use std::time::Duration;

#[cfg(target_os = "macos")]
const PRIMARY: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PRIMARY: Key = Key::Control;

/// Presses `chord` and always releases every modifier it pressed, even when a
/// later key fails, so no modifier is left stuck down.
pub(crate) fn press_paste(input: &mut Enigo, chord: PasteChord) -> Result<(), String> {
    let modifiers: &[Key] = match chord {
        PasteChord::Standard => &[PRIMARY],
        PasteChord::CtrlShiftV => &[Key::Control, Key::Shift],
    };
    let mut held: Vec<Key> = Vec::new();
    let mut press = || -> enigo::InputResult<()> {
        for m in modifiers {
            input.key(*m, Direction::Press)?;
            held.push(*m);
        }
        // Some apps ignore a chord whose modifier arrives in the same instant as the key.
        std::thread::sleep(Duration::from_millis(12));
        input.key(v_key(), Direction::Click)
    };
    let result = press();
    for m in held.iter().rev() {
        let _ = input.key(*m, Direction::Release);
    }
    result.map_err(|e| e.to_string())
}

/// Windows shortcuts match on the virtual-key code, which is layout-aware already.
#[cfg(not(target_os = "macos"))]
fn v_key() -> Key {
    Key::Other(0x56) // VK_V
}

/// macOS shortcuts match on the character the key types in the current
/// layout, so find the key that types "v" (on Dvorak it is not the QWERTY V).
#[cfg(target_os = "macos")]
fn v_key() -> Key {
    const ANSI_V: u32 = 9;
    Key::Other(layout::keycode_for('v').map(u32::from).unwrap_or(ANSI_V))
}

#[cfg(target_os = "macos")]
mod layout {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        static kTISPropertyUnicodeKeyLayoutData: CFTypeRef;
        fn TISCopyCurrentKeyboardLayoutInputSource() -> CFTypeRef;
        fn TISGetInputSourceProperty(source: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const c_void,
            key_code: u16,
            key_action: u16,
            modifier_state: u32,
            keyboard_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_len: usize,
            actual_len: *mut usize,
            chars: *mut u16,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFRelease(obj: CFTypeRef);
    }

    const KEY_ACTION_DISPLAY: u16 = 3;
    const NO_DEAD_KEYS: u32 = 1;

    /// The virtual key code that types `ch` with no modifiers, if any.
    /// Text Input Sources must be queried on the main thread.
    pub(super) fn keycode_for(ch: char) -> Option<u16> {
        // SAFETY: plain Carbon/CoreFoundation calls. The copied input source is
        // released exactly once; the layout data is borrowed from it and only
        // read while it is alive.
        unsafe {
            let source = TISCopyCurrentKeyboardLayoutInputSource();
            if source.is_null() {
                return None;
            }
            let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
            let found = (!data.is_null())
                .then(|| {
                    let layout = CFDataGetBytePtr(data).cast::<c_void>();
                    let kind = u32::from(LMGetKbdType());
                    (0u16..128).find(|&code| {
                        let (mut dead, mut len, mut out) = (0u32, 0usize, [0u16; 4]);
                        let status = UCKeyTranslate(
                            layout,
                            code,
                            KEY_ACTION_DISPLAY,
                            0,
                            kind,
                            NO_DEAD_KEYS,
                            &mut dead,
                            out.len(),
                            &mut len,
                            out.as_mut_ptr(),
                        );
                        status == 0 && len == 1 && char::from_u32(u32::from(out[0])) == Some(ch)
                    })
                })
                .flatten();
            CFRelease(source);
            found
        }
    }
}
