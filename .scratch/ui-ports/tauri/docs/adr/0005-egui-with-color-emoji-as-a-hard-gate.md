# 5. egui/eframe for the GUI, with color emoji support as a hard gate

Status: accepted (2026-11-12)

## Context

The user wants a Rust GUI built with egui. Typing emoji via the native OS input (e.g. the macOS emoji picker) and seeing them rendered in color is a must-have. egui 0.37's `epaint` has a color-glyph path, and the official `egui_system_font_provider` crate loads system fonts on demand, including `sbix` (Apple Color Emoji), `COLR` and `CBDT` glyphs. Spike 1 (`spikes/01-egui-emoji-ime/`) confirmed this on macOS.

## Decision

Use `eframe`/`egui` 0.37 with `egui_commonmark` for Markdown and `egui_system_font_provider` for emoji and fallback glyphs. Before any UI work, a spike must show on macOS: emoji picker input arrives in a text field, emoji render in color, umlauts/IME input work. If the spike fails, we switch the GUI library before building the UI.

macOS is the MVP platform; Linux and Windows are explicit targets, so platform-specific code is avoided unless necessary.

## Consequences

- The spike is the first piece of work and can invalidate this ADR.
- egui renders text itself; anything the spike does not cover (e.g. complex scripts, CJK IME candidate window placement) may need later attention.
- Spike 1 passed (2026-11-13), with two app-side workarounds the `ui` module must carry:
  - **macOS emoji picker bridge.** winit 0.30 (used by eframe 0.37) has no Edit menu, so Ctrl+Cmd+Space does nothing. It also drops text the Character Viewer inserts outside an IME composition. We add Edit → "Emoji & Symbols" and wrap `WinitView`'s `insertText:replacementRange:` to feed such text to egui via `raw_input_hook`. Fixed upstream in winit 0.31; remove the bridge when eframe adopts it. This is the one deliberate piece of platform-specific code.
  - **Grapheme-aware editing.** egui deletes and moves by `char`, which splits ZWJ, flag and skin-tone emoji. Note fields use a `TextBuffer` wrapper that deletes whole grapheme clusters and snap the cursor to cluster boundaries.
- Keep eframe's default fonts without `monochrome_emoji_fonts`; the bundled monochrome emoji fonts would otherwise shadow the system color emoji font.
