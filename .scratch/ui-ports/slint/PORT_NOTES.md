# Slint port of the Question Desk — notes

Slint **1.18.1**, style set explicitly in `build.rs` (`native`, which resolves to Cupertino on
macOS, Fluent on Windows, Qt or Fluent on Linux; `QUESTION_DESK_STYLE` overrides it).
Backend **winit 0.30**, renderer **Skia over OpenGL** (explicitly selected in `src/ui/mod.rs`).
An optional `qt` cargo feature builds the Qt backend with the `qt` (QStyle) style.

Verified on macOS 27 (Apple Silicon) inside the nono sandbox: the GUI launches, the MCP
server answers on port 7463, rounds from the demo client appear and render (snapshots in
`shots/`), dialogs open, zoom from the saved state applies, `cargo build --release`,
`cargo test` and `cargo clippy --all-targets` pass without warnings. The tests in `tests/` and
`src/{server,format,schema,config,desk}.rs` are byte-identical to the parent repo. Not
verified: anything that needs a human at the keyboard (typing, IME, the Character Viewer,
menus, clicking), the real on-screen look including title bar and menu bar (no Screen
Recording permission), and Windows/Linux.

Files: `src/ui/app.slint` (UI), `src/ui/mod.rs` (read model → Slint, intents, dialogs,
window state), `src/ui/markdown.rs`, `src/ui/grapheme.rs`, `src/ui/macos_text_input.rs`,
`src/ui/snapshot.rs`, `build.rs`, `Cargo.toml`, `src/main.rs`, `tools/screenshot.sh`.
`theme.rs`, `icons.rs` and the Inter fonts are gone; the Lucide SVGs stay (Slint colourises
them).

## What was ported

| Area | Status |
|---|---|
| Sidebar: Agent Sessions, open rounds, open count, progress/age, "disconnected", "New" badge | Done. `ListView` of rows (session headers + rounds). Resizable by dragging the divider. |
| History | Not in this code base (`desk.rs` has no History), so nothing to port. |
| Top bar: sidebar toggle, logo, session / title, "Agent disconnected" + tooltip, `n / N`, header chip, Settings | Done (`Button`s with Lucide icons, `Tooltip`). egui's light/dark/system toggle moved to the native menu: View → Appearance. |
| Step bar | Done: one `Button` per Question (`primary` = current, "✓" = answered) + Review, tooltips with the question text. No wrapping (egui wrapped). |
| Question page: question, Markdown `body`, Recommendation text, Option cards, preview panel, Note | Done. Cards: `CheckBox` (multiSelect) or `RadioButton` from std-widgets as the mark, key cap, label, "Recommended" chip, description. Preview panel next to the Options when the page is ≥ 720 px wide and the Question has previews, else below; shows the hovered Option, else the first selected one. |
| Note field | `TextEdit`, placeholder text as in egui, `Tab` focuses it, `Esc` leaves it. |
| Review Page | Done: Unanswered count, warning-bordered cards, click to jump back. |
| Footer | Done: Cancel round / Back / Next–Submit, shortcut hints as key caps. |
| Confirmations: Cancel, Submit with Unanswered, Close | Done as Slint `Dialog` windows (platform button order). `Esc` dismisses; `Enter` confirms only Submit-with-Unanswered, as before. |
| Settings (host, port, token; save restarts the server) | Done as a `Dialog`. The token is a password `LineEdit` whose built-in eye button replaces egui's "Show token" checkbox. Loopback warning, errors, "Saved in …". |
| Empty state with the MCP URL / server error | Done. |
| Window attention on a new round | winit: un-minimise, raise to always-on-top for 300 ms, `request_user_attention(Informational)` (Dock bounce). Qt backend: `NSApp requestUserAttention` only. |
| Keyboard (README "Answering") | `1`–`9`, `Enter` (accept Recommendation), `Tab`/`Esc` (Note), `Esc` (Cancel round) via a root `FocusScope`; `⌘↩` / `⌘⇧↩` / `⌘,` / zoom as native menu shortcuts. |
| Zoom `⌘+` / `⌘-` / `⌘0` | Done by dispatching `ScaleFactorChanged` (whole UI scales, physical window size stays). Not verified interactively. |
| Window size, zoom, appearance, sidebar width remembered | `window.json` next to `config.toml` (eframe kept it in its own storage). Saved on quit/close. |
| Native menu bar | File (Settings… ⌘,, Close Window ⌘W), Edit (Cut/Copy/Paste/Select All, Emoji & Symbols ⌃⌘Space), View (show/hide open rounds ⌃⌘S, Appearance, zoom), Round (Next ⌘↩, Previous ⌘⇧↩, Review, Cancel Round…). Slint adds the App menu (About, Services, Hide, Quit) itself on macOS. |

