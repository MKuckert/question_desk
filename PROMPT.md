# Prototype A: "Modern Web App" — shadcn-egui design system

## Context
- Existing app (Rust, egui/eframe — check its Cargo.toml for the version).
- Goal: a styled prototype of this app using the shadcn-egui design system (shadcn/ui v4 look: neutral, typography-driven, light + dark from one preset).
- This is a UI-layer change only. Do NOT change app logic, data flow, or behavior.
- Verified research with snippets and API notes: /workspace/research/egui-beautiful-apps.md — consult it if you get stuck, and verify any API doubt against docs.rs or the official examples on github.com/emilk/egui (raw fetch works).
- Use port 7461 in your environment for testing purposes.

## Setup
1. If the app is on egui < 0.37, upgrade to egui/eframe 0.37 first and fix breaking changes (notably: Rounding → CornerRadius with per-widget-state corner_radius fields, new Id APIs, global_theme_preference_switch → global_theme_preference_buttons). Verify `cargo build` passes before styling.
2. Add to Cargo.toml:
   shadcn-egui = "0.8"            # Inter font + Lucide icons are always included
   egui_extras = { version = "0.37", features = ["svg"] }
3. In app setup (CreationContext):
   - egui_extras::install_image_loaders(&cc.egui_ctx);  // for Lucide SVG icons
   - shadcn_egui::Theme::vega().apply(&cc.egui_ctx);    // default look; alternatively Preset::from_code("<code>").apply(ctx) with a preset pasted from ui.shadcn.com (returns a Result; handle it)
   - ctx.set_theme(egui::Theme::Dark);                  // default dark, support light

## Styling work
- Replace stock egui widgets with shadcn components wherever they exist: Button, Badge, Input, Label, Card, Tabs, Switch, Checkbox, Select, Slider, Separator, Tooltip, Popover, Dialog, Icon (IconView). Components live in shadcn_egui::components::* — read docs.rs/crate/shadcn-egui for exact signatures.
- Structure content in Cards with a clear heading hierarchy; use Tabs/Separator for grouping.
- Add Lucide icons to primary actions and navigation via IconView.
- Light + dark: the preset provides both palettes — switch with egui::global_theme_preference_buttons(ui) (or your own toggle calling ctx.set_theme(..) and re-applying). Both must look good.
- Typography: keep Inter (the default here) at 14–15 px base; use the component size variants rather than hand-tweaking.

## Fairness rules (all three prototypes follow these)
- Prototype and style the whole app.
- Inter font; light and dark both working.

## Definition of done
- cargo build --release passes; app runs; theme switch works without panics.
- Screenshots of the app in light and dark saved to docs/prototype-shadcn-{light,dark}.png.
- A short NOTES.md: what you changed, any shadcn components you couldn't map to existing UI, and one suggested next step.
