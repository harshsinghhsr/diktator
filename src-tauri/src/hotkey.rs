//! Global shortcut: reports press and release of the dictation shortcut, and
//! Esc while a dictation is running. Built on `global-hotkey`, whose manager
//! must live on the thread that runs the app's event loop (the main thread on
//! macOS; a thread with a message loop on Windows), so every registration is
//! dispatched to the main thread. Registered combos are swallowed by the OS and
//! never reach the focused app, which is why Esc is only registered mid-dictation.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::cell::RefCell;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutEvent {
    Pressed,
    Released,
    Escape,
}

/// Runs a closure on the app's main thread (Tauri's `run_on_main_thread`).
pub type MainThread = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// Parses the stored shortcut format: `+`-separated, case-insensitive, any
/// modifiers (`cmd`, `ctrl`, `alt`, `shift`) then exactly one key, e.g.
/// "cmd+shift+f" or "ctrl+alt+space". Key names match the settings recorder.
pub fn parse_shortcut(s: &str) -> Result<HotKey, String> {
    let invalid = |why: &str| format!("Shortcut \"{s}\" is not valid: {why}.");
    let mut mods = Modifiers::empty();
    let mut key = None;
    for part in s.split('+').map(|p| p.trim().to_lowercase()) {
        let m = match part.as_str() {
            "cmd" | "command" | "meta" | "super" | "win" => Some(Modifiers::SUPER),
            "ctrl" | "control" => Some(Modifiers::CONTROL),
            "alt" | "opt" | "option" => Some(Modifiers::ALT),
            "shift" => Some(Modifiers::SHIFT),
            _ => None,
        };
        match (m, key) {
            (Some(m), None) => mods |= m,
            (None, None) => key = Some(key_code(&part).ok_or_else(|| invalid(&format!("unknown key \"{part}\"")))?),
            (_, Some(_)) => return Err(invalid("put modifiers first and use exactly one key")),
        }
    }
    let key = key.ok_or_else(|| invalid("it needs a key, not just modifiers"))?;
    Ok(HotKey::new(Some(mods), key))
}

fn key_code(name: &str) -> Option<Code> {
    #[rustfmt::skip]
    const LETTERS: [Code; 26] = [
        Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF, Code::KeyG, Code::KeyH, Code::KeyI,
        Code::KeyJ, Code::KeyK, Code::KeyL, Code::KeyM, Code::KeyN, Code::KeyO, Code::KeyP, Code::KeyQ, Code::KeyR,
        Code::KeyS, Code::KeyT, Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX, Code::KeyY, Code::KeyZ,
    ];
    #[rustfmt::skip]
    const DIGITS: [Code; 10] = [
        Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4,
        Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
    ];
    #[rustfmt::skip]
    const NUMPAD: [Code; 10] = [
        Code::Numpad0, Code::Numpad1, Code::Numpad2, Code::Numpad3, Code::Numpad4,
        Code::Numpad5, Code::Numpad6, Code::Numpad7, Code::Numpad8, Code::Numpad9,
    ];
    #[rustfmt::skip]
    const FUNCTION: [Code; 24] = [
        Code::F1, Code::F2, Code::F3, Code::F4, Code::F5, Code::F6, Code::F7, Code::F8, Code::F9, Code::F10,
        Code::F11, Code::F12, Code::F13, Code::F14, Code::F15, Code::F16, Code::F17, Code::F18, Code::F19,
        Code::F20, Code::F21, Code::F22, Code::F23, Code::F24,
    ];
    let single = |c: char| name.len() == 1 && name.starts_with(c);
    if let Some(c) = name.chars().next().filter(|_| name.len() == 1) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[(c as u8 - b'a') as usize]);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(DIGITS[d as usize]);
        }
    }
    if let Some(n) = name.strip_prefix("keypad").or_else(|| name.strip_prefix("num")) {
        return n.parse::<usize>().ok().and_then(|i| NUMPAD.get(i).copied());
    }
    if let Some(n) = name.strip_prefix('f') {
        if let Ok(i) = n.parse::<usize>() {
            return (1..=24).contains(&i).then(|| FUNCTION[i - 1]);
        }
    }
    Some(match name {
        "space" => Code::Space,
        "return" | "enter" => Code::Enter,
        "tab" => Code::Tab,
        "backspace" => Code::Backspace,
        "forwarddelete" | "delete" | "del" => Code::Delete,
        "insert" | "ins" => Code::Insert,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "left" => Code::ArrowLeft,
        "right" => Code::ArrowRight,
        "up" => Code::ArrowUp,
        "down" => Code::ArrowDown,
        "esc" | "escape" => Code::Escape,
        "minus" => Code::Minus,
        "equal" | "equals" => Code::Equal,
        "leftbracket" => Code::BracketLeft,
        "rightbracket" => Code::BracketRight,
        "backslash" => Code::Backslash,
        "semicolon" => Code::Semicolon,
        "quote" => Code::Quote,
        "comma" => Code::Comma,
        "period" => Code::Period,
        "slash" => Code::Slash,
        "grave" | "backtick" => Code::Backquote,
        _ if single('-') => Code::Minus,
        _ if single('=') => Code::Equal,
        _ if single(',') => Code::Comma,
        _ if single('.') => Code::Period,
        _ if single('/') => Code::Slash,
        _ => return None,
    })
}

