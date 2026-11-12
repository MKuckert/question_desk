# 5. egui/eframe for the GUI, with color emoji support as a hard gate

Status: accepted (2026-11-12)

## Context

The user wants a Rust GUI built with egui. Typing emoji via the native OS input (e.g. the macOS emoji picker) and seeing them rendered in color is a must-have. egui 0.37's `epaint` has a color-glyph path, and the official `egui_system_font_provider` crate loads system fonts on demand, including `sbix` (Apple Color Emoji), `COLR` and `CBDT` glyphs. Source reading only, not yet run.

## Decision

Use `eframe`/`egui` 0.37 with `egui_commonmark` for Markdown and `egui_system_font_provider` for emoji and fallback glyphs. Before any UI work, a spike must show on macOS: emoji picker input arrives in a text field, emoji render in color, umlauts/IME input work. If the spike fails, we switch the GUI library before building the UI.

macOS is the MVP platform; Linux and Windows are explicit targets, so platform-specific code is avoided unless necessary.

## Consequences

- The spike is the first piece of work and can invalidate this ADR.
- egui renders text itself; anything the spike does not cover (e.g. complex scripts) may need later attention.
