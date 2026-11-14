//! The bespoke look: design tokens, fonts, both egui themes and the class-based button theme.
//!
//! Every color, radius and spacing the window uses comes from here. Light and dark share the
//! tokens and differ only in their [`Palette`].

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontFamily, FontId, Frame, Margin, Shadow,
    Stroke, Style, TextStyle, Theme,
    class::HasClasses as _,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
    theme::{DefaultStyle, StyleProvider},
    widget_style::{ButtonStyle, StyleArgs, WidgetState},
};

// ---- Tokens -------------------------------------------------------------------------------

/// Corner radius of widgets, cards and chips' container frames.
pub const RADIUS: u8 = 8;
/// Corner radius of windows and dialogs.
pub const RADIUS_LG: u8 = 12;
/// Spacing scale.
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 8.0;
pub const SPACE_M: f32 = 12.0;
pub const SPACE_L: f32 = 16.0;

/// Text sizes.
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_BODY: f32 = 15.0;
pub const TEXT_BUTTON: f32 = 14.0;
pub const TEXT_MONO: f32 = 13.0;
pub const TEXT_TITLE: f32 = 20.0;
pub const TEXT_HEADING: f32 = 24.0;

/// The colors of one theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Behind the page.
    pub bg: Color32,
    /// Top bar, footer and sidebar.
    pub panel: Color32,
    /// Cards, inputs, dialogs.
    pub surface: Color32,
    /// Preview and code blocks.
    pub sunken: Color32,
    /// A hovered surface.
    pub hover: Color32,
    /// A pressed surface.
    pub press: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub muted: Color32,
    pub accent: Color32,
    /// Text and icons on an accent fill.
    pub on_accent: Color32,
    pub success: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub on_danger: Color32,
}

/// Warm amber on warm charcoal.
pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x14, 0x13, 0x11),
    panel: Color32::from_rgb(0x1B, 0x1A, 0x17),
    surface: Color32::from_rgb(0x22, 0x20, 0x1C),
    sunken: Color32::from_rgb(0x0F, 0x0E, 0x0C),
    hover: Color32::from_rgb(0x2B, 0x29, 0x24),
    press: Color32::from_rgb(0x34, 0x31, 0x2B),
    border: Color32::from_rgb(0x34, 0x31, 0x2B),
    border_strong: Color32::from_rgb(0x4A, 0x46, 0x3E),
    text: Color32::from_rgb(0xE7, 0xE3, 0xDA),
    text_strong: Color32::from_rgb(0xFA, 0xF7, 0xF0),
    muted: Color32::from_rgb(0x9B, 0x95, 0x8A),
    accent: Color32::from_rgb(0xE9, 0xA2, 0x3B),
    on_accent: Color32::from_rgb(0x1A, 0x12, 0x06),
    success: Color32::from_rgb(0x6B, 0xCB, 0x8B),
    warn: Color32::from_rgb(0xF0, 0x79, 0x5B),
    danger: Color32::from_rgb(0xD9, 0x4A, 0x4A),
    on_danger: Color32::WHITE,
};

