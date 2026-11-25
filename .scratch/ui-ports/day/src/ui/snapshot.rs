//! Own-window snapshots for `tools/screenshot.sh` (feature `snapshot`), for sandboxes that deny
//! `screencapture`. The app renders only its own frontmost window (AppKit
//! `cacheDisplayInRect:`), never the screen.
//!
//! The script writes `$QD_SNAPSHOT_DIR/request`: optional command lines, then the PNG path as
//! the last line. Commands drive the window into a state first: `toggle N` (Option N, from 1),
//! `note TEXT`, `next`, `back`, `review`, `goto N`, `round N` (sidebar round, from 1), `title TEXT` (the round whose title contains it),
//! `appearance light|dark`, `hover N`, `settings`, `cancel` (Escape), `submit` (⌘↩).
//!
//! Keyboard checks without activating the app (which would take the user's focus): `key NAME`
//! sends a synthetic AppKit key event (`1`…`9`, `tab`, `enter`, `esc`, `cmd-enter`,
//! `cmd-shift-enter`) the way AppKit routes it, menu key equivalents first; `type TEXT` inserts
//! text into the first responder as the Character Viewer or an input method does; `menus` and
//! `responder` log the menu bar and the first responder; `close` clicks the close button and
//! `quit` chooses Quit; `click TITLE` clicks a button.

use std::path::PathBuf;

use super::{act, desk, state};
use crate::desk::Desk;

pub(crate) fn install() {
    let Some(dir) = std::env::var_os("QD_SNAPSHOT_DIR").map(PathBuf::from) else {
        return;
    };
    day::task(async move {
        let request = dir.join("request");
        loop {
            day::sleep(100).await;
            let Ok(text) = std::fs::read_to_string(&request) else {
                continue;
            };
            let _ = std::fs::remove_file(&request);
            let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            let Some(out) = lines.pop() else { continue };
            for command in lines {
                run(command.trim());
                day::sleep(150).await;
            }
            day::sleep(600).await;
            // Day's own `window_image()` asks the window server first, which the nono sandbox
            // answers with a blank image; `QD_SNAPSHOT_MODE=server` still selects it.
            let server = std::env::var("QD_SNAPSHOT_MODE").is_ok_and(|m| m == "server");
            let png = if server {
                day::window_image().chrome().capture().map_err(|e| e.to_string())
            } else {
                offscreen_capture()
            };
            match png {
                Ok(png) => {
                    if let Err(e) = std::fs::write(out.trim(), png) {
                        log::error!("snapshot: {e}");
                    }
                }
                Err(e) => log::error!("snapshot: {e}"),
            }
        }
    });
}

fn run(command: &str) {
    let (verb, arg) = command.split_once(' ').unwrap_or((command, ""));
    let n = arg.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1));
    match (verb, n) {
        ("toggle", Some(i)) => act(|d| d.toggle_option(i)),
        ("goto", Some(i)) => act(|d| d.go_to(i)),
        ("round", Some(i)) => {
            let ticket = desk()
                .sessions()
                .into_iter()
                .flat_map(|s| s.rounds)
                .nth(i)
                .map(|r| r.ticket);
            if let Some(ticket) = ticket {
                act(|d| d.select(&ticket));
            }
        }
        ("title", _) => {
            let ticket = desk()
                .sessions()
                .into_iter()
                .flat_map(|s| s.rounds)
                .find(|r| r.title.as_deref().is_some_and(|t| t.contains(arg)))
                .map(|r| r.ticket);
            if let Some(ticket) = ticket {
                act(|d| d.select(&ticket));
            }
        }
        ("note", _) => act(|d| d.set_note(arg)),
        ("next", _) => act(Desk::next),
        ("back", _) => act(Desk::back),
        ("review", _) => act(Desk::review),
        ("hover", Some(i)) => state().hovered.set(Some(i)),
        ("settings", _) => {
            day::open_preferences();
        }
        ("cancel", _) => super::page::escape(),
        ("key", _) => key(arg),
        ("type", _) => type_text(arg),
        ("menus", _) => dump_menus(),
        ("close", _) => close_window(),
        ("click", _) => click(arg),
        ("quit", _) => super::confirm_quit(),
        ("responder", _) => log_responder(),
        ("submit", _) => super::page::next_or_submit(),
        ("appearance", _) => day::set_appearance(match arg {
            "light" => Some(false),
            "dark" => Some(true),
            _ => None,
        }),
        _ => log::warn!("snapshot: unknown command {command:?}"),
    }
}

