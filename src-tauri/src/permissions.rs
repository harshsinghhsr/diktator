//! macOS privacy permissions (Accessibility; the microphone prompt comes from
//! the first recording). Windows needs none.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub platform: &'static str,
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Shows the system "allow Diktator to control this computer" prompt and
    /// adds Diktator to the Accessibility list. Returns the current state.
    pub fn request() -> bool {
        unsafe {
            // CFStringRef is toll-free bridged to NSString.
            let key: &NSString = &*(kAXTrustedCheckOptionPrompt as *const NSString);
            let yes = NSNumber::new_bool(true);
            let options = NSDictionary::from_slices(&[key], &[&*yes]);
            AXIsProcessTrustedWithOptions(objc2::rc::Retained::as_ptr(&options) as *const c_void)
        }
    }
}

pub fn status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    return PermissionStatus { accessibility: mac::trusted(), platform: "macos" };
    #[cfg(not(target_os = "macos"))]
    PermissionStatus { accessibility: true, platform: "windows" }
}

/// Opens the relevant OS settings page. `pane` is "accessibility" or "microphone".
pub fn open_pane(pane: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if pane == "accessibility" {
            mac::request();
        }
        let anchor = if pane == "microphone" { "Privacy_Microphone" } else { "Privacy_Accessibility" };
        std::process::Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        if pane != "microphone" {
            return Ok(());
        }
        std::process::Command::new("explorer")
            .arg("ms-settings:privacy-microphone")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = pane;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_reports_platform() {
        let s = super::status();
        assert!(s.platform == "macos" || s.platform == "windows");
    }
}
