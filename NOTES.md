# Prototype A: shadcn-egui

Screenshots: `docs/prototype-shadcn-dark.png`, `docs/prototype-shadcn-light.png`.

## What changed

UI layer only (`src/ui/mod.rs`, `Cargo.toml`). Desk calls, keyboard handling, Note focus
and dialog decisions are unchanged; `cargo test` and `cargo clippy --all-targets` pass.

- egui/eframe were already on 0.37, so no upgrade was needed.
- Added `shadcn-egui = "0.8"` and `egui_extras` (svg). Setup installs the image loaders and
  applies `Theme::vega()`. Dark is the default. The old `set_text_sizes` override (16 px body)
  is removed, so text uses the theme's sizes: Inter, 14 px body, 20 px heading.
- Light/dark: a sun/moon ghost icon button in the top bar calls `ctx.set_theme(..)`. The
  choice is saved like the zoom (`theme` key), so a restart keeps it.
- Mapping:
  - Top bar: Ghost icon `Button` (sidebar toggle), outline `Badge` for the session,
    destructive `Badge` + `tooltip` for "Agent disconnected", secondary `Badge` for the header.
  - Sidebar: `Item` (Sm, clickable, Muted when selected) per round, `Badge` "New", Lucide icons.
  - Step bar: `Button` Sm. Current = Default, answered = Secondary + check, open = Outline,
    Review = Ghost/Default.
  - Question: `Card` with heading, `Separator` and Markdown body. The Recommendation text
    is an `Alert`.
  - Preview: `Card` (Sm). Note: `Label` + TextEdit (themed by the preset's visuals).
  - Footer: Ghost "Cancel round", Outline "Back", Default "Next"/"Submit" with Lucide icons.
    Shortcut hints are `Kbd` chips.
  - Review: one `Card` per question, an `Alert` for unanswered ones, a destructive
    "Unanswered" `Badge` with a destructive ring.
  - Dialogs: `Dialog` + `dialog::footer`. Destructive confirm for cancel/close, Default
    for "Submit anyway".

## Components that did not map

- **Option card**: shadcn has no selectable card / radio card. `Item` draws no selected look
  and clamps its description, so the card is an egui `Frame` painted from the palette:
  `card`/`border`, `accent`+`ring` on hover, a `primary` ring when selected. The radio/checkbox
  mark is a Lucide `IconView`, not `Checkbox`, because a nested checkbox would take the card's
  click.
- **Note field**: `Textarea` only takes an `id_salt`. The app needs the global
  `Id::unique` note id (Tab/Esc focus handling from `raw_input_hook`) and the grapheme-aware
  `TextEdit` wrapper, so it stays a themed `egui::TextEdit`.
- **Card click-to-edit (Review)**: `Card::show` returns no `Response`, so the click is an
  `interact` over the card's rect.
- Not used: Tabs, Switch, Select, Slider, Input, Popover. The app has no matching UI.
- Markdown bodies (`egui_commonmark`) render with the preset's egui visuals, not shadcn
  typography recipes.

## Suggested next step

Register an app variant (`ItemVariant::app`) for a "selectable option" item, with a primary ring
on `State::SELECTED` and no description clamp. Then the Option card becomes a real shadcn
component with focus ring and keyboard activation, instead of a hand-painted frame.
