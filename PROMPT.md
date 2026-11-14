# Prototype B: "Dark Power Tool" — egui-elements (TokyoNight)

## Context
- Existing app (Rust, egui/eframe — check its Cargo.toml for the version).
- Goal: a dark, rich "carefully crafted developer product" look using egui-elements: TokyoNight palette, themed widgets with personality (corner badges, hold-to-confirm buttons, glow effects).
- This is a UI-layer change only. Do NOT change app logic or behavior.
- Verified research with snippets and API notes: /workspace/research/egui-beautiful-apps.md — consult it if stuck; verify API doubts against docs.rs/crate/egui-elements or the GitHub README (greekfetacheese/egui-elements).
- Use port 7462 in your environment for testing purposes.

## Setup
1. If the app is on egui < 0.37, upgrade to egui/eframe 0.37 first and fix breaking changes (Rounding → CornerRadius per-widget-state fields, new Id APIs, global_theme_preference_buttons). Verify the build before styling.
2. Add to Cargo.toml:
   egui-elements = { version = "1.4", features = ["full"] }
   If 1.4 fails to build, fall back to 1.3.20. (full enables Lucide icons + composites; widgets and themes work with no features.)
3. In app setup:
   let mut theme = egui_elements::Theme::new(egui_elements::ThemeKind::TokyoNight);
   theme.install(&cc.egui_ctx); // writes egui::Style + per-widget visuals
   - After theme.install, install the Inter font at highest priority for cross-prototype consistency (keep the theme's colors) — use the 0.37 ctx.add_font(FontInsert::new(...)) pattern from egui's examples/custom_font (imports: egui::epaint::text::{FontInsert, InsertFontFamily}).
   - If you use Lucide icons: egui_extras::install_image_loaders(&cc.egui_ctx); (egui_extras with the svg feature).

## Styling work
- Swap in egui-elements widgets where they exist: Button, Label, ComboBox, Modal, Window. Name-collision warning: these shadow egui types — import the crate's widgets after `use eframe::egui::*`, or alias them.
- Use Badge on buttons that carry notifications (unread counts, error states) — top-right corner, theme error color; Badge::dot() for a plain dot.
- Destructive / irreversible actions: Button::new("…").hold(1.0) — hold-to-confirm with progress sweep and completion burst. Use it for at least one real destructive action in the app.
- Light mode: ThemeKind::TokyoNightLight — on switch, construct the new theme and call theme.install(ctx) again (always re-install when switching).
- If the crate's live ThemeEditor (editor::ThemeEditor + utils::theme_switcher) is available, wire it into a settings menu so the user can tweak live — keep TokyoNight as the default.
- Optional "wow" (keep subtle, behind a settings toggle): particles::Light or Snow with its themed settings window.

## Fairness rules (all three prototypes follow these)
- Prototype and style the whole app.
- Inter font; light and dark both working.

## Definition of done
- cargo build --release passes; app runs; dark ↔ light theme switch works without panics.
- Screenshots of the app in light and dark saved to docs/prototype-shadcn-{light,dark}.png.
- A short NOTES.md: what you changed, widget mappings, and any egui-elements APIs that felt awkward or unstable.
