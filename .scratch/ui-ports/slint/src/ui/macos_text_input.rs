//! macOS: the system Character Viewer (Edit → Emoji & Symbols, Ctrl+Cmd+Space) with Slint's
//! winit 0.30 backend.
//!
//! The menu item itself is a plain Slint `MenuItem` in the native menu bar; [`open_picker`] is
//! its action. The remaining problem is winit 0.30's `insertText:replacementRange:`: it drops
//! text that arrives without marked (composing) text, which is how the Character Viewer
//! inserts. We wrap that method and hand such text to Slint as a key press carrying the text,
//! which `TextInput` inserts at the cursor. Fixed upstream in winit 0.31; drop this then. The Qt
//! backend does not need it (`WinitView` does not exist there, so nothing is wrapped).

use std::sync::{Mutex, OnceLock};

use objc2::{
    MainThreadMarker, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSEventType};
use objc2_foundation::{NSAttributedString, NSRange, NSString};

type InsertText = unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, NSRange);
type Sink = Box<dyn Fn(String) + Send>;

static ORIGINAL_INSERT_TEXT: OnceLock<InsertText> = OnceLock::new();
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Call once on the main thread, after the first window exists. `sink` receives text the
/// Character Viewer inserted.
pub fn install(sink: impl Fn(String) + Send + 'static) {
    *SINK.lock().unwrap() = Some(Box::new(sink));
    if ORIGINAL_INSERT_TEXT.get().is_some() {
        return;
    }
    let Some(method) = AnyClass::get(c"WinitView")
        .and_then(|class| class.instance_method(sel!(insertText:replacementRange:)))
    else {
        log::info!("no WinitView (Qt backend?); the Character Viewer needs no bridge");
        return;
    };
    let wrapper: InsertText = insert_text;
    // SAFETY: same signature as winit's method; the original is stored before AppKit can
    // call the wrapper, since both happen on the main thread.
    unsafe {
        let original = method.set_implementation(std::mem::transmute::<InsertText, Imp>(wrapper));
        let _ = ORIGINAL_INSERT_TEXT.set(std::mem::transmute::<Imp, InsertText>(original));
    }
    log::info!("Character Viewer bridge installed on WinitView");
}

/// Opens the Character Viewer, like AppKit's own Edit → Emoji & Symbols.
pub fn open_picker() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).orderFrontCharacterPalette(None);
    }
}

/// Bounces the Dock icon once (works with either backend).
pub fn request_attention() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .requestUserAttention(objc2_app_kit::NSRequestUserAttentionType::InformationalRequest);
    }
}

unsafe extern "C-unwind" fn insert_text(
    this: &AnyObject,
    cmd: Sel,
    string: &AnyObject,
    range: NSRange,
) {
    // Typing (keyDown → interpretKeyEvents) and IME commits are handled by winit itself.
    let has_marked_text: bool = unsafe { msg_send![this, hasMarkedText] };
    let from_key_down = MainThreadMarker::new()
        .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent())
        .is_some_and(|event| event.r#type() == NSEventType::KeyDown);

    if !has_marked_text && !from_key_down {
        let text = if let Some(s) = string.downcast_ref::<NSAttributedString>() {
            s.string().to_string()
        } else if let Some(s) = string.downcast_ref::<NSString>() {
            s.to_string()
        } else {
            String::new()
        };
        if !text.is_empty() && !text.starts_with(char::is_control) {
            log::info!("Character Viewer inserted {text:?}");
            if let Some(sink) = SINK.lock().unwrap().as_ref() {
                sink(text);
            }
        }
    }

    if let Some(original) = ORIGINAL_INSERT_TEXT.get() {
        unsafe { original(this, cmd, string, range) };
    }
}
