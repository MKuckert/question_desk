//! macOS: make the system Character Viewer (emoji picker) work with eframe 0.37 / winit 0.30.
//!
//! 1. winit's menu bar has no Edit menu, so Ctrl+Cmd+Space has nothing to trigger.
//!    We add Edit → "Emoji & Symbols" bound to `orderFrontCharacterPalette:`.
//! 2. winit 0.30's `insertText:replacementRange:` drops text that arrives without marked
//!    (composing) text, which is how the Character Viewer inserts. We wrap that method and
//!    queue such text; [`take_pending`] hands it to `raw_input_hook` as `egui::Event::Text`.
//!    Fixed upstream in winit 0.31 (Character Viewer → `Ime::Commit`); drop this then.

use std::sync::{Mutex, OnceLock};

use eframe::egui;
use objc2::{
    MainThreadMarker, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem};
use objc2_foundation::{NSAttributedString, NSRange, NSString, ns_string};

type InsertText = unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, NSRange);

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ORIGINAL_INSERT_TEXT: OnceLock<InsertText> = OnceLock::new();
static EGUI_CTX: OnceLock<egui::Context> = OnceLock::new();

/// Call once from the eframe app creator (main thread, after winit built its menu and view).
pub fn install(ctx: &egui::Context) {
    let mtm = MainThreadMarker::new().expect("eframe app creator runs on the main thread");
    let _ = EGUI_CTX.set(ctx.clone());
    add_emoji_menu_item(mtm);
    wrap_insert_text();
}

/// Text inserted by the Character Viewer since the last call.
pub fn take_pending() -> Vec<String> {
    std::mem::take(&mut PENDING.lock().unwrap())
}

fn add_emoji_menu_item(mtm: MainThreadMarker) {
    let Some(main_menu) = NSApplication::sharedApplication(mtm).mainMenu() else {
        log::warn!("No main menu; Ctrl+Cmd+Space will not open the emoji picker");
        return;
    };
    // SAFETY: `orderFrontCharacterPalette:` takes a sender and is handled by NSApplication.
    let emoji_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            ns_string!("Emoji & Symbols"),
            Some(sel!(orderFrontCharacterPalette:)),
            ns_string!(" "),
        )
    };
    emoji_item.setKeyEquivalentModifierMask(
        NSEventModifierFlags::Control | NSEventModifierFlags::Command,
    );
    let edit_menu = NSMenu::initWithTitle(mtm.alloc(), ns_string!("Edit"));
    edit_menu.addItem(&emoji_item);
    let edit_item = NSMenuItem::new(mtm);
    edit_item.setSubmenu(Some(&edit_menu));
    main_menu.addItem(&edit_item);
}

fn wrap_insert_text() {
    let Some(method) = AnyClass::get(c"WinitView")
        .and_then(|class| class.instance_method(sel!(insertText:replacementRange:)))
    else {
        log::warn!("WinitView insertText:replacementRange: not found; emoji picker input is lost");
        return;
    };
    let wrapper: InsertText = insert_text;
    // SAFETY: same signature as winit's method; the original is stored before AppKit can
    // call the wrapper, since both happen on the main thread.
    unsafe {
        let original = method.set_implementation(std::mem::transmute::<InsertText, Imp>(wrapper));
        let _ = ORIGINAL_INSERT_TEXT.set(std::mem::transmute::<Imp, InsertText>(original));
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
            PENDING.lock().unwrap().push(text);
            if let Some(ctx) = EGUI_CTX.get() {
                ctx.request_repaint();
            }
        }
    }

    if let Some(original) = ORIGINAL_INSERT_TEXT.get() {
        unsafe { original(this, cmd, string, range) };
    }
}
