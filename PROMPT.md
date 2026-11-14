# Prototype C: "Bespoke Brand" — pure egui 0.37, zero design-system crates

## Context
- Existing app (Rust, egui/eframe — check its Cargo.toml for the version).
- Goal: a fully custom, branded look using only egui 0.37's own theming tools — no theme crates. This prototype proves what the built-in toolbox (incl. the new experimental StyleProvider widget-theme system) can do, and gives us full ownership of the design.
- This is a UI-layer change only. Do NOT change app logic or behavior.
- Verified research with snippets and API notes: /workspace/research/egui-beautiful-apps.md — consult it if stuck. Official reference examples (raw fetch from github.com/emilk/egui, main branch): examples/custom_style, examples/styling_engine, examples/custom_font.
- Use port 7463 in your environment for testing purposes.

## Setup
1. If the app is on egui < 0.37, upgrade to egui/eframe 0.37 first and fix breaking changes (Rounding → CornerRadius per-widget-state fields, new Id APIs, global_theme_preference_buttons). Verify the build before styling.
2. Add to Cargo.toml: egui_extras = { version = "0.37", features = ["svg"] } (icons only — no other new crates).
3. Bundle the Inter font: download OFL-Inter (Regular + one weight for headings) into assets/fonts/, install via the 0.37 API (see egui's examples/custom_font):
   use egui::epaint::text::{FontInsert, InsertFontFamily};
   ctx.add_font(FontInsert::new(
       "inter",
       egui::FontData::from_static(include_bytes!("assets/fonts/Inter-Regular.ttf")),
       vec![InsertFontFamily {
           family: egui::FontFamily::Proportional,
           priority: epaint::text::FontPriority::Highest,
       }],
   ));

## Design tokens (define once in a small theme module; use everywhere)
- One accent per theme: pick a distinctive hue (e.g. warm amber for dark, deep teal for light — or the app's brand color if it has one).
- Derived states: hover = accent.gamma_multiply(1.4), active = accent.gamma_multiply(0.7).
- Corner radius: 8 — egui 0.37 uses the corner_radius: CornerRadius field on each widget state (the old Rounding type and set_all_rounding helper are gone; assign per state in a loop).
- Spacing scale: 4/8/12/16 px; base text 14–15 px.

## Styling work
- ctx.style_mut_of(egui::Theme::Light, |s| …) and same for Dark: window_fill, hyperlink_color, selection (accent at ~25% alpha + a light stroke), corner_radius on all widget states, and spacing per the tokens.
- Class-based button theming with the new experimental StyleProvider (0.37):
  - Check egui's Cargo.toml/docs for the exact experimental feature flag and enable it.
  - impl StyleProvider<ButtonStyle> for MyTheme { fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle { … } } — use args.classes.has_class("primary" | "danger"), react to WidgetState::Hovered/Active with the gamma shifts, build a Frame with the token radius and white text. The official examples/styling_engine is the template — fetch it if unsure.
  - Register: ctx.add_widget_theme::<ButtonStyle>(MyTheme); (or ui.add_widget_theme per frame, as the example does); use egui::Button::new("…").with_class("primary").
  - If the experimental flag proves too unstable, fall back to a custom Button widget built from Frame + per-widget visuals — do not get blocked.
- Icons: a handful of Lucide SVGs via egui_extras (install_image_loaders).
- Light + dark: egui::global_theme_preference_buttons(ui); both themes must use the same tokens with different base colors.

## Fairness rules (all three prototypes follow these)
- Prototype and style the whole app.
- Inter font; light and dark both working.

## Definition of done
- cargo build --release passes; app runs; theme switch works without panics.
- Screenshots of the app in light and dark saved to docs/prototype-shadcn-{light,dark}.png.
- A short NOTES.md: the token table (colors, radius, spacing), what StyleProvider covered vs. what needed manual workarounds, and an honest effort assessment compared to a design-system crate.
