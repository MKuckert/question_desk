# 6. Bespoke egui theme instead of a design-system crate

Status: accepted (2026-11-15)

## Context

The MVP UI used egui's default look. We built three throwaway prototypes of a restyled UI on branches `prototype/bespoke`, `prototype/shadcn` and `prototype/tokyo`. All three restyle the whole app, use Inter, support light and dark, and leave desk, server and keyboard logic unchanged.

- **Bespoke**: pure egui 0.37. It has its own design tokens, a warm amber accent in dark and a deep teal accent in light, and Lucide icons via `egui_extras`. Buttons are styled through egui's experimental `StyleProvider` with classes such as `primary`, `danger`, `ghost`, `nav` and `step`. Cards, chips and radio marks are built from `Frame` and `Painter`.
- **shadcn**: `shadcn-egui` 0.8 with the `vega` preset. It is clean and functional, but almost entirely black and white, even emoji lose their color, and the grey Note field stands out oddly. It has no selectable card, so the Option card was still hand-built.
- **Tokyo**: `egui-elements` 1.4 with the Tokyo Night themes. The crate had to be vendored (about 24k lines) to fix a build break with eframe's `persistence` feature. Its `full` feature pulls in heavy unused dependencies. `theme.install` overwrites the whole `egui::Style`. It also brought a theme switcher, a live theme editor, particles and hold-to-confirm. The editor is a playground this app doesn't need, and the Tokyo look didn't win on taste.

Bespoke was preferred. Its colors work in both themes, especially the Recommended chip; its icons are much better than the MVP's; and its hover feedback is good. It was also the cleanest implementation of the three.

## Decision

Adopt the bespoke prototype as the UI layer:

- All tokens (palettes, radius, spacing, text sizes), fonts and button classes live in `src/ui/theme.rs`. Views take colors only from `theme::of(ui)` and the token constants, never from color literals.
- The only new dependency is `egui_extras` (feature `svg`) for bundled Lucide icons. `eframe` enables the `experimental` feature for `StyleProvider`.
- Inter Regular and SemiBold are bundled under the OFL. Weight comes from a separate `inter-semibold` family, because `strong()` only changes the color. The system font provider stays behind Inter as a fallback, so color emoji keep working (ADR 0005).

Two ideas from the other prototypes are to be folded in on top:

- Footer shortcut hints are shown as key chips on their own row (from shadcn), replacing the single long text line.
- The sidebar gets a count of open Rounds and an inline "New" badge or dot for unseen Rounds, laid out inline rather than as overlays (from shadcn and Tokyo).

Rejected: the shadcn-egui and egui-elements crates, a theme switcher or editor, particles, and hold-to-confirm for cancelling. Cancel and Close keep the click-to-confirm dialog.

## Consequences

- No third-party design system to keep in step with egui releases, but every component except buttons is our own code.
- `StyleProvider` sits behind egui's `experimental` feature and may break on upgrades. It is used only by `ButtonTheme` in `theme.rs` (about 70 lines). If it breaks, replace it with a helper that sets the fill and stroke on a plain `Button`.
- The theme must be set with `style_mut_of(Theme::Light | Theme::Dark, …)`. `global_style_mut` changes only the current theme, so text sizes are lost on a switch.
- `Color32::gamma_multiply` also scales alpha, so pressed fills use `theme::pressed`, which scales only the RGB channels.
- Text on the amber accent needs a near-black `on_accent` color for contrast.
