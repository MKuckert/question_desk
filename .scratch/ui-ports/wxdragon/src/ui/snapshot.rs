//! Development aid behind the `snapshot` feature (macOS): renders the app's own window to a
//! PNG on request, for `tools/screenshot.sh` in environments where window capture is not
//! permitted. Only this window is rendered, never the screen.
//!
//! With `QD_SNAPSHOT_DIR` set, the app polls `$QD_SNAPSHOT_DIR/request`; the file holds the
//! output path. AppKit draws the window frame and native controls (`cacheDisplayInRect:`);
//! web views draw out of process, so each one is captured with WKWebView's own snapshot
//! and composited on top.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::NSRect;
use wxdragon::prelude::*;

pub fn install(frame: &Frame) {
    let Some(dir) = std::env::var_os("QD_SNAPSHOT_DIR").map(PathBuf::from) else {
        return;
    };
    let frame = *frame;
    let timer = Timer::new(&frame);
    timer.on_tick(move |_| {
        let request = dir.join("request");
        let Ok(out) = std::fs::read_to_string(&request) else { return };
        let _ = std::fs::remove_file(&request);
        let out = PathBuf::from(out.trim());
        unsafe {
            log_menus();
            take(frame.get_handle(), out)
        };
    });
    timer.start(300, false);
    std::mem::forget(timer);
}

struct Pending {
    rep: Retained<AnyObject>,
    images: Vec<(Retained<AnyObject>, NSRect)>,
    remaining: usize,
    out: PathBuf,
}

unsafe fn take(handle: *mut std::ffi::c_void, out: PathBuf) {
    unsafe {
        let object = handle as *mut AnyObject;
        if object.is_null() {
            return;
        }
        let ns_window = AnyClass::get(c"NSWindow").expect("AppKit is loaded");
        let is_window: Bool = msg_send![object, isKindOfClass: ns_window];
        let mut window: *mut AnyObject = if is_window.as_bool() {
            object
        } else {
            msg_send![object, window]
        };
        // A modal dialog of ours (Settings, a confirmation) is rendered instead.
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let modal: *mut AnyObject = msg_send![app, modalWindow];
        if !modal.is_null() {
            window = modal;
        }
        let content: *mut AnyObject = msg_send![window, contentView];
        // The theme frame above the content view also draws the title bar.
        let root: *mut AnyObject = msg_send![content, superview];
        let root = if root.is_null() { content } else { root };
        let bounds: NSRect = msg_send![root, bounds];
        let rep: *mut AnyObject = msg_send![root, bitmapImageRepForCachingDisplayInRect: bounds];
        let Some(rep) = Retained::retain(rep) else { return };
        let _: () = msg_send![root, cacheDisplayInRect: bounds, toBitmapImageRep: &*rep];

        let mut webviews = Vec::new();
        collect_webviews(root, &mut webviews);
        let pending = Rc::new(RefCell::new(Pending {
            rep,
            images: Vec::new(),
            remaining: webviews.len(),
            out,
        }));
        if webviews.is_empty() {
            finish(&pending.borrow());
            return;
        }
        for view in webviews {
            let view_bounds: NSRect = msg_send![view, bounds];
            let rect: NSRect = msg_send![view, convertRect: view_bounds, toView: root];
            let pending = pending.clone();
            let block = RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
                let mut p = pending.borrow_mut();
                if let Some(image) = Retained::retain(image) {
                    p.images.push((image, rect));
                }
                p.remaining -= 1;
                if p.remaining == 0 {
                    finish(&p);
                }
            });
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![view, takeSnapshotWithConfiguration: none, completionHandler: &*block];
        }
    }
}

unsafe fn collect_webviews(view: *mut AnyObject, out: &mut Vec<*mut AnyObject>) {
    unsafe {
        if let Some(wk) = AnyClass::get(c"WKWebView") {
            let is_web: Bool = msg_send![view, isKindOfClass: wk];
            if is_web.as_bool() {
                let hidden: Bool = msg_send![view, isHiddenOrHasHiddenAncestor];
                if !hidden.as_bool() {
                    out.push(view);
                }
                return;
            }
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            collect_webviews(child, out);
        }
    }
}

fn finish(p: &Pending) {
    unsafe {
        let rep = &*p.rep;
        if !p.images.is_empty() {
            let context: *mut AnyObject =
                msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: rep];
            let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
            let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: context];
            for (image, rect) in &p.images {
                let _: () = msg_send![&**image, drawInRect: *rect];
            }
            let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
        }
        let properties: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        // NSBitmapImageFileTypePNG
        let data: *mut AnyObject =
            msg_send![rep, representationUsingType: 4usize, properties: properties];
        if data.is_null() {
            log::warn!("snapshot: PNG encoding failed");
            return;
        }
        let len: usize = msg_send![data, length];
        let bytes: *const u8 = msg_send![data, bytes];
        let png = std::slice::from_raw_parts(bytes, len);
        let tmp = p.out.with_extension("png.part");
        if std::fs::write(&tmp, png).and_then(|_| std::fs::rename(&tmp, &p.out)).is_err() {
            log::warn!("snapshot: could not write {}", p.out.display());
        } else {
            log::info!("snapshot written to {}", p.out.display());
        }
    }
}