Missing or weaker than egui:

- **Body height cap.** egui capped the body at 35 % of the window with its own scroll area. A
  nested `ScrollView` around `StyledText` mis-sized and drew outside its clip, so the body is
  shown at full height and the page scrolls instead.
- **Step bar does not wrap** (a round with ~25+ Questions overflows to the right).
- **Dialogs are separate windows**, not sheets: Slint has neither sheets nor app-modal
  windows. While one is open the main window ignores input (an overlay plus a modal flag) and
  the dialog is centred over it, but it can be moved behind the main window.
- **No Undo/Redo menu items**: `TextEdit` keeps its own undo stack (`⌘Z` works inside it), but
  exposes no function a menu item could call.
- **Edit menu items act on the Note only**; in the Settings dialog the `LineEdit`s get
  `⌘C`/`⌘V` directly because that window has no menu bar of its own.
- `Cmd+Q` (App → Quit, created by Slint/muda) terminates without the "Close the Question
  Desk?" confirmation; open rounds are cancelled after the event loop returns. (egui/winit had
  the same Quit item.)
- Option-card accessibility is a separate node per card (role checkbox / radio button,
  checked state, default action), but the visual `RadioButton` inside is a one-button
  `RadioGroup`, recreated whenever the selection changes because `RadioGroup` cannot be
  cleared from outside (single-select Options can be un-selected in this app).

## Markdown

Slint 1.18 has `StyledText` (`@markdown(...)` in `.slint`, `StyledText::from_markdown` in Rust).
It supports paragraphs, **bold**, *italic*, ~~strike~~, `inline code`, links (with a
`link-clicked` callback) and nested ordered/unordered lists; it rejects headings, code blocks,
tables, block quotes, rules, images, task lists and HTML blocks (parse error).

`src/ui/markdown.rs` therefore splits the body with `pulldown-cmark` into top-level blocks:

| Markdown | Rendering | Fidelity |
|---|---|---|
| Paragraphs, lists, inline styles, links | `StyledText` as is (re-serialised with `pulldown-cmark-to-cmark`) | Good. Soft breaks become spaces (StyledText would break the line). Links open in the browser (http/https/mailto only). |
| Headings | `StyledText` in bold at 1.45 / 1.25 / 1.1 rem | Good. |
| Fenced/indented code | Monospace `Text` (Menlo) on a sunken box, no wrapping | Good, no syntax highlighting (egui had none either). |
| Tables | One row of equal-width cells per table row, header row bold on a tinted background | Column widths are equal, not content-based; cells are plain text (inline styling dropped), because a `StyledText` reserves an extra line of height and doubled every row. |
| Block quotes | Grey `StyledText` with a bar | Nested blocks inside the quote are flattened. |
| Rules | 1 px line | Good. |
| Images, HTML | Plain text fallback | Images are not shown (egui_commonmark did not load remote images either). |

Every `StyledText` paragraph also reserves about one extra line below itself; between blocks
it reads as paragraph spacing, but the body box has a visible empty line at the end.

## Native-ness

### macOS (verified by snapshot; interaction not verified)

Wins:
- **Native menu bar** through muda, with real key equivalents shown in the menus, an App
  menu, and Edit → Emoji & Symbols that opens the system Character Viewer.
