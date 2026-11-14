# Prototype C: Bespoke Brand (pure egui 0.37)

Screenshots: `docs/prototype-shadcn-dark.png`, `docs/prototype-shadcn-light.png` (names as given in PROMPT.md).

## What changed

UI layer only: `src/ui/theme.rs` (tokens, themes, button `StyleProvider`), `src/ui/icons.rs` (Lucide SVGs), `src/ui/mod.rs` (restyled views), `assets/` (Inter 4.1 Regular + SemiBold under OFL, 13 Lucide icons under ISC). Desk calls, keyboard handling, Note focus and dialog decisions are unchanged; `cargo test` and `cargo clippy --all-targets` pass.

- egui/eframe were already on 0.37, so no upgrade was needed.
- New dependency: `egui_extras` with the `svg` feature only. The `experimental` feature is enabled on `eframe` (it forwards to `egui/experimental`) for `egui::theme`.
- `QUESTION_DESK_THEME=light|dark` overrides the saved theme preference at startup. It exists for the screenshots and can be removed.

## Tokens

| Token | Dark | Light |
|---|---|---|
| bg (page) | `#141311` | `#FAF9F6` |
| panel (bars, sidebar) | `#1B1A17` | `#F3F2EE` |
| surface (cards, inputs, dialogs) | `#22201C` | `#FFFFFF` |
| sunken (preview, code) | `#0F0E0C` | `#F0EFEA` |
| hover / press surface | `#2B2924` / `#34312B` | `#F1F0EB` / `#E7E5DE` |
| border / border strong | `#34312B` / `#4A463E` | `#E2E0D9` / `#CBC8BF` |
| text / strong / muted | `#E7E3DA` / `#FAF7F0` / `#9B958A` | `#2A2925` / `#12110F` / `#6F6B62` |
| **accent** | warm amber `#E9A23B` | deep teal `#0F766E` |
| on accent | `#1A1206` | `#FFFFFF` |
| success / warn / danger | `#6BCB8B` / `#F0795B` / `#D94A4A` | `#15803D` / `#B45309` / `#C2322F` |

| Derived / scale | Value |
|---|---|
| hover fill | `accent.gamma_multiply(1.4)` |
| active fill | accent RGB × 0.7, alpha kept (see below) |
| selection | accent at 25 % + 1 px accent stroke |
| radius | 8 (all widget states, cards, chips); 12 for windows/dialogs |
| spacing | 4 / 8 / 12 / 16 (`SPACE_XS..SPACE_L`); item spacing 8, button padding 12×6, window margin 16 |
| type | Inter; body 15, button 14, small 12, mono 13, title 20 and heading 24 in Inter SemiBold |

## What StyleProvider covered

- **Buttons, fully.** One `StyleProvider<ButtonStyle>` handles the classes `primary`, `danger`, `ghost`, `nav` (sidebar entries, using egui's built-in `selected` class), and `step` / `step-current` / `step-done` (step bar). It also handles hover and press shifts, icon tint and alignment. The step bar no longer sets per-button `.fill()` colors, and the dialog confirm buttons became `primary` or `danger`.
- Classes compose well: `.with_class(STEP).with_class_if(STEP_DONE, …)`.

## What needed manual work

- **There is one provider per style type, and it applies to every button in the app.** The provider must delegate unclassed buttons to `egui::theme::DefaultStyle` itself, otherwise it would restyle everything, including egui's own theme-preference buttons.
- **Only Button, TextEdit, Checkbox and Separator have style types.** Frames, labels, panels, modals and scroll areas do not. So the option cards, review cards, chips, key badges, the preview panel and the radio/checkbox marks are hand-built from `Frame` and `Painter` with the tokens. Hover feedback on cards uses `Frame::begin` / `Prepared::paint`, so the fill can change after the hover is known.
- **Global visuals still do most of the work:** `style_mut_of(Theme::Light/Dark)` sets fills, strokes, corner radius in a loop over widget states, selection, shadows and spacing. `global_style_mut`, which the app used before, changes only the current theme, so switching themes lost the text sizes.
- **`gamma_multiply(0.7)` also scales alpha.** On the light theme the "active" fill became lighter instead of darker, so `pressed()` scales only the RGB channels. Because 1.4 clips, the amber hover drifts toward yellow.
- **White text on amber has poor contrast**, so the dark theme uses a near-black `on_accent`.
- **`strong()` only changes the color.** Real weight needs a second font family (`FontFamily::Name("inter-semibold")`) and explicit `.font(semibold(..))` calls. Color emoji still fall back through the system font provider in both families (🎉 in the screenshots).
- **The API is experimental.** It is hidden from docs without the feature, and the PROMPT's `experimental_theme` name doesn't exist; the feature is `experimental`. Expect breakage on upgrades.

## Effort compared to a design-system crate

About half a day for a coherent result. The theme module (~420 lines including tests) is a one-time cost. Most of the time went into views that StyleProvider can't reach (cards, chips, marks, sidebar entries), which a design-system crate would mostly provide as ready-made components. In return, every color and size is ours, light and dark share one token set, and there's no third-party crate to keep in step with egui releases. If the app grows many more custom components, a crate saves more time. For this app's small set of views, the bespoke route is affordable.