/// Delivers a synthetic key-down to the window, as AppKit would for a real key press, so
/// the screenshot script can exercise the keyboard path (`QD_AUTOPILOT` `key:` steps).
/// Names: `1`–`9`, `return`, `cmd-return`, `cmd-shift-return`, `tab`, `escape`,
/// `text:…` for typed characters; anything else only logs the first responder.
pub fn send_key(frame: &Frame, name: &str) {
    const SHIFT: usize = 1 << 17;
    const COMMAND: usize = 1 << 20;
    let (chars, code, flags): (String, u16, usize) = match name {
        "return" => ("\r".into(), 36, 0),
        "cmd-return" => ("\r".into(), 36, COMMAND),
        "cmd-shift-return" => ("\r".into(), 36, COMMAND | SHIFT),
        "tab" => ("\t".into(), 48, 0),
        "escape" => ("\u{1b}".into(), 53, 0),
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
            let codes = [29u16, 18, 19, 20, 21, 23, 22, 26, 28, 25];
            (d.into(), codes[(d.as_bytes()[0] - b'0') as usize], 0)
        }
        other => match other.strip_prefix("text:") {
            Some(text) => {
                for c in text.chars() {
                    post(frame, &c.to_string(), 0, 0);
                }
                return;
            }
            None => {
                // Only logs the first responder.
                post(frame, "", 0, 1 << 30);
                return;
            }
        },
    };
    post(frame, &chars, code, flags);
}

fn post(frame: &Frame, chars: &str, code: u16, flags: usize) {
    unsafe {
        let object = frame.get_handle() as *mut AnyObject;
        if object.is_null() {
            return;
        }
        let ns_window = AnyClass::get(c"NSWindow").expect("AppKit is loaded");
        let is_window: Bool = msg_send![object, isKindOfClass: ns_window];
        let window: *mut AnyObject = if is_window.as_bool() {
            object
        } else {
            msg_send![object, window]
        };
        // Key window within the app (the app itself is not activated, so the user's
        // frontmost app keeps the keyboard); wx looks for focus in the key window.
        let is_key: Bool = msg_send![window, isKeyWindow];
        if !is_key.as_bool() {
            let _: () = msg_send![window, makeKeyWindow];
        }
        let number: isize = msg_send![window, windowNumber];
        let responder: *mut AnyObject = msg_send![window, firstResponder];
        let is_key: Bool = msg_send![window, isKeyWindow];
        log::debug!(
            "key window {}, first responder {}",
            is_key.as_bool(),
            if responder.is_null() { "nil".to_owned() } else { (*responder).class().name().to_string_lossy().into_owned() }
        );
        let c_chars = std::ffi::CString::new(chars).unwrap_or_default();
        let string: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c_chars.as_ptr()];
        let location = objc2_foundation::NSPoint::new(0.0, 0.0);
        let none: *mut AnyObject = std::ptr::null_mut();
        // NSEventTypeKeyDown = 10
        let event: *mut AnyObject = msg_send![class!(NSEvent),
            keyEventWithType: 10usize,
            location: location,
            modifierFlags: flags,
            timestamp: 0.0f64,
            windowNumber: number,
            context: none,
            characters: string,
            charactersIgnoringModifiers: string,
            isARepeat: Bool::NO,
            keyCode: code];
        if event.is_null() || flags & (1 << 30) != 0 {
            return;
        }
        // Command keys are menu key equivalents; the rest go to the first responder.
        if flags & (1 << 20) != 0 {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let menu: *mut AnyObject = msg_send![app, mainMenu];
            let handled: Bool = msg_send![menu, performKeyEquivalent: event];
            if handled.as_bool() {
                return;
            }
        }
        let _: () = msg_send![window, sendEvent: event];
    }
}


/// Logs the menu bar as AppKit has it (titles, key equivalents), to check what wx and
/// AppKit put there: the app menu, Edit's system items, the Window menu.
unsafe fn log_menus() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let main: *mut AnyObject = msg_send![app, mainMenu];
        if main.is_null() {
            return;
        }
        let count: isize = msg_send![main, numberOfItems];
        for i in 0..count {
            let item: *mut AnyObject = msg_send![main, itemAtIndex: i];
            let submenu: *mut AnyObject = msg_send![item, submenu];
            if submenu.is_null() {
                continue;
            }
            let title: *mut AnyObject = msg_send![submenu, title];
            let mut entries = Vec::new();
            let n: isize = msg_send![submenu, numberOfItems];
            for j in 0..n {
                let entry: *mut AnyObject = msg_send![submenu, itemAtIndex: j];
                let separator: Bool = msg_send![entry, isSeparatorItem];
                if separator.as_bool() {
                    entries.push("—".to_owned());
                    continue;
                }
                let hidden: Bool = msg_send![entry, isHidden];
                if hidden.as_bool() {
                    continue;
                }
                let t: *mut AnyObject = msg_send![entry, title];
                let k: *mut AnyObject = msg_send![entry, keyEquivalent];
                entries.push(format!("{} [{}]", ns_string(t), ns_string(k).escape_debug()));
            }
            log::info!("menu {}: {}", ns_string(title), entries.join(" | "));
        }
    }
}

unsafe fn ns_string(s: *mut AnyObject) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
        let utf8: *const std::ffi::c_char = msg_send![s, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}
