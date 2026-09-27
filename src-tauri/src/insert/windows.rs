//! Windows clipboard paste. Clipboard ownership needs a window with a message
//! loop, so each paste gets a worker thread owning a hidden message-only
//! window. It offers the text with delayed rendering: Windows sends
//! `WM_RENDERFORMAT` when an app reads it. The worker then restores the user's
//! clipboard (every memory-backed format) unless another app took the
//! clipboard meanwhile, which Windows reports with `WM_DESTROYCLIPBOARD`.

use super::keys::press_paste;
use super::{restore_due, PasteError, CHECK_EVERY};
use crate::settings::PasteChord;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_DESTROY, WM_DESTROYCLIPBOARD, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_TIMER, WNDCLASSW,
};

const CF_UNICODETEXT: u32 = 13;
/// Asks the worker to restore right away (a new paste, or the keystroke failed).
const WM_FINISH_NOW: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;
const CLASS_NAME: PCWSTR = w!("DiktatorClipboardOwner");

/// Formats whose clipboard handle is not a block of memory (GDI objects,
/// owner-display and private handles); they can't be copied byte for byte.
/// Images survive anyway through CF_DIB, from which Windows rebuilds CF_BITMAP.
fn is_memory_format(format: u32) -> bool {
    const HANDLE_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
    !HANDLE_FORMATS.contains(&format) && !(0x200..=0x3FF).contains(&format)
}

/// State of the paste owned by this worker thread; read by the window procedure.
struct Transaction {
    text: Vec<u16>,
    saved: Vec<(u32, Vec<u8>)>,
    published_at: Instant,
    first_read: Option<Instant>,
    /// Another app replaced our clipboard content: leave theirs alone.
    replaced: bool,
    /// We are emptying the clipboard ourselves; that is not "replaced".
    restoring: bool,
}

thread_local! {
    static TX: RefCell<Option<Transaction>> = const { RefCell::new(None) };
}

/// The worker of the most recent paste, if it is still waiting.
struct Worker {
    hwnd: isize, // HWND is a raw pointer, so not Send; it is only used with PostMessageW.
    thread: JoinHandle<()>,
}

static ACTIVE: Mutex<Option<Worker>> = Mutex::new(None);

fn hwnd_from(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// OpenClipboard fails while another app briefly holds the clipboard; retry.
fn open_clipboard(owner: HWND) -> Result<(), String> {
    for _ in 0..10 {
        // SAFETY: plain Win32 call with our own window handle.
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err("the clipboard is in use by another app".into())
}

/// Copies `bytes` into a movable global memory block for SetClipboardData.
fn global_copy(bytes: &[u8]) -> Option<HANDLE> {
    // SAFETY: the block is allocated with at least `bytes.len()` bytes, locked
    // while written, and ownership passes to the clipboard on success.
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
        let dst = GlobalLock(mem);
        if dst.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(mem);
        Some(HANDLE(mem.0))
    }
}

/// Reads every memory-backed format. The clipboard must be open.
fn read_formats() -> Vec<(u32, Vec<u8>)> {
    let mut saved = Vec::new();
    // SAFETY: the clipboard is open; each handle is locked only while copied.
    unsafe {
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            if is_memory_format(format) {
                if let Ok(handle) = GetClipboardData(format) {
                    let mem = HGLOBAL(handle.0);
                    let src = GlobalLock(mem);
                    if !src.is_null() {
                        saved.push((format, std::slice::from_raw_parts(src.cast::<u8>(), GlobalSize(mem)).to_vec()));
                        let _ = GlobalUnlock(mem);
                    }
                }
            }
            format = EnumClipboardFormats(format);
        }
    }
    saved
}

/// Marks our entry so Windows clipboard history, cloud sync and clipboard
/// monitors skip it. The clipboard must be open.
fn mark_private() {
    let zero = 0u32.to_ne_bytes();
    for name in [
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        w!("CanIncludeInClipboardHistory"),
        w!("CanUploadToCloudClipboard"),
    ] {
        // SAFETY: registering a format name and handing a fresh memory block to the clipboard.
        unsafe {
            let format = RegisterClipboardFormatW(name);
            if let (true, Some(mem)) = (format != 0, global_copy(&zero)) {
                let _ = SetClipboardData(format, Some(mem));
            }
        }
    }
}

/// Puts the saved formats back if the clipboard is still ours.
fn restore(hwnd: HWND, tx: &mut Transaction) {
    // SAFETY: GetClipboardOwner has no preconditions.
    let still_ours = !tx.replaced && unsafe { GetClipboardOwner() }.is_ok_and(|owner| owner == hwnd);
    if !still_ours || open_clipboard(hwnd).is_err() {
        return;
    }
    tx.restoring = true;
    // SAFETY: the clipboard is open with our window as owner, so SetClipboardData succeeds.
    unsafe {
        let _ = EmptyClipboard();
        for (format, bytes) in &tx.saved {
            if let Some(mem) = global_copy(bytes) {
                let _ = SetClipboardData(*format, Some(mem));
            }
        }
        let _ = CloseClipboard();
    }
}

