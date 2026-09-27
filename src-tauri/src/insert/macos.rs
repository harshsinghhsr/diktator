//! macOS clipboard paste. The text is written as an `NSPasteboardItem` whose
//! string comes from a data provider, so AppKit calls us back the moment an
//! app reads it. A main-queue timer then restores the snapshot of the user's
//! pasteboard (every item, every type) if nobody else wrote to it meanwhile.
//! Everything here runs on the main thread.

use super::keys::press_paste;
use super::{restore_due, PasteError, CHECK_EVERY};
use crate::settings::PasteChord;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider, NSPasteboardType, NSPasteboardTypeString,
};
use objc2_foundation::{NSArray, NSData, NSObject, NSObjectProtocol, NSString};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Clipboard managers that follow the nspasteboard.org convention skip items
/// carrying this type, so dictations don't pile up in the user's history.
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

/// Every item on the pasteboard, as (type, bytes) pairs.
pub(crate) type Snapshot = Vec<Vec<(String, Vec<u8>)>>;

pub(crate) fn snapshot(pb: &NSPasteboard) -> Snapshot {
    let Some(items) = pb.pasteboardItems() else { return Vec::new() };
    items
        .iter()
        .map(|item| {
            item.types()
                .iter()
                .filter_map(|t| item.dataForType(&t).map(|data| (t.to_string(), data.to_vec())))
                .collect()
        })
        .collect()
}

pub(crate) fn restore(pb: &NSPasteboard, snap: &Snapshot) {
    pb.clearContents();
    if snap.is_empty() {
        return;
    }
    let items: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = snap
        .iter()
        .map(|types| {
            let item = NSPasteboardItem::new();
            for (t, bytes) in types {
                item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(t));
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    if !pb.writeObjects(&NSArray::from_retained_slice(&items)) {
        log::warn!("could not restore the previous clipboard");
    }
}

pub struct ProviderState {
    text: String,
    first_read: Arc<Mutex<Option<Instant>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "DiktatorPasteProvider"]
    #[ivars = ProviderState]
    struct PasteProvider;

    unsafe impl NSObjectProtocol for PasteProvider {}

    unsafe impl NSPasteboardItemDataProvider for PasteProvider {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide_data(
            &self,
            _pasteboard: Option<&NSPasteboard>,
            item: &NSPasteboardItem,
            data_type: &NSPasteboardType,
        ) {
            let state = self.ivars();
            item.setString_forType(&NSString::from_str(&state.text), data_type);
            state.first_read.lock().unwrap().get_or_insert_with(Instant::now);
        }
    }
);

impl PasteProvider {
    fn new(text: &str, first_read: Arc<Mutex<Option<Instant>>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProviderState { text: text.to_string(), first_read });
        // SAFETY: NSObject's designated initializer on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// A paste waiting for the target app to read the text.
struct Pending {
    id: u64,
    snapshot: Snapshot,
    /// `changeCount` right after we wrote; a different value means someone else wrote.
    change_count: isize,
    sent_at: Instant,
    first_read: Arc<Mutex<Option<Instant>>>,
    _provider: Retained<PasteProvider>,
}

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    static NEXT_ID: RefCell<u64> = const { RefCell::new(0) };
}

/// Puts the user's clipboard back, unless something else replaced ours.
fn finish(p: Pending) {
    let pb = NSPasteboard::generalPasteboard();
    if pb.changeCount() == p.change_count {
        restore(&pb, &p.snapshot);
    }
}

/// Restores a still-waiting paste right away (a new dictation is starting).
fn finish_now() {
    if let Some(p) = PENDING.with(|slot| slot.borrow_mut().take()) {
        finish(p);
    }
}

fn schedule_check(id: u64) {
    let when = DispatchTime::try_from(CHECK_EVERY).unwrap_or(DispatchTime::NOW);
    let _ = DispatchQueue::main().after(when, move || check(id));
}

fn check(id: u64) {
    let due = PENDING.with(|slot| {
        let slot = slot.borrow();
        let p = slot.as_ref().filter(|p| p.id == id)?;
        let first_read = *p.first_read.lock().unwrap();
        Some(restore_due(p.sent_at, first_read, Instant::now()))
    });
    match due {
        None => {} // already finished by a newer paste
        Some(true) => finish_now(),
        Some(false) => schedule_check(id),
    }
}

pub(super) fn paste(text: &str, chord: PasteChord, input: &mut enigo::Enigo) -> Result<(), PasteError> {
    if MainThreadMarker::new().is_none() {
        return Err(PasteError::Keystroke("text insertion must run on the main thread".into()));
    }
    finish_now();
    let pb = NSPasteboard::generalPasteboard();
    let previous = snapshot(&pb);

    let first_read = Arc::new(Mutex::new(None));
    let provider = PasteProvider::new(text, first_read.clone());
    let item = NSPasteboardItem::new();
    // SAFETY: an immutable framework string constant.
    let string_type = unsafe { NSPasteboardTypeString };
    let offered =
        item.setDataProvider_forTypes(ProtocolObject::from_ref(&*provider), &NSArray::from_slice(&[string_type]));
    item.setData_forType(&NSData::new(), &NSString::from_str(TRANSIENT_TYPE));
    pb.clearContents();
    let written = offered && pb.writeObjects(&NSArray::from_retained_slice(&[ProtocolObject::from_retained(item)]));
    if !written {
        restore(&pb, &previous);
        return Err(PasteError::Clipboard("the pasteboard refused the text".into()));
    }
    let change_count = pb.changeCount();

    let sent_at = Instant::now();
    if let Err(e) = press_paste(input, chord) {
        restore(&pb, &previous);
        return Err(PasteError::Keystroke(e));
    }
    let id = NEXT_ID.with(|n| {
        *n.borrow_mut() += 1;
        *n.borrow()
    });
    PENDING.with(|slot| {
        *slot.borrow_mut() =
            Some(Pending { id, snapshot: previous, change_count, sent_at, first_read, _provider: provider });
    });
    schedule_check(id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frees a private pasteboard (`-releaseGlobally` has no generated binding).
    fn release(pb: &NSPasteboard) {
        // SAFETY: documented NSPasteboard method taking no arguments.
        let () = unsafe { msg_send![pb, releaseGlobally] };
    }

    #[test]
    fn snapshot_and_restore_keep_every_type() {
        // A private pasteboard: never touches the user's clipboard.
        let pb = NSPasteboard::pasteboardWithUniqueName();
        let item = NSPasteboardItem::new();
        item.setString_forType(&NSString::from_str("hello"), unsafe { NSPasteboardTypeString });
        item.setData_forType(&NSData::with_bytes(&[1, 2, 3]), &NSString::from_str("com.example.custom"));
        pb.clearContents();
        assert!(pb.writeObjects(&NSArray::from_retained_slice(&[ProtocolObject::from_retained(item)])));

        let snap = snapshot(&pb);
        pb.clearContents();
        pb.setString_forType(&NSString::from_str("dictated"), unsafe { NSPasteboardTypeString });
        restore(&pb, &snap);

        assert_eq!(snapshot(&pb), snap);
        let text = pb.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
        assert_eq!(text.as_deref(), Some("hello"));
        release(&pb);
    }

    #[test]
    fn restore_of_an_empty_clipboard_leaves_it_empty() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        pb.setString_forType(&NSString::from_str("dictated"), unsafe { NSPasteboardTypeString });
        restore(&pb, &Vec::new());
        assert!(snapshot(&pb).is_empty());
        release(&pb);
    }
}
