//! Facts about the app that will receive the paste.

use crate::settings::{PasteChord, PasteOverride};

#[derive(Debug, Default)]
pub struct Target {
    /// Lower-case bundle id (macOS) or exe file name (Windows).
    pub app_id: Option<String>,
    /// Windows: the target runs elevated and we do not (UIPI blocks our input).
    pub elevated: bool,
    /// macOS: secure event input is on (a password field has focus), so synthetic keys are dropped.
    pub secure_input: bool,
}

pub fn choose_chord(app_id: Option<&str>, overrides: &[PasteOverride]) -> PasteChord {
    app_id
        .and_then(|app| overrides.iter().find(|o| o.app.eq_ignore_ascii_case(app)))
        .map(|o| o.chord)
        .unwrap_or(PasteChord::Standard)
}

#[cfg(target_os = "macos")]
pub fn current() -> Target {
    use objc2_app_kit::NSWorkspace;
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
    }
    let app_id = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier())
        .map(|id| id.to_string().to_lowercase());
    Target { app_id, elevated: false, secure_input: unsafe { IsSecureEventInputEnabled() } != 0 }
}

#[cfg(target_os = "windows")]
pub fn current() -> Target {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe fn is_elevated(process: HANDLE) -> Option<bool> {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }

    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Target::default();
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            // Access denied to query an elevated process is itself the signal.
            return Target {
                app_id: None,
                elevated: !is_elevated(GetCurrentProcess()).unwrap_or(false),
                secure_input: false,
            };
        };
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let app_id = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size)
            .ok()
            .map(|_| String::from_utf16_lossy(&buf[..size as usize]))
            .and_then(|path| path.rsplit(['\\', '/']).next().map(|s| s.to_lowercase()));
        // Denied access to query the target is itself a sign it outranks us.
        let target_elevated = is_elevated(process).unwrap_or(true);
        let _ = CloseHandle(process);
        let self_elevated = is_elevated(GetCurrentProcess()).unwrap_or(false);
        Target { app_id, elevated: target_elevated && !self_elevated, secure_input: false }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn current() -> Target {
    Target::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_match_case_insensitively_and_default_to_standard() {
        let o = vec![PasteOverride { app: "WindowsTerminal.exe".into(), chord: PasteChord::CtrlShiftV }];
        assert_eq!(choose_chord(Some("windowsterminal.exe"), &o), PasteChord::CtrlShiftV);
        assert_eq!(choose_chord(Some("notepad.exe"), &o), PasteChord::Standard);
        assert_eq!(choose_chord(None, &o), PasteChord::Standard);
    }

    #[test]
    fn current_does_not_panic() {
        let t = current();
        eprintln!("{t:?}");
    }
}
