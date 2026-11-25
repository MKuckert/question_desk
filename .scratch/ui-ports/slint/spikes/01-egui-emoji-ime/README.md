# Spike 1: color emoji, emoji picker input, umlauts/IME in egui 0.37 on macOS

Gate for ADR 0005. Throwaway crate; not part of the Question Desk build.

Stack: `eframe` 0.37 (default features), `egui_commonmark` 0.26, `egui_system_font_provider` 0.37 (pulled in by eframe's `system_fonts` default feature; added directly only for the headless test).

## How to run

```sh
cd spikes/01-egui-emoji-ime
cargo test --release -- --nocapture   # headless rendering check, writes target/font_atlas.png
cargo run --release                   # interactive window: work through the checklist
cargo run --release -- --screenshot /tmp/spike1.png   # renders the window once, saves a PNG, quits
```

The window needs a GPU, so it does not start inside the nono sandbox (`metal found no adapters`). Run it from a normal terminal.

In the window: tick each checklist item that works, then press **Print report to stdout**. The report lists the checklist, the fonts discovered, the codepoints in both text fields, and the last `Text`/`Ime`/`Key` events.

## Results

### Rendering: PASS (automated, headless)

`cargo test` shapes the sample emoji through `SystemFontProvider` on a headless `egui::Context`, then checks the shaped glyphs and the CPU-rasterized font atlas:

- `Apple Color Emoji` is discovered on demand (alongside `System Font` for `⌘` and `Heiti SC` for `日本語`).
- 😀 🎉 👍🏽 👨‍👩‍👧 🇩🇪 ❤️ ✅ 🦀 each shape to exactly one visible glyph, and that glyph is a color glyph (`is_color`). So skin-tone modifiers, ZWJ sequences, flags and VS16 are shaped correctly by harfrust.
- The atlas contains real color bitmaps (see `target/font_atlas.png`). Color glyphs are not tinted by the text color.

Findings from reading the source:

- The `egui_system_font_provider` README still says system emoji fonts are skipped. That line is out of date: 0.37 queries `GenericFamily::Emoji`, accepts `sbix`/`COLR`/`CBDT` glyphs, and enables `epaint/color_fonts` (vello_cpu) itself.
- eframe's default features include `default_fonts` but not `monochrome_emoji_fonts`. So the bundled NotoEmoji/emoji-icon-font do not cover emoji ahead of the system color font. Do not enable `monochrome_emoji_fonts`.
- epaint emits zero-width "continuation" glyphs so that glyph count equals char count, which cursor and selection code rely on. Multi-char emoji therefore have several `Glyph`s but only one visible one.

### Input, run 1 (2026-11-12): two failures, both fixed in the spike (confirmed by run 2)

**Emoji picker: FAIL out of the box.** Ctrl+Cmd+Space does nothing. There are two causes, both in winit 0.30.13 (the version eframe 0.37 uses):

1. winit's menu bar has only the app menu (Hide/Quit), no Edit menu. Ctrl+Cmd+Space is the key equivalent of AppKit's Edit → "Emoji & Symbols" item, so it has nothing to trigger.
2. Even with the picker open, winit drops what it inserts. `insertText:replacementRange:` only forwards text while an IME composition (marked text) is in progress, and the Character Viewer inserts without one. Upstream winit master fixes this ("AppKit can call `insertText:` directly for text selected from the Character Viewer…"), but only on the 0.31 line, still in beta.

Spike workaround (`src/macos_text_input.rs`, about 100 lines, macOS only, removable once eframe moves to winit 0.31):
- Add Edit → "Emoji & Symbols" (Ctrl+Cmd+Space → `orderFrontCharacterPalette:`) to winit's menu.
- Replace `WinitView`'s `insertText:replacementRange:` with a wrapper. Text that arrives with no marked text and outside a keyDown is queued and delivered through `App::raw_input_hook` as `egui::Event::Text`; the original method still runs.

**Backspace over multi-char emoji: FAIL out of the box.** egui deletes and moves by `char`, so a ZWJ family falls apart person by person and a flag loses its country. `TextBuffer::delete_previous_char`/`delete_next_char` are overridable, so `src/grapheme.rs` wraps `String` in a `GraphemeText` that deletes whole grapheme clusters (`unicode-segmentation`). After each frame it also snaps a cursor that landed inside a cluster to the cluster's edge (arrow keys, clicks). Unit-tested headlessly.

**Press-and-hold accent popup: not supported** (key repeat instead). Not needed; dropped from the checklist.

Confirmed by the event log and field codepoints in the raw report below:

- Umlauts on the German layout arrive as plain `Text` events (`ö` U+00F6, `ä` U+00E4, `ü` U+00FC).
- Dead keys go through winit's IME path: `Preedit "¨"` / `"^"` / `"´"` / `` "`" `` then `Commit("ü")`, `Commit("Â")`, `Commit("á")`, `Commit("à")`. So egui's preedit/commit handling works, which is what German input needs.
- A lone dead key followed by a non-combining key commits the bare accent (`Commit("¨")`, U+00A8 in the field). That is normal macOS behaviour.
- Option+a → `å` arrives as `Text`.
- Pasting emoji text keeps every codepoint intact (ZWJ U+200D, VS16 U+FE0F, skin tone, regional indicators).
- Emoji, umlauts and CJK render correctly in labels, tinted labels, text fields and the Markdown block (checked by the tester).

Not tested: CJK IME (preedit display, candidate window position). Dead keys already prove the preedit/commit path, so this only matters for CJK input.

### Input, run 2 (2026-11-13): PASS with both workarounds

- **Emoji picker:** Edit → "Emoji & Symbols" appears; Ctrl+Cmd+Space opens the picker; picked emoji land in both fields and render in color. Each pick logs `Character Viewer inserted "…"` exactly once, and the field holds exactly what was picked (`"😃😍ä"` = U+1F603 U+1F60D U+00E4). So the intercept neither doubles nor drops text, and typing afterwards is unaffected.
- **Skin tone from the picker** arrives as one insert (`"👍🏻"`). ZWJ/flag picks were ticked but are not in the log; pasted ZWJ/flag text was already shown intact in run 1.
- **Backspace/Delete and arrow keys** treat family, flag and skin-tone emoji as one unit.
- **Umlauts and dead keys** still work (`Ime Commit("ä")`, `Text "ß"`).
- Not verified: Fn/Globe key emoji picker, CJK IME. Neither blocks the gate.
- macOS logs `error messaging the mach port for IMKCFRunLoopWakeUpReliable` once when the picker opens. This is a common InputMethodKit message with no visible effect.

Raw output:

```text

[2026-11-13T08:56:48Z INFO  spike_egui_emoji_ime::macos_text_input] Character Viewer inserted "😃"
2026-11-13 09:56:48.965 spike-egui-emoji-ime[51842:2052350] error messaging the mach port for IMKCFRunLoopWakeUpReliable
[2026-11-13T08:56:48Z INFO  spike_egui_emoji_ime] Text "😃"  [U+1F603]
[2026-11-13T08:56:51Z INFO  spike_egui_emoji_ime] Key ControlLeft Modifiers::NONE
[2026-11-13T08:56:51Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers { ctrl: true }
[2026-11-13T08:56:53Z INFO  spike_egui_emoji_ime::macos_text_input] Character Viewer inserted "😍"
[2026-11-13T08:56:53Z INFO  spike_egui_emoji_ime] Text "😍"  [U+1F60D]
[2026-11-13T08:57:03Z INFO  spike_egui_emoji_ime] Key ControlLeft Modifiers::NONE
[2026-11-13T08:57:03Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers { ctrl: true }
[2026-11-13T08:57:05Z INFO  spike_egui_emoji_ime::macos_text_input] Character Viewer inserted "😍"
[2026-11-13T08:57:05Z INFO  spike_egui_emoji_ime] Text "😍"  [U+1F60D]
[2026-11-13T08:57:06Z INFO  spike_egui_emoji_ime] Key ControlLeft Modifiers::NONE
[2026-11-13T08:57:06Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers { ctrl: true }
[2026-11-13T08:57:07Z INFO  spike_egui_emoji_ime::macos_text_input] Character Viewer inserted "😝"
[2026-11-13T08:57:07Z INFO  spike_egui_emoji_ime] Text "😝"  [U+1F61D]
[2026-11-13T08:57:11Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers::NONE
[2026-11-13T08:57:11Z INFO  spike_egui_emoji_ime] Key ControlLeft Modifiers { mac_cmd: true, command: true }
[2026-11-13T08:57:59Z INFO  spike_egui_emoji_ime::macos_text_input] Character Viewer inserted "👍🏻"
[2026-11-13T08:57:59Z INFO  spike_egui_emoji_ime] Text "👍🏻"  [U+1F44D U+1F3FB]
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Key A Modifiers::NONE
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Text "a"  [U+0061]
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Key S Modifiers::NONE
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Text "s"  [U+0073]
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Key D Modifiers::NONE
[2026-11-13T08:58:24Z INFO  spike_egui_emoji_ime] Text "d"  [U+0064]
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Key Quote Modifiers::NONE
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Text "ä"  [U+00E4]
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Key Semicolon Modifiers::NONE
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Text "ö"  [U+00F6]
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Key OpenBracket Modifiers::NONE
[2026-11-13T08:58:31Z INFO  spike_egui_emoji_ime] Text "ü"  [U+00FC]
[2026-11-13T08:58:33Z INFO  spike_egui_emoji_ime] Key Minus Modifiers::NONE
[2026-11-13T08:58:33Z INFO  spike_egui_emoji_ime] Text "ß"  [U+00DF]
[2026-11-13T08:58:41Z INFO  spike_egui_emoji_ime] Key AltLeft Modifiers::NONE
[2026-11-13T08:58:41Z INFO  spike_egui_emoji_ime] Ime Preedit { text: "¨", active_range_chars: Some(1..1) }
[2026-11-13T08:58:42Z INFO  spike_egui_emoji_ime] Ime Preedit { text: "", active_range_chars: None }
[2026-11-13T08:58:42Z INFO  spike_egui_emoji_ime] Ime Commit("ä")
[2026-11-13T08:58:52Z INFO  spike_egui_emoji_ime] Key AltLeft Modifiers::NONE
[2026-11-13T08:58:52Z INFO  spike_egui_emoji_ime] Key ArrowLeft Modifiers { alt: true }
[2026-11-13T08:58:53Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:58:53Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:58:54Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:58:57Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers::NONE
[2026-11-13T08:58:58Z INFO  spike_egui_emoji_ime] Key SuperLeft Modifiers::NONE
[2026-11-13T08:58:58Z INFO  spike_egui_emoji_ime] Paste " 😀 🎉 👍🏽 👨\u{200d}👩\u{200d}👧 🇩🇪 ❤\u{fe0f} ✅ ⌘ 日本"
[2026-11-13T08:58:59Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:58:59Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:00Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:00Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:00Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:00Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:01Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:01Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:01Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:02Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:02Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:02Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:02Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:03Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:03Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:03Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:03Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:04Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:04Z INFO  spike_egui_emoji_ime] Key Backspace Modifiers::NONE
[2026-11-13T08:59:09Z INFO  spike_egui_emoji_ime] Key ArrowLeft Modifiers::NONE
[2026-11-13T08:59:09Z INFO  spike_egui_emoji_ime] Key ArrowRight Modifiers::NONE
[2026-11-13T08:59:09Z INFO  spike_egui_emoji_ime] Key ArrowRight Modifiers::NONE
[2026-11-13T08:59:10Z INFO  spike_egui_emoji_ime] Key ArrowLeft Modifiers::NONE
# Spike 1 report

## Checklist
- [x] Emoji in the sample labels render in color (not monochrome, not ◻)
- [x] Emoji keep their colors in tinted text (red/blue lines)
- [x] ZWJ family 👨‍👩‍👧, flag 🇩🇪 and skin tone 👍🏽 render as ONE glyph each
- [x] Menu bar shows Edit → Emoji & Symbols
- [x] Ctrl+Cmd+Space (or the Edit menu item) opens the emoji picker
- [x] Picked emoji is inserted into the single-line field
- [x] Picked emoji is inserted into the multi-line field
- [x] Picking a ZWJ/flag/skin-tone emoji inserts it whole (check the codepoints)
- [ ] Fn/Globe key (if set to 'Show Emoji & Symbols') also inserts
- [x] Typing after picking: no doubled or lost characters
- [x] Inserted emoji render in color inside the text field
- [x] Umlauts äöüß typed directly (German layout) appear correctly
- [x] Dead keys / Option+u then a → ä (US layout) produce the umlaut, no stray ¨
- [ ] IME (e.g. Japanese Kana/Romaji, Pinyin): preedit is shown, commit inserts text
- [ ] IME candidate window appears near the cursor
- [x] Backspace/Delete remove a whole emoji (family, flag, skin tone) in one press
- [x] Arrow keys step over a whole emoji (typing after one arrow press keeps it intact)
- [x] Copy/paste of emoji text in and out of the field works
- [x] Markdown block renders emoji in color and umlauts correctly

## Fonts discovered via SystemFontProvider
- system:Apple Color Emoji:0:0
- system:System Font:0:0
- system:Heiti SC:1:1
- system:Apple Color Emoji:0:0
- system:System Font:0:0
- system:Heiti SC:1:1

## Single-line field
"😃😍ä"
[U+1F603 U+1F60D U+00E4]

## Multi-line field
"asdäöüß"
[U+0061 U+0073 U+0064 U+00E4 U+00F6 U+00FC U+00DF]

## Last input events
- Text "ö"  [U+00F6]
- Key OpenBracket Modifiers::NONE
- Text "ü"  [U+00FC]
- Key Minus Modifiers::NONE
- Text "ß"  [U+00DF]
- Key AltLeft Modifiers::NONE
- Ime Preedit { text: "¨", active_range_chars: Some(1..1) }
- Ime Preedit { text: "", active_range_chars: None }
- Ime Commit("ä")
- Key AltLeft Modifiers::NONE
- Key ArrowLeft Modifiers { alt: true }
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key SuperLeft Modifiers::NONE
- Key SuperLeft Modifiers::NONE
- Paste " 😀 🎉 👍🏽 👨\u{200d}👩\u{200d}👧 🇩🇪 ❤\u{fe0f} ✅ ⌘ 日本"
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key Backspace Modifiers::NONE
- Key ArrowLeft Modifiers::NONE
- Key ArrowRight Modifiers::NONE
- Key ArrowRight Modifiers::NONE
- Key ArrowLeft Modifiers::NONE
```

## Verdict

**Gate passed. ADR 0005 stands:** eframe/egui 0.37 + `egui_commonmark` + system fonts.

- Color emoji, umlauts and dead keys work out of the box.
- The MVP's `ui` module must carry two pieces from this spike:
  - `macos_text_input` (Edit → Emoji & Symbols menu item and the `insertText:` bridge into `raw_input_hook`). macOS only; remove it once eframe uses winit ≥ 0.31.
  - `GraphemeText` + cursor snapping for every `TextEdit` (Note fields), so emoji are edited as single units.
- Do not enable `monochrome_emoji_fonts`.
- Open, non-blocking: CJK IME candidate window placement; Fn/Globe key picker.