/// Deep teal on warm paper.
pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xFA, 0xF9, 0xF6),
    panel: Color32::from_rgb(0xF3, 0xF2, 0xEE),
    surface: Color32::WHITE,
    sunken: Color32::from_rgb(0xF0, 0xEF, 0xEA),
    hover: Color32::from_rgb(0xF1, 0xF0, 0xEB),
    press: Color32::from_rgb(0xE7, 0xE5, 0xDE),
    border: Color32::from_rgb(0xE2, 0xE0, 0xD9),
    border_strong: Color32::from_rgb(0xCB, 0xC8, 0xBF),
    text: Color32::from_rgb(0x2A, 0x29, 0x25),
    text_strong: Color32::from_rgb(0x12, 0x11, 0x0F),
    muted: Color32::from_rgb(0x6F, 0x6B, 0x62),
    accent: Color32::from_rgb(0x0F, 0x76, 0x6E),
    on_accent: Color32::WHITE,
    success: Color32::from_rgb(0x15, 0x80, 0x3D),
    warn: Color32::from_rgb(0xB4, 0x53, 0x09),
    danger: Color32::from_rgb(0xC2, 0x32, 0x2F),
    on_danger: Color32::WHITE,
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// The palette of the theme `ui` is painted in.
pub fn of(ui: &egui::Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// A hovered fill: brighter.
pub fn hovered(color: Color32) -> Color32 {
    color.gamma_multiply(1.4)
}

/// A pressed fill: darker. Scales only the color channels; `gamma_multiply` would also make it
/// translucent, which turns it lighter on a light background.
pub fn pressed(color: Color32) -> Color32 {
    let [r, g, b, a] = color.to_array();
    let f = |c: u8| (c as f32 * 0.7).round() as u8;
    Color32::from_rgba_premultiplied(f(r), f(g), f(b), a)
}

/// A translucent tint of `color` over any background, e.g. for selected cards.
pub fn tint(color: Color32, amount: f32) -> Color32 {
    color.gamma_multiply(amount)
}

/// The Inter SemiBold family, for titles and labels that need weight. egui's `strong()` only
/// changes the color.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

const SEMIBOLD: &str = "inter-semibold";

// ---- Button classes -----------------------------------------------------------------------

/// The main action of a view: accent fill.
pub const PRIMARY: &str = "primary";
/// A destructive confirmation: danger fill.
pub const DANGER: &str = "danger";
/// A quiet action: no frame until hovered.
pub const GHOST: &str = "ghost";
/// A step bar pill; combine with [`STEP_CURRENT`] or [`STEP_DONE`].
pub const STEP: &str = "step";
pub const STEP_CURRENT: &str = "step-current";
pub const STEP_DONE: &str = "step-done";
/// A full-width, left-aligned sidebar entry; selection via `Button::selected`.
pub const NAV: &str = "nav";

/// Computes the style of every button: classed buttons get the bespoke look, all others egui's
/// default look on top of the tokenized visuals.
struct ButtonTheme;

impl StyleProvider<ButtonStyle> for ButtonTheme {
    fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle {
        let mut style: ButtonStyle = StyleProvider::<ButtonStyle>::style(&mut DefaultStyle, args);
        let p = palette(args.style.visuals.dark_mode);
        let state = args.state;
        let selected = args.classes.has_class(&egui::Button::CLASS_SELECTED);

        // (fill, text, border) in the inactive state; hover and press shift the fill.
        let (fill, text, border) = if args.has_class(PRIMARY) {
            (p.accent, p.on_accent, None)
        } else if args.has_class(DANGER) {
            (p.danger, p.on_danger, None)
        } else if args.has_class(STEP_CURRENT) {
            (p.accent, p.on_accent, None)
        } else if args.has_class(STEP_DONE) {
            (
                tint(p.success, 0.22),
                p.text_strong,
                Some(tint(p.success, 0.6)),
            )
        } else if args.has_class(STEP) {
            (Color32::TRANSPARENT, p.muted, Some(p.border_strong))
        } else if args.has_class(NAV) {
            let fill = if selected {
                tint(p.accent, 0.18)
            } else {
                Color32::TRANSPARENT
            };
            (fill, p.text, None)
        } else if args.has_class(GHOST) {
            (Color32::TRANSPARENT, p.muted, None)
        } else {
            return style;
        };

        let transparent = fill == Color32::TRANSPARENT;
        let (fill, text) = match state {
            WidgetState::Hovered if transparent => (p.hover, p.text_strong),
            WidgetState::Active if transparent => (p.press, p.text_strong),
            WidgetState::Hovered => (hovered(fill), text),
            WidgetState::Active => (pressed(fill), text),
            WidgetState::Inactive | WidgetState::Noninteractive => (fill, text),
        };
        let border = border.map_or(Stroke::NONE, |c| Stroke::new(1.0, c));

        let layout = &mut style.atom_layout;
        layout.frame = Frame::new()
            .fill(fill)
            .corner_radius(RADIUS)
            .inner_margin(layout.frame.inner_margin)
            .apply_stroke_and_expansion_without_layout_shift(border, 0.0);
        layout.text_style.color = text;
        if args
            .classes
            .has_class(&egui::Button::CLASS_IMAGE_TINT_FOLLOWS_TEXT_COLOR)
        {
            layout.image_tint = text;
        }
        if args.has_class(NAV) {
            layout.align2 = Some(Align2::LEFT_CENTER);
        }
        if args.has_class(STEP) {
            layout.text_style.font_id = semibold(TEXT_BUTTON);
            layout.align2 = Some(Align2::CENTER_CENTER);
        }
        style
    }
}

// ---- Installation -------------------------------------------------------------------------

/// Installs fonts, both themes, the button theme and the SVG loader.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    install_fonts(ctx);
    ctx.style_mut_of(Theme::Dark, |style| apply(style, &DARK));
    ctx.style_mut_of(Theme::Light, |style| apply(style, &LIGHT));
    ctx.add_widget_theme::<ButtonStyle>(ButtonTheme);
    // For screenshots: `QUESTION_DESK_THEME=light|dark` overrides the saved preference.
    match std::env::var("QUESTION_DESK_THEME").as_deref() {
        Ok("light") => ctx.set_theme(egui::ThemePreference::Light),
        Ok("dark") => ctx.set_theme(egui::ThemePreference::Dark),
        _ => {}
    }
}