fn end(hwnd: HWND) {
    TX.with(|slot| {
        if let Some(tx) = slot.borrow_mut().as_mut() {
            restore(hwnd, tx);
        }
    });
    // SAFETY: our own timer and window; WM_DESTROY then posts WM_QUIT.
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_ID);
        let _ = DestroyWindow(hwnd);
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_RENDERFORMAT if wparam.0 as u32 == CF_UNICODETEXT => {
            // An app is reading our text; the clipboard is already open for it.
            TX.with(|slot| {
                if let Some(tx) = slot.borrow_mut().as_mut() {
                    let bytes: Vec<u8> = tx.text.iter().flat_map(|u| u.to_ne_bytes()).collect();
                    if let Some(mem) = global_copy(&bytes) {
                        // SAFETY: rendering a delayed format during WM_RENDERFORMAT.
                        let _ = unsafe { SetClipboardData(CF_UNICODETEXT, Some(mem)) };
                    }
                    tx.first_read.get_or_insert_with(Instant::now);
                }
            });
            LRESULT(0)
        }
        WM_RENDERALLFORMATS => {
            // Our window is going away while still owning unrendered text.
            if open_clipboard(hwnd).is_ok() {
                // SAFETY: the clipboard is open with our window.
                unsafe {
                    if GetClipboardOwner().is_ok_and(|owner| owner == hwnd) {
                        let _ = window_proc(hwnd, WM_RENDERFORMAT, WPARAM(CF_UNICODETEXT as usize), LPARAM(0));
                    }
                    let _ = CloseClipboard();
                }
            }
            LRESULT(0)
        }
        WM_DESTROYCLIPBOARD => {
            TX.with(|slot| {
                if let Some(tx) = slot.borrow_mut().as_mut() {
                    tx.replaced |= !tx.restoring;
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            let (replaced, due) = TX.with(|slot| {
                slot.borrow().as_ref().map_or((true, true), |tx| {
                    (tx.replaced, restore_due(tx.published_at, tx.first_read, Instant::now()))
                })
            });
            if replaced || due {
                end(hwnd);
            }
            LRESULT(0)
        }
        WM_FINISH_NOW => {
            end(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this worker thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Worker thread: take the clipboard, report back, then pump messages until
/// the transaction ends.
fn work(text: Vec<u16>, ready: mpsc::Sender<Result<isize, String>>) {
    // SAFETY: standard window-class registration and message-only window
    // creation on this thread; the window lives until `end` destroys it.
    let hwnd = unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            let _ = ready.send(Err("no module handle".into()));
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&class); // fails harmlessly if already registered
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            None,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                let _ = ready.send(Err(format!("could not create the clipboard window: {e}")));
                return;
            }
        }
    };
    let published = (|| {
        open_clipboard(hwnd)?;
        let saved = read_formats();
        // SAFETY: the clipboard is open with our window; EmptyClipboard makes it the owner.
        unsafe {
            let offered = EmptyClipboard().is_ok() && SetClipboardData(CF_UNICODETEXT, None).is_ok();
            mark_private();
            let _ = CloseClipboard();
            if !offered {
                return Err("could not put text on the clipboard".to_string());
            }
        }
        Ok(saved)
    })();
    let saved = match published {
        Ok(saved) => saved,
        Err(e) => {
            // SAFETY: our own window.
            let _ = unsafe { DestroyWindow(hwnd) };
            let _ = ready.send(Err(e));
            return;
        }
    };
    TX.with(|slot| {
        *slot.borrow_mut() = Some(Transaction {
            text,
            saved,
            published_at: Instant::now(),
            first_read: None,
            replaced: false,
            restoring: false,
        });
    });
    // SAFETY: a timer on our own window.
    unsafe { SetTimer(Some(hwnd), TIMER_ID, CHECK_EVERY.as_millis() as u32, None) };
    let _ = ready.send(Ok(hwnd.0 as isize));
    let mut msg = MSG::default();
    // SAFETY: standard message loop for this thread's window.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Ends the previous paste (restoring its clipboard) before a new one starts,
/// so the new snapshot is the user's clipboard and not our old text.
fn finish_previous() {
    let previous = ACTIVE.lock().unwrap().take();
    if let Some(w) = previous {
        // SAFETY: posting to a window that may already be gone is harmless.
        let _ = unsafe { PostMessageW(Some(hwnd_from(w.hwnd)), WM_FINISH_NOW, WPARAM(0), LPARAM(0)) };
        let _ = w.thread.join();
    }
}

pub(super) fn paste(text: &str, chord: PasteChord, input: &mut enigo::Enigo) -> Result<(), PasteError> {
    finish_previous();
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let (ready_tx, ready) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("clipboard-paste".into())
        .spawn(move || work(wide, ready_tx))
        .map_err(|e| PasteError::Clipboard(e.to_string()))?;
    let hwnd = match ready.recv() {
        Ok(Ok(hwnd)) => hwnd,
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(PasteError::Clipboard(e));
        }
        Err(_) => return Err(PasteError::Clipboard("the clipboard worker stopped".into())),
    };
    *ACTIVE.lock().unwrap() = Some(Worker { hwnd, thread });
    press_paste(input, chord).map_err(|e| {
        finish_previous(); // nothing was pasted: give the clipboard back now
        PasteError::Keystroke(e)
    })
}

#[cfg(test)]
mod tests {
    use super::is_memory_format;

    #[test]
    fn copies_memory_formats_and_skips_handles() {
        assert!(is_memory_format(13), "CF_UNICODETEXT");
        assert!(is_memory_format(8), "CF_DIB");
        assert!(is_memory_format(0xC123), "registered formats (HTML, RTF, ...)");
        assert!(!is_memory_format(2), "CF_BITMAP");
        assert!(!is_memory_format(14), "CF_ENHMETAFILE");
        assert!(!is_memory_format(0x300), "GDI object range");
    }
}