/// `cacheDisplayInRect:` of the main window's frame view: what AppKit draws, without asking the
/// window server (which the sandbox may refuse). Materials the window server composites (the
/// sidebar's glass) come out flat.
#[cfg(target_os = "macos")]
fn offscreen_capture() -> Result<Vec<u8>, String> {
    use objc2_app_kit::{NSApplication, NSBitmapImageFileType};
    use objc2_foundation::NSDictionary;
    let mtm = objc2::MainThreadMarker::new().ok_or("not on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    // The frontmost visible window: the Settings window when it is open.
    let window = app
        .orderedWindows()
        .iter()
        .find(|w| w.isVisible())
        .or_else(|| app.mainWindow())
        .ok_or("no window")?;
    // An open alert is a sheet: a window of its own on top of this one.
    let window = window.attachedSheet().unwrap_or(window);
    let content = window.contentView().ok_or("no content view")?;
    // The frame view (the content view's superview) includes the title bar and toolbar.
    let view = unsafe { content.superview() }.unwrap_or(content);
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("no bitmap rep")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("png encode failed")?;
    Ok(data.to_vec())
}

#[cfg(not(target_os = "macos"))]
fn offscreen_capture() -> Result<Vec<u8>, String> {
    day::window_image().capture().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn main_window() -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    let mtm = objc2::MainThreadMarker::new()?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.windows().iter().find(|w| w.isVisible() && w.title().to_string() != "Settings")
}

/// A synthetic key press, routed as AppKit routes one: Escape and ⌘ chords to the menu bar
/// first (Day's Escape monitor and `performKeyEquivalent:`), everything else to the window.
#[cfg(target_os = "macos")]
fn key(name: &str) {
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::{NSPoint, NSString};
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let Some(window) = main_window() else { return };
    let (chars, code, flags): (&str, u16, NSEventModifierFlags) = match name {
        "tab" => ("\t", 48, NSEventModifierFlags::empty()),
        "enter" => ("\r", 36, NSEventModifierFlags::empty()),
        "esc" => ("\u{1b}", 53, NSEventModifierFlags::empty()),
        "cmd-enter" => ("\r", 36, NSEventModifierFlags::Command),
        "cmd-shift-enter" => ("\r", 36, NSEventModifierFlags::Command | NSEventModifierFlags::Shift),
        d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => {
            const CODES: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
            (d, CODES[d.parse::<usize>().unwrap_or(0)], NSEventModifierFlags::empty())
        }
        _ => {
            log::warn!("snapshot: unknown key {name:?}");
            return;
        }
    };
    let Some(event) = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        flags,
        0.0,
        window.windowNumber(),
        None,
        &NSString::from_str(chars),
        &NSString::from_str(chars),
        false,
        code,
    ) else {
        return;
    };
    let to_menu = name == "esc" || flags.contains(NSEventModifierFlags::Command);
    let handled = to_menu
        && NSApplication::sharedApplication(mtm)
            .mainMenu()
            .is_some_and(|menu| menu.performKeyEquivalent(&event));
    if !handled {
        window.sendEvent(&event);
    }
    log::info!("snapshot: key {name} (menu: {handled})");
}

/// Inserts `text` into the first responder, as the Character Viewer or an input method does.
#[cfg(target_os = "macos")]
fn type_text(text: &str) {
    use objc2_foundation::NSString;
    let Some(window) = main_window() else { return };
    let Some(responder) = window.firstResponder() else { return };
    let text = NSString::from_str(text);
    let _: () = unsafe { objc2::msg_send![&*responder, insertText: &*text] };
}

#[cfg(target_os = "macos")]
fn log_responder() {
    let Some(window) = main_window() else { return };
    let name = window
        .firstResponder()
        .map(|r| r.class().name().to_str().unwrap_or("?").to_owned());
    log::info!("snapshot: first responder {name:?}");
}

#[cfg(target_os = "macos")]
fn dump_menus() {
    use objc2_app_kit::{NSApplication, NSMenu};
    fn walk(menu: &NSMenu, depth: usize, out: &mut String) {
        for item in menu.itemArray().iter() {
            let key = item.keyEquivalent().to_string();
            out.push_str(&format!(
                "{}{} [{:#x} {:?}]\n",
                "  ".repeat(depth),
                item.title(),
                item.keyEquivalentModifierMask().0,
                key
            ));
            if let Some(sub) = item.submenu() {
                walk(&sub, depth + 1, out);
            }
        }
    }
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let mut out = String::new();
    if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
        walk(&menu, 0, &mut out);
    }
    log::info!("snapshot: menu bar\n{out}");
}

/// The window's close button, as a click on it goes: through `windowShouldClose:`.
#[cfg(target_os = "macos")]
fn close_window() {
    if let Some(window) = main_window() {
        window.performClose(None);
    }
}

/// Clicks the first button titled `title` in any window, as the mouse would.
#[cfg(target_os = "macos")]
fn click(title: &str) {
    use objc2_app_kit::{NSButton, NSView};
    fn find(view: &NSView, title: &str) -> Option<objc2::rc::Retained<NSButton>> {
        if let Some(button) = view.downcast_ref::<NSButton>()
            && button.title().to_string() == title
        {
            return Some(objc2::Message::retain(button));
        }
        view.subviews().iter().find_map(|sub| find(&sub, title))
    }
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let windows = objc2_app_kit::NSApplication::sharedApplication(mtm).windows();
    let found = windows
        .iter()
        .filter_map(|w| w.contentView())
        .find_map(|content| find(&content, title));
    match found {
        // SAFETY: a live button on the main thread, clicked with no sender.
        Some(button) => unsafe { button.performClick(None) },
        None => log::warn!("snapshot: no button {title:?}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn click(_: &str) {}
#[cfg(not(target_os = "macos"))]
fn close_window() {}
#[cfg(not(target_os = "macos"))]
fn key(_: &str) {}
#[cfg(not(target_os = "macos"))]
fn type_text(_: &str) {}
#[cfg(not(target_os = "macos"))]
fn log_responder() {}
#[cfg(not(target_os = "macos"))]
fn dump_menus() {}