fn install_fonts(ctx: &egui::Context) {
    // Inter goes first in the proportional family; egui's defaults and the system font
    // provider (color emoji) stay behind it as fallbacks.
    ctx.add_font(FontInsert::new(
        "inter",
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Highest,
        }],
    ));
    ctx.add_font(FontInsert::new(
        SEMIBOLD,
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")),
        vec![InsertFontFamily {
            family: FontFamily::Name(SEMIBOLD.into()),
            priority: FontPriority::Highest,
        }],
    ));
}

fn apply(style: &mut Style, p: &Palette) {
    for (text_style, font) in [
        (TextStyle::Small, FontId::proportional(TEXT_SMALL)),
        (TextStyle::Body, FontId::proportional(TEXT_BODY)),
        (TextStyle::Button, FontId::proportional(TEXT_BUTTON)),
        (TextStyle::Monospace, FontId::monospace(TEXT_MONO)),
        (TextStyle::Heading, semibold(TEXT_HEADING)),
    ] {
        style.text_styles.insert(text_style, font);
    }

    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(SPACE_S, SPACE_S);
    spacing.button_padding = egui::vec2(SPACE_M, SPACE_XS + 2.0);
    spacing.interact_size.y = 32.0;
    spacing.window_margin = Margin::same(SPACE_L as i8);
    spacing.menu_margin = Margin::same(SPACE_S as i8);
    spacing.indent = SPACE_L;
    spacing.icon_spacing = SPACE_S;

    let v = &mut style.visuals;
    v.panel_fill = p.panel;
    v.window_fill = p.surface;
    v.faint_bg_color = p.surface;
    v.extreme_bg_color = p.sunken;
    v.code_bg_color = p.sunken;
    v.text_edit_bg_color = Some(p.surface);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = tint(p.accent, 0.25);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    let shadow_alpha = if v.dark_mode { 110 } else { 28 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha / 2),
    };

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = p.surface;
    w.inactive.weak_bg_fill = p.surface;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text_strong);
    w.active.bg_fill = p.press;
    w.active.weak_bg_fill = p.press;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(2.0, p.text_strong);
    w.open = w.hovered;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = CornerRadius::same(RADIUS);
        state.expansion = 0.0;
    }
}

// ---- Shared pieces ------------------------------------------------------------------------

/// A small rounded label, e.g. a header or "Recommended".
pub fn chip(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    Frame::new()
        .fill(bg)
        .corner_radius(RADIUS)
        .inner_margin(Margin::symmetric(SPACE_S as i8, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(semibold(TEXT_SMALL))
                    .color(fg),
            )
        });
}

/// A neutral chip for headers.
pub fn neutral_chip(ui: &mut egui::Ui, text: &str) {
    let p = of(ui);
    chip(ui, text, p.text, p.press);
}

/// An accent chip, e.g. "Recommended".
pub fn accent_chip(ui: &mut egui::Ui, text: &str) {
    let p = of(ui);
    let fg = if ui.visuals().dark_mode {
        p.accent
    } else {
        pressed(p.accent)
    };
    chip(ui, text, fg, tint(p.accent, 0.16));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every button class lays out and paints in both themes, switching between frames.
    #[test]
    fn button_classes_render_in_both_themes() {
        let ctx = egui::Context::default();
        install(&ctx);
        for preference in [
            egui::ThemePreference::Light,
            egui::ThemePreference::Dark,
            egui::ThemePreference::Light,
        ] {
            ctx.set_theme(preference);
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::global_theme_preference_buttons(ui);
                for class in [PRIMARY, DANGER, GHOST, STEP, NAV] {
                    ui.add(egui::Button::new(class).with_class(class));
                }
                ui.add(
                    egui::Button::new("3")
                        .with_class(STEP)
                        .with_class(STEP_CURRENT),
                );
                ui.add(
                    egui::Button::new("2")
                        .with_class(STEP)
                        .with_class(STEP_DONE),
                );
                ui.add(egui::Button::new("nav").with_class(NAV).selected(true));
                ui.add(egui::Button::new("plain"));
            });
        }
    }
}
