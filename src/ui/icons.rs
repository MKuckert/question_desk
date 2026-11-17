//! Lucide icons (ISC, `assets/icons/`), drawn white so egui can tint them.

use eframe::egui::{self, Color32, ImageSource, include_image};

pub const PANEL_CLOSE: ImageSource<'static> =
    include_image!("../../assets/icons/panel-left-close.svg");
pub const PANEL_OPEN: ImageSource<'static> =
    include_image!("../../assets/icons/panel-left-open.svg");
pub const BACK: ImageSource<'static> = include_image!("../../assets/icons/chevron-left.svg");
pub const NEXT: ImageSource<'static> = include_image!("../../assets/icons/chevron-right.svg");
pub const SEND: ImageSource<'static> = include_image!("../../assets/icons/send.svg");
pub const CLOSE: ImageSource<'static> = include_image!("../../assets/icons/x.svg");
pub const WARN: ImageSource<'static> = include_image!("../../assets/icons/triangle-alert.svg");
pub const CHECK: ImageSource<'static> = include_image!("../../assets/icons/check.svg");
pub const INBOX: ImageSource<'static> = include_image!("../../assets/icons/inbox.svg");
pub const LOGO: ImageSource<'static> =
    include_image!("../../assets/icons/message-circle-question.svg");
pub const SPARKLES: ImageSource<'static> = include_image!("../../assets/icons/sparkles.svg");
pub const EYE: ImageSource<'static> = include_image!("../../assets/icons/eye.svg");
pub const DONE: ImageSource<'static> = include_image!("../../assets/icons/circle-check-big.svg");
pub const SETTINGS: ImageSource<'static> = include_image!("../../assets/icons/settings.svg");

/// An icon at `size` points, tinted `color`.
pub fn icon(source: ImageSource<'static>, size: f32, color: Color32) -> egui::Image<'static> {
    egui::Image::new(source)
        .fit_to_exact_size(egui::vec2(size, size))
        .tint(color)
}

/// An icon for a button, tinted with the button's text color by the button theme.
pub fn button_icon(source: ImageSource<'static>) -> egui::Image<'static> {
    egui::Image::new(source).fit_to_exact_size(egui::vec2(16.0, 16.0))
}