- **System font** (SF Pro via fontique) at the platform's 13 px; Inter is gone.
- **Accent colour** follows System Settings (Cupertino "accentifies" its blue), and the
  colour scheme follows the system, switchable per app under View → Appearance.
- Cupertino `Button`s (primary = accent), `CheckBox`, `RadioButton`, `LineEdit` (password
  field with eye button), `TextEdit`, `ScrollView` with overlay scrollbars, tooltips after
  a delay, `Dialog`s with platform button order (Cancel left of Save).
- IME (marked text via winit), text selection, word-wise and grapheme-wise cursor movement
  are Slint's own (it uses `unicode-segmentation` for arrows and forward delete). From the
  code they should behave like AppKit's in the common cases; not tried by hand here.
- Colour emoji render (Skia, Apple Color Emoji) in labels, the body, the Note and window
  titles (title bar is native AppKit).

Compromises:
- Everything inside the window is drawn by Slint. Cupertino is an imitation: it is close
  (control shapes, colours from the system grey ramp) but not AppKit — no vibrancy/translucent
  sidebar, no focus ring animation, no native context menu in text fields beyond Slint's,
  no services/spelling, no `NSTextView` features (look up, dictation inline UI, autocorrect).
- **Sheets** are separate windows (see above).
- Emoji are drawn noticeably smaller than the surrounding text (bitmap strike scaled down).
- **Character Viewer input still needs the winit 0.30 bridge** (`macos_text_input.rs`, now
  ~110 lines): Slint 1.18 still uses winit 0.30.13, whose `insertText:replacementRange:` drops
  text that arrives without marked text. The bridge forwards it as a Slint key event carrying
  the text; the menu item itself is a plain Slint `MenuItem`. The objc method swizzle from the
  egui version is kept; the hand-made `NSMenu` is gone.
- **Backspace over emoji**: Slint deliberately deletes one code point on Backspace (arrows
  and forward delete are grapheme-aware). So 👩‍🍳 ⌫ would leave "👩‍", 🇩🇪 ⌫ "🇩".
  `grapheme.rs` (now ~70 lines with tests) repairs such edits in the `edited` callback by
  removing the rest of an emoji cluster and resetting the cursor. Plain combining sequences
  keep Slint's behaviour.
- Semantic warning/success colours (systemOrange/systemGreen) are our own; `Palette` has none.

### Windows (expected, not verified)

`native` → Fluent: the most maintained Slint style, close to WinUI 3. Native menu bar via muda
(Win32 menu) with accelerators, dark-mode-aware. Dialogs are top-level windows. The macOS
bridge code is `cfg`-gated away. Skia uses D3D/OpenGL there; the explicit `skia-opengl` choice
may be worth revisiting (default Skia uses D3D on Windows).

### Linux (expected, not verified)

`native` → Qt style if Qt is found at build time, else Fluent. With winit, muda is not used;
Slint draws the menu bar inside the window. The Qt backend (feature `qt`) would give QStyle
widgets and a Qt menu bar on KDE.

## `native`/Cupertino versus `qt`

Built both (`cargo build --release` and `--features qt` into `target-qt/`, against Homebrew
qtbase 6.12; qtdeclarative is not needed). Snapshots: `shots/cupertino-*` and `shots/qt-*`.

Observed with the Qt backend on macOS:
- The QStyle-drawn controls (`NativeButton`, `NativeCheckBox`, …) look like flat grey
  Fusion-ish controls in the snapshot, not Aqua; the window chrome and fonts are native.
- The palette is a mix: `Palette` follows QPalette while our cards/chips use `Palette`
  brushes tuned for Cupertino, so contrast suffers (e.g. text on the "Recommended" chip).
  View → Appearance has no effect (the qt style ignores `Palette.color-scheme`).
- In the snapshot the menu bar is drawn by Slint inside the window, with garbled glyphs
  ("File" renders as "Djgc"); whether Qt also puts a native menu bar at the top of the screen
  could not be checked, because the sandbox cannot capture the screen.