fn escape_key() -> HotKey {
    HotKey::new(None, Code::Escape)
}

/// Ids currently registered, read by the OS event handler to classify events.
#[derive(Default)]
struct Registered {
    shortcut: Option<HotKey>,
    escape: bool,
}

pub(crate) fn classify(
    id: u32,
    state: HotKeyState,
    shortcut: Option<u32>,
    escape: Option<u32>,
) -> Option<ShortcutEvent> {
    if Some(id) == shortcut {
        return Some(match state {
            HotKeyState::Pressed => ShortcutEvent::Pressed,
            HotKeyState::Released => ShortcutEvent::Released,
        });
    }
    (Some(id) == escape && state == HotKeyState::Pressed).then_some(ShortcutEvent::Escape)
}

thread_local! {
    // Only ever touched on the main thread (see module docs).
    static MANAGER: RefCell<Option<GlobalHotKeyManager>> = const { RefCell::new(None) };
}

/// Runs `f` with the main thread's manager, creating it on first use.
fn with_manager<R>(f: impl FnOnce(&GlobalHotKeyManager) -> Result<R, String>) -> Result<R, String> {
    MANAGER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(GlobalHotKeyManager::new().map_err(|e| format!("Global shortcuts are unavailable: {e}"))?);
        }
        f(slot.as_ref().expect("just created"))
    })
}

pub struct HotkeyService {
    main: MainThread,
    registered: Arc<Mutex<Registered>>,
    on_active: Arc<dyn Fn(bool) + Send + Sync>,
}

