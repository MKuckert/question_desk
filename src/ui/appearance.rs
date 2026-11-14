//! The look: an egui-elements theme (Tokyo Night by default) with Inter and the app's text
//! sizes, plus the Appearance window with the live theme editor and the optional ambient light.

use eframe::egui::{
    self, FontData, FontFamily, TextStyle,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};
use egui_elements::{
    Button, Light, LightSettings, ParticleLayer, Theme, ThemeKind, Window, editor::ThemeEditor,
    theme::ThemeColors, utils,
};

pub struct Appearance {
    pub theme: Theme,
    pub editor: ThemeEditor,
    pub window_open: bool,
    light: LightSettings,
}

impl Appearance {
    /// Installs Inter and the theme stored under `kind` (a [`ThemeKind::to_str`] name).
    pub fn new(ctx: &egui::Context, kind: Option<&str>) -> Self {
        install_inter(ctx);
        let kind = ThemeKind::to_vec()
            .into_iter()
            .find(|k| Some(k.to_str()) == kind)
            .unwrap_or(ThemeKind::TokyoNight);
        let mut appearance = Self {
            theme: Theme::new(kind),
            editor: ThemeEditor::new(),
            window_open: false,
            light: LightSettings {
                active: false,
                density: 24,
                intensity: 0.35,
                radius_min: 6.0,
                radius_max: 28.0,
                layer: ParticleLayer::Background,
                ..LightSettings::default()
            },
        };
        appearance.install(ctx);
        appearance
    }

    pub fn colors(&self) -> ThemeColors {
        self.theme.colors
    }

    /// `theme.install` replaces the whole `egui::Style`, so the text sizes go on top every time.
    fn install(&mut self, ctx: &egui::Context) {
        self.theme.install(ctx);
        set_text_sizes(ctx);
    }

    fn set_kind(&mut self, ctx: &egui::Context, kind: ThemeKind) {
        self.theme = Theme::new(kind);
        self.install(ctx);
    }

    /// Flips between Tokyo Night and Tokyo Night Light.
    pub fn toggle_dark(&mut self, ctx: &egui::Context) {
        let kind = if self.theme.dark {
            ThemeKind::TokyoNightLight
        } else {
            ThemeKind::TokyoNight
        };
        self.set_kind(ctx, kind);
    }

    /// The Appearance window, the theme editor and the ambient light, if open or on.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut open = self.window_open;
        Window::new("Appearance")
            .open(&mut open)
            .frame(self.theme.window_frame)
            .title_frame(self.theme.window_frame.stroke(egui::Stroke::NONE))
            .min_width(280.0)
            .default_pos(ctx.content_rect().right_top() + egui::vec2(-320.0, 56.0))
            .show(&ctx, |ui| self.window_body(ui));
        self.window_open = open;

        if self.editor.open {
            self.editor.bg_color = self.theme.colors.bg;
            if let Some(theme) = self.editor.show(&mut self.theme, ui) {
                self.theme = theme;
            }
            // The editor mutates the theme in place; it only takes effect once installed.
            self.install(&ctx);
        }

        self.light.show_window(&ctx);
        if self.light.active {
            Light::from_settings("ambient_light", &self.light).show(&ctx);
        }
    }

    fn window_body(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.label(egui::RichText::new("Mode").strong());
        ui.horizontal(|ui| {
            for (kind, label) in [
                (ThemeKind::TokyoNight, "Dark"),
                (ThemeKind::TokyoNightLight, "Light"),
            ] {
                let selected = self.theme.kind == kind;
                if ui.add(Button::selectable(selected, label)).clicked() && !selected {
                    self.set_kind(&ctx, kind);
                }
            }
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Theme").strong());
        if let Some(theme) = utils::theme_switcher(&self.theme, ui) {
            self.theme = theme;
            set_text_sizes(&ctx);
        }
        if ui.add(Button::new("Open theme editor…")).clicked() {
            self.editor.open = true;
        }
        ui.separator();
        ui.checkbox(&mut self.light.active, "Ambient light");
        if ui
            .add_enabled(self.light.active, Button::new("Light settings…"))
            .clicked()
        {
            self.light.color = self.theme.colors.accent;
            self.light.open = true;
        }
    }
}

/// Inter before egui's default fonts; the fallbacks still cover emoji and symbols.
fn install_inter(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        "Inter",
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Highest,
        }],
    ));
}

/// A larger base font than egui's default; long text should be comfortable to read.
fn set_text_sizes(ctx: &egui::Context) {
    ctx.global_style_mut(|style| {
        for (text_style, size) in [
            (TextStyle::Small, 12.0),
            (TextStyle::Body, 16.0),
            (TextStyle::Button, 16.0),
            (TextStyle::Monospace, 14.0),
            (TextStyle::Heading, 24.0),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
}