- `Window::take_snapshot` called from a Slint timer made the Qt backend paint synchronously,
  which re-entered Slint's timer code and aborted ("Recursion in timer code", a panic in
  `_Slint_paintEvent`). Fixed in our hook by deferring the capture with `spawn_local`; worth
  knowing for any code that renders from a timer under Qt.
- Native `CheckBox` claims horizontal stretch differently from Cupertino (fixed in the layout).
- Needs Qt 6 at build time and its frameworks at run time (LGPL; must be bundled for
  distribution, ~20–30 MB), and no colour-emoji or IME advantage that we could verify.

**Recommendation: `native` (Cupertino on macOS) with winit + Skia/OpenGL as the default.**
It looks more coherent, follows the accent colour and appearance, needs no Qt, and the
native menu bar is real muda/AppKit. Keep `qt` as an experiment only.

## The style deprecation and what it means for us

Slint 1.16 made **Fluent the default style on all platforms**; `native` (or naming a style)
keeps the old platform-adaptive choice. The blog post
(<https://slint.dev/blog/default-native-style-change>) says the non-Fluent styles (Cupertino,
Material, Qt, Cosmic) "will receive less updates and eventually be deprecated", with no date,
and that Cupertino sits in an "uncanny valley". Only visuals are affected; scrolling physics
and input handling stay platform-specific.

For this app:
- We set `with_style("native")` explicitly, so the 1.16 default change does not affect us.
- Cupertino will fall behind Fluent and may disappear; the port then degrades to Fluent on
  macOS — a deliberate cross-platform look, not a native one. The UI uses only std-widgets
  and `Palette`, so switching style is a build-time setting (`QUESTION_DESK_STYLE=fluent`);
  nothing in `app.slint` depends on Cupertino internals, except the colour tuning of cards.
- "Native look on macOS" with Slint is therefore a shrinking target. If native look is the
  main criterion, Slint is the weaker bet of the three ports; if a consistent, well-maintained
  look is acceptable, Fluent is the safe long-term choice.

## Renderer and backend

| Option | Result on macOS 27 in the sandbox |
|---|---|
| winit + Skia (default graphics API: wgpu/Metal) | Window stays fully transparent: Slint keeps a new window at alpha 0 until the first frame (a wgpu workaround, `RevealOnFirstFrame`), and no frame was ever presented. |
| **winit + Skia over OpenGL** | Works; colour emoji; best text rendering. **Chosen.** |
| winit + FemtoVG (OpenGL) | Works; colour emoji; slightly softer text. |
| winit + software | Works, but **no colour emoji** (blank). |
| Qt (QPainter) | Works; see above. |

OpenGL is deprecated on macOS but still shipped; if Skia's Metal path is fixed upstream
(it may be a sandbox-only effect, which we could not test outside), switching back is one line.

## Screenshots in the sandbox

`shots/` (all taken with `tools/screenshot.sh`; `*-dialog.png` are the dialog windows):

- Cupertino, light and dark: `cupertino-{light,dark}-all.png`,
  `cupertino-{light,dark}-grilling-emoji.png`; plus `cupertino-light-previews.png`,
  `cupertino-dark-empty.png`, `cupertino-dark-review.png`, `cupertino-light-settings(-dialog).png`,
  `cupertino-dark-submit(-dialog).png`, `cupertino-light-cancel(-dialog).png`,
  `cupertino-light-about(-dialog).png`.
- Qt (system appearance, which was dark; the qt style ignores our Appearance switch):
  `qt-system-all.png`, `qt-system-grilling-emoji.png`, `qt-system-settings(-dialog).png`.

The nono sandbox has no Screen Recording permission (`CGPreflightScreenCaptureAccess() ==
false`), so `screencapture -l` fails. `tools/screenshot.sh` now falls back to asking an app it
started to render its own windows (`Window::take_snapshot`, `src/ui/snapshot.rs`, only with
`QUESTION_DESK_SNAPSHOT` set). These images show the window content only: no title bar, no
menu bar, no window shadow, no hover. `QUESTION_DESK_SHOT_DIALOG=review|settings|cancel|
submit|close` opens a page or dialog first and also writes `…-dialog.png`.