impl HotkeyService {
    pub fn start(
        shortcut: String,
        on_event: impl Fn(ShortcutEvent) + Send + Sync + 'static,
        on_active: impl Fn(bool) + Send + Sync + 'static,
        main: MainThread,
    ) -> HotkeyService {
        let registered = Arc::new(Mutex::new(Registered::default()));
        let seen = registered.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            let r = seen.lock().unwrap();
            let ids = (r.shortcut.map(|h| h.id()), r.escape.then(|| escape_key().id()));
            drop(r);
            if let Some(ev) = classify(e.id, e.state, ids.0, ids.1) {
                on_event(ev);
            }
        }));
        let service = HotkeyService { main, registered, on_active: Arc::new(on_active) };
        let (registered, on_active) = (service.registered.clone(), service.on_active.clone());
        (service.main)(Box::new(move || {
            let result = parse_shortcut(&shortcut).and_then(|h| {
                with_manager(|m| m.register(h).map_err(|e| format!("Could not use the shortcut: {e}")))?;
                registered.lock().unwrap().shortcut = Some(h);
                Ok(())
            });
            if let Err(e) = &result {
                log::warn!("{e}");
            }
            on_active(result.is_ok());
        }));
        service
    }

    /// Swaps the shortcut, keeping the old one if the new one can't be registered.
    /// Blocks up to 2 s for the main thread, so never call it *from* the main thread.
    pub fn set_shortcut(&self, shortcut: &str) -> Result<(), String> {
        let new = parse_shortcut(shortcut)?;
        let (registered, on_active) = (self.registered.clone(), self.on_active.clone());
        let (reply, rx) = mpsc::channel();
        (self.main)(Box::new(move || {
            let old = registered.lock().unwrap().shortcut;
            let result = with_manager(|m| {
                if let Some(old) = old {
                    let _ = m.unregister(old);
                }
                m.register(new).map_err(|e| {
                    if let Some(old) = old {
                        let _ = m.register(old); // keep the app usable
                    }
                    format!("Could not use that shortcut (another app may own it): {e}")
                })
            });
            if result.is_ok() {
                registered.lock().unwrap().shortcut = Some(new);
            }
            on_active(registered.lock().unwrap().shortcut.is_some());
            let _ = reply.send(result);
        }));
        rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "The shortcut service did not respond.".to_string())?
    }

    /// Registers Esc (swallowing it system-wide) only while a dictation runs.
    pub fn set_escape(&self, enabled: bool) {
        let registered = self.registered.clone();
        (self.main)(Box::new(move || {
            let mut r = registered.lock().unwrap();
            if r.escape == enabled {
                return;
            }
            let result = with_manager(|m| {
                if enabled { m.register(escape_key()) } else { m.unregister(escape_key()) }.map_err(|e| e.to_string())
            });
            match result {
                Ok(()) => r.escape = enabled,
                Err(e) => log::warn!("Esc shortcut: {e}"),
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_stored_format() {
        assert_eq!(
            parse_shortcut("cmd+shift+f").unwrap(),
            HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyF)
        );
        assert_eq!(
            parse_shortcut("Ctrl+Alt+Space").unwrap(),
            HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space)
        );
        assert_eq!(parse_shortcut("ctrl+shift+f13").unwrap().key, Code::F13);
        assert_eq!(parse_shortcut("alt+keypad7").unwrap().key, Code::Numpad7);
        assert_eq!(parse_shortcut("cmd+grave").unwrap().key, Code::Backquote);
        assert_eq!(parse_shortcut("cmd+9").unwrap().key, Code::Digit9);
        assert_eq!(parse_shortcut("f5").unwrap(), HotKey::new(None, Code::F5));
    }

    #[test]
    fn rejects_bad_shortcuts() {
        assert!(parse_shortcut("definitely+not+a+key").is_err());
        assert!(parse_shortcut("cmd+shift").is_err(), "modifiers only");
        assert!(parse_shortcut("cmd+f+g").is_err(), "two keys");
        assert!(parse_shortcut("f+cmd").is_err(), "modifier after key");
        assert!(parse_shortcut("f25").is_err());
        assert!(parse_shortcut("").is_err());
    }

    #[test]
    fn classify_maps_ids_and_states() {
        let (a, b) = (parse_shortcut("ctrl+alt+j").unwrap().id(), escape_key().id());
        assert_eq!(classify(a, HotKeyState::Pressed, Some(a), Some(b)), Some(ShortcutEvent::Pressed));
        assert_eq!(classify(a, HotKeyState::Released, Some(a), Some(b)), Some(ShortcutEvent::Released));
        assert_eq!(classify(b, HotKeyState::Pressed, Some(a), Some(b)), Some(ShortcutEvent::Escape));
        assert_eq!(classify(b, HotKeyState::Released, Some(a), Some(b)), None, "Esc acts on press only");
        assert_eq!(classify(b, HotKeyState::Pressed, Some(a), None), None, "Esc not registered");
    }
}
