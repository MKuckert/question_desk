//! System fonts and colours. Everything starts from the platform's GUI font and system
//! colours, so light/dark mode, the accent colour and font size follow the OS. The only
//! literal colours are the warning and success tints, which wx has no system colour for.

use std::cell::Cell;

use wxdragon::prelude::*;

thread_local! {
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

pub fn set_zoom(zoom: f32) {
    ZOOM.with(|z| z.set(zoom));
}

pub fn zoom() -> f32 {
    ZOOM.with(Cell::get)
}

/// The platform GUI font at the current zoom.
pub fn base_font() -> Font {
    let mut font = SystemSettings::get_font(SystemFont::DefaultGuiFont)
        .unwrap_or_default();
    let size = system_point_size() as f32 * zoom();
    font.set_point_size(size.round() as i32);
    font
}

/// Point size of the GUI font (13 on macOS, 9 on Windows, usually 10–11 on GTK).
pub fn system_point_size() -> i32 {
    SystemSettings::get_font(SystemFont::DefaultGuiFont)
        .map(|f| f.get_point_size())
        .filter(|&s| s > 0)
        .unwrap_or(12)
}

/// The base font's size in points, for the web views.
pub fn base_point_size() -> f32 {
    system_point_size() as f32 * zoom()
}

pub fn scaled(factor: f32, bold: bool) -> Font {
    let mut font = base_font();
    font.set_point_size((font.get_point_size() as f32 * factor).round() as i32);
    if bold {
        font.make_bold();
    }
    font
}

pub fn bold(widget: &impl WxWidget) {
    widget.set_font(&scaled(1.0, true));
}

pub fn small(widget: &impl WxWidget) {
    widget.set_font(&scaled(0.85, false));
}

pub fn title(widget: &impl WxWidget) {
    widget.set_font(&scaled(1.3, true));
}

pub fn dark() -> bool {
    is_system_dark_mode()
}

/// Secondary text: the system's grey text colour.
pub fn muted_colour() -> Colour {
    if cfg!(target_os = "macos") {
        // NSColor.secondaryLabelColor, flattened: wx maps GRAYTEXT to the much fainter
        // disabledControlTextColor on macOS.
        if dark() {
            Colour::rgb(0x98, 0x98, 0x9d)
        } else {
            Colour::rgb(0x6e, 0x6e, 0x73)
        }
    } else {
        SystemSettings::get_colour(SystemColour::GrayText)
    }
}

/// The user's accent colour: on macOS wx maps the list highlight to
/// `selectedContentBackgroundColor`, which follows the accent colour setting; elsewhere
/// the selection highlight is the accent.
pub fn accent_colour() -> Colour {
    #[cfg(target_os = "macos")]
    if let Some(colour) = macos_accent() {
        return colour;
    }
    SystemSettings::get_colour(SystemColour::Highlight)
}

/// `NSColor.controlAccentColor` in sRGB. wx's nearest system colours on macOS are
/// selection backgrounds, which turn grey while the window is inactive.
#[cfg(target_os = "macos")]
fn macos_accent() -> Option<Colour> {
    use objc2::{class, msg_send, runtime::AnyObject};
    unsafe {
        let accent: *mut AnyObject = msg_send![class!(NSColor), controlAccentColor];
        let srgb: *mut AnyObject = msg_send![class!(NSColorSpace), sRGBColorSpace];
        if accent.is_null() || srgb.is_null() {
            return None;
        }
        let colour: *mut AnyObject = msg_send![accent, colorUsingColorSpace: srgb];
        if colour.is_null() {
            return None;
        }
        let r: f64 = msg_send![colour, redComponent];
        let g: f64 = msg_send![colour, greenComponent];
        let b: f64 = msg_send![colour, blueComponent];
        let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Colour::rgb(channel(r), channel(g), channel(b)))
    }
}

/// Orange in the style of the platform's warning tint.
pub fn warning_colour() -> Colour {
    if dark() {
        Colour::rgb(0xff, 0x9f, 0x0a)
    } else {
        Colour::rgb(0xc2, 0x5e, 0x00)
    }
}

#[allow(dead_code)]
pub fn success_colour() -> Colour {
    if dark() {
        Colour::rgb(0x30, 0xd1, 0x58)
    } else {
        Colour::rgb(0x1f, 0x8a, 0x3a)
    }
}

/// Margins in device-independent pixels.
pub const SPACE_XS: i32 = 4;
pub const SPACE_S: i32 = 8;
pub const SPACE_M: i32 = 12;
pub const SPACE_L: i32 = 20;