The script also uses port 7463, `XDG_CONFIG_HOME=<copy>/.xdg`, finds the window by the app's
PID (other ports run binaries also called `question-desk`), and takes `QUESTION_DESK_BIN`
(e.g. the Qt build) and `QUESTION_DESK_APPEARANCE`.

## Build time and binary size

Clean `cargo build --release --locked --bin question-desk` into fresh target directories, one
after the other on the same machine (Apple M5 Pro, crates already downloaded; Skia comes as a
prebuilt static library, so no C++ build of Skia). The egui build used the parent repo's
unchanged sources with `--target-dir` inside this copy.

| | egui 0.37 (original) | Slint 1.18.1, winit + Skia/OpenGL | Slint, `--features qt` |
|---|---|---|---|
| Clean release build, wall / CPU | 27.0 s / 337 s | 42.4 s / 414 s | ~43 s for the first build into an empty target dir (that run stopped at a compile error in our crate, so slightly more to completion) |
| Rebuild after a `.slint` change | — | ~15 s | ~16 s |
| Binary | 27.5 MB (21.4 MB stripped) | 34.4 MB (26.2 MB stripped) | 34.6 MB + QtCore/QtGui/QtWidgets frameworks (~36 MB) to ship |

The Qt variant still links Skia, because the `qt` feature adds the Qt backend on top of winit.

## Licensing

Slint is triple-licensed: GPL-3.0-only, the Slint Royalty-free Desktop, Mobile and Web
Applications License 2.0, or a paid commercial licence. The Question Desk has no licence
declared yet.

- **Royalty-free 2.0** (desktop apps, no embedded use): free, closed source allowed, on the
  condition of **attribution**: either show the `AboutSlint` widget in an About screen reachable
  from the top-level menu, or put the "Made with Slint" badge on the public download page. The
  port does the former: Help → About Question Desk shows `AboutSlint`. You may not
  re-export Slint's APIs or distribute Slint on its own.
- **GPL-3.0-only**: the whole app must be distributed under GPLv3 (source available,
  same licence for derivatives). Fine if the Question Desk goes open source under GPLv3; it
  rules out a permissive (MIT/Apache) or closed release unless the royalty-free licence is used.
- The `qt` variant adds Qt (LGPLv3 for qtbase): dynamic linking and shipping replaceable
  frameworks, plus Qt's notices.
- The egui stack was MIT/Apache only, so either Slint option adds an obligation the egui
  build did not have.

## Risks

- **Style deprecation** (above): Cupertino may stop getting fixes; the native look is the
  part most likely to rot.
- **winit 0.30** is still under Slint 1.18; the Character Viewer bridge swizzles a winit
  method by class name (`WinitView`) and must be removed when Slint moves to winit 0.31.
- **Skia Metal path** left the window invisible here; we pinned OpenGL.
- `RadioGroup` can't be cleared, so single-select cards rebuild their radio on each change.
- `StyledText` is young: extra line height, no tables/headings/code, Markdown errors are
  silent (we fall back to plain text).
- Dialogs as separate windows can get lost behind the main window.
- Zoom via a synthetic `ScaleFactorChanged` is unofficial and is reset when the window moves
  to a display with a different scale factor.
- Binary size ~34 MB (Skia), about 7 MB more than egui.
- The Qt backend did not show a clear native win and complicates distribution.

## Rating (macOS)

- **Native look: 5/10.** Native menu bar, system font, accent colour and dark mode are
  real; the window content is a decent Cupertino imitation that a Mac user still spots
  (control proportions, no vibrancy, dialogs as plain windows, small emoji). It is
  clearly more "Mac" than the bespoke egui theme, clearly less than AppKit or WebKit-with-
  system-CSS.
- **Feel: 6/10.** Keyboard model and shortcuts are intact, the menu gives discoverable
  key equivalents, text editing is solid (with our two repairs), scrolling is smooth. Minus
  points for separate-window dialogs, the body no longer being capped with its own scroll,
  and the still-required winit bridge. Interaction was not tested by hand in the sandbox.
