# Prototype B: "Dark Power Tool" (egui-elements, Tokyo Night)

UI layer only. `desk`, `server`, `schema` and `format` are untouched, as are the keyboard handling, dialog flow and focus rules in `ui/mod.rs`.

## What changed

- **egui/eframe**: already on 0.37, so no upgrade.
- **egui-elements 1.4, `full`**: vendored in `vendor/egui-elements` through `[patch.crates-io]`, with one fix (see below). 1.3.20 was no fallback because it needs egui 0.36.
- **`src/ui/appearance.rs`** (new):
  - Installs the theme (Tokyo Night by default), then Inter (`assets/fonts`, OFL) at `FontPriority::Highest` for `Proportional`, then the app's text sizes.
  - `theme.install` replaces the whole `egui::Style`, so the text sizes go back on after every install.
  - Dark/light switching builds a new `Theme` and installs it again.
  - The chosen `ThemeKind` is stored next to the zoom (`THEME_KEY`).
- **Appearance window** (gear in the top bar):
  - Dark/Light toggle and `utils::theme_switcher` with all built-in themes.
  - "Open theme editor…" opens the live `editor::ThemeEditor`; while it is open, the edited theme is installed again every frame.
  - "Ambient light" toggle (off by default) for `particles::Light`, tinted with the accent color, plus its themed `LightSettings` window.
- **Top bar**: a sun/moon button switches light/dark in one click.
- **Icons**: Lucide (`egui_extras` with `svg` for the image loaders).
- **`App::new`** was split out of `run()` so the UI can be rendered headless.
- **`src/ui/screenshots.rs`**:
  - An ignored test renders the real UI offscreen with `egui_kittest` (wgpu) and writes `docs/*.png`. Run it with `cargo test --release --lib screenshots -- --ignored`.
  - A regular test toggles dark↔light three times and opens the theme editor across frames.

## Widget mapping

| Before (egui) | Now |
|---|---|
| top bar `☰`/`⏴` button | `Button` + Lucide `PanelLeftOpen/Close`, `Badge::new(n)` = number of new rounds |
| sidebar `selectable_label` with `●` prefix | `Button::selectable(..).wrap()` full width, `Badge::dot()` for new rounds |
| step bar `egui::Button` with `.fill` | `Button` with `.selected` (current) / `.bg_color(success)` (answered); Review gets `Badge::new(unanswered)` |
| Option card `egui::Frame` + `☑ ◉` text | `egui_elements::Frame` with `FrameVisuals` (hover/click/selected fills, accent border) + Lucide `CircleDot/Circle/SquareCheck/Square` |
| Review entries `Frame::group` + `interact` | same interactive `egui_elements::Frame`, warning border when unanswered |
| footer buttons | `Button`: Cancel (error text, `X` icon), Back (`ChevronLeft`), Next (accent fill, `ChevronRight`), Submit (success fill, `Send`) |
| `egui::Modal` dialogs | `egui_elements::Modal` with heading, footer and `.alert(true)` for destructive dialogs |
| "Cancel round" / "Cancel rounds and quit" confirm | **`Button::hold(1.0)`** with error-colored sweep and `HoldBurst::Glow` |
| chips | `egui::Frame` tinted with theme accent/success |
| "⚠ Agent disconnected" | `Label::new(text, Some(TriangleAlert icon))` in the warning color |
| empty state | Lucide `Inbox` in the accent color above the heading |

`ComboBox` and `Window` are used in the Appearance window, theme switcher and editor. The app had no combo boxes of its own.

Behavior note: the Cancel and Close dialogs now need a 1 s hold on the confirm button instead of a click. This is the hold-to-confirm the prompt asked for. The flow is unchanged: Esc, the backdrop, × and "Keep answering" all keep answering, and Enter still confirms only the harmless Submit dialog.

## Awkward or unstable egui-elements APIs

- **1.4.0 does not build with eframe's `persistence` feature**, whatever crate features you enable. `SecureTextEditState` is compiled unconditionally and calls `get_persisted`, which needs serde derives that only exist under `egui/persistence`. The fix is two `cfg_attr(feature = "serde", …)` lines (vendored). Without it, you would have to drop persistence, which stores zoom and window size.
- **`Button::image` / `Button::image_and_text`** limit the image to the text height, so 18 pt Lucide icons came out as specks. The workarounds are `Button::new((image, text))` atoms and an explicit `fit_to_exact_size`.
- **`Badge` is a pure overlay.** It never reserves space, so it covers the label of tight buttons ("Review"). You have to widen the button yourself.
- **`theme.install` overwrites the whole `egui::Style`**, including your spacing and text sizes, on every install, theme switch and editor frame. Every caller has to reapply its own style tweaks.
- **`theme_switcher` and `ThemeEditor::show` install the theme themselves** and return the new one. That is easy to double-install or to miss the style re-tweaks after.
- **`ThemeEditor::new()` hardcodes a dark background**, so the editor window is dark in light mode unless you set `bg_color`.
- **`Modal::new(id, &mut open)`** differs from `egui::Modal`. Footer and content are separate closures, so two closures can't both mutate one "decision" variable; we route Enter through the content closure's return value.
- **Naming clashes**: `Button`, `Label`, `Frame`, `Modal`, `Window` and `ComboBox` shadow egui's. We import egui types qualified (`egui::Frame`) next to `egui_elements::Frame`.
- **`full` pulls in heavy dependencies** (xcap, enigo, image, secure-types) for the QR scanner, which is Linux/Windows only and unused here.

## Screenshots

- `docs/prototype-shadcn-dark.png` and `docs/prototype-shadcn-light.png`: file names as required by PROMPT.md, even though this is the Tokyo prototype.
- Extras: `docs/prototype-tokyo-review-dark.png`, `-cancel-dark.png` (hold-to-confirm modal) and `-appearance-light.png`.

They are rendered offscreen with egui_kittest because the window can't be brought to the front from inside the nono sandbox.
