# Port notes: Question Desk on wxDragon (wxWidgets)

The egui/eframe GUI is replaced by [wxDragon](https://github.com/AllenDang/wxDragon) 0.9.25,
safe Rust bindings to wxWidgets 3.3.4, so the window is built from the platform's own
controls: Cocoa on macOS, Win32 on Windows, GTK on Linux. `desk`, `server`, `format`,
`schema` and `config` are unchanged, and so are the tests in `tests/`. Only `src/ui/`,
`src/main.rs`, `Cargo.toml`, `build.rs`, `assets/windows/` and `tools/screenshot.sh` changed.

Status: macOS verified (build, tests, clippy, screenshots, keyboard path). Windows and Linux
not built here; see "Per platform".

## What was ported

| egui feature | wx port |
|---|---|
| Sidebar: Agent Sessions, open rounds, counts, "New" | `wxDataViewTreeCtrl` (NSOutlineView on macOS, GtkTreeView, generic on Win32): a row per Agent Session with its rounds; `Open Rounds (n)` above it; `●` marks unseen rounds, `⚠` disconnected ones; `answered/total` per row. Resizable and collapsible (`wxSplitterWindow`, View → Hide Sidebar ⌃⌘S). |
| Top bar | Round title, Agent Session, "⚠ Agent disconnected" (with the egui tooltip), "Question 2 of 5 · Header", a Settings… button. The window title shows the round title. |
| Step bar | A row of native toggle buttons `1 ✓`, `2`, …, `Review`; the current one is pressed, tooltips carry the question text. |
| Question page | Question title, Markdown `body` in a native web view, Recommendation text that matches no Option (`★ Recommended:`), Options, Note. Centered column up to 960 px, scrolls as a whole. |
| Option cards | Single-select: native radio buttons (`wxRB_SINGLE`, so the desk alone decides what is selected and nothing is ever pre-selected). `multiSelect`: native checkboxes. Bold label, `★ Recommended` in the accent colour, the digit key at the right, the description below (clicking it toggles too). |
| Preview panel | A second web view with the hovered Option's `preview` (else the selected one's), monospaced; beside the Options when the column is at least 720 px wide, below them otherwise. |
| Note | Native multiline `wxTextCtrl` (NSTextView / EDIT / GtkTextView) with the egui hint text as a caption above it (wxDragon cannot set a placeholder on multiline fields). |
| Review Page | One native group box (`wxStaticBoxSizer`, NSBox) per Question: `✓`/`⚠` and header in the box title, question, Answer or **Unanswered** in orange, Note, and an Edit button that jumps back. Count of unanswered Questions on top. |
| Cancel / Submit-with-Unanswered / Close confirmations | Native `wxMessageDialog` (NSAlert, TaskDialog, GtkMessageDialog) with the egui texts and button labels. Cancel and Close default to "Keep Answering"; Submit Anyway is the default button. |
| Settings dialog | Native modal dialog: host, port, token (password field with "Show token"), loopback warning, "Saved in …", Save/Cancel in platform order. Saving restarts the server; errors keep the dialog open. |
| Empty state | "No open questions" and the MCP URL (or why the server did not start). |
| Window attention | On a new round: de-iconize, raise, `RequestUserAttention` (Dock bounce / taskbar flash) without stealing keyboard focus. The egui version also put the window on top for a frame; wx's `Raise` already orders it to the front. |
| Keyboard (README "Answering") | `1`–`9`, `Tab` into the Note, `Esc` out of it, `Enter` accepts the Recommendation, `Esc` cancels (with confirmation): a `wxEVT_CHAR_HOOK` on the frame. `⌘↩` / `⌘⇧↩` (Ctrl on Windows/Linux) are menu accelerators of the Round menu, so they also show in the menu. Settings `⌘,`. |
| Zoom `⌘+` / `⌘-` | View → Zoom In / Zoom Out / Actual Size (`⌘=`, `⌘-`, `⌘0`) scale the page (fonts and web views). The chrome (sidebar, top bar, footer) keeps the system size. |
| Window size remembered | `window.toml` next to `config.toml`: size, maximized, sidebar width and visibility, zoom. Written when the window closes. |
| Menu bar | App menu (macOS, wx moves Settings… and Quit there), Edit (Undo/Redo/Cut/Copy/Paste/Select All on the standard ids, so wx maps them to AppKit's `copy:` etc.), Round (Next/Previous Question, Review, Cancel Round…), View (sidebar, zoom). On macOS wx adds the Window menu and AppKit adds Emoji & Symbols / Start Dictation to Edit. |

Missing or reduced compared with egui:

- **Sidebar rows are one line.** NSOutlineView rows in `wxDataViewTreeCtrl` do not wrap, so
  the "5 min ago" age is gone and the status is reduced to `●`/`⚠` marks plus
  `answered/total`. A custom `wxDataViewCustomRenderer` could draw two lines; not done.
- **No placeholder in the Note**: the hint is a caption above the field.
- **Radio buttons cannot be cleared by clicking** (native behaviour). The digit key toggles the
  Option off again, as in the egui version, and clicking its description toggles it too.
- **No "Recommended" chip**: it is accent-coloured text. No key-cap badges in the footer; the
  shortcut hints are one line of small grey text, and the menus show the accelerators.
- **Zoom** scales the page, not the whole window.
- **No light/dark/system switch in the window**: the app follows the system appearance (macOS
  View menu has none either). `QD_APPEARANCE=light|dark` forces one, for screenshots.
- History does not exist in this code base (the desk keeps no finished rounds), so there was
  nothing to port.

Removed: `theme.rs`, `icons.rs`, `grapheme.rs`, `macos_text_input.rs`, the Inter fonts and the
Lucide icons, and the `eframe`, `egui_extras`, `egui_commonmark`, `unicode-segmentation`,
`objc2-app-kit` and `objc2-foundation` (normal build) dependencies.

## Native-ness

### Wins (macOS, verified)

- Real AppKit controls: NSButton radio/checkbox/push/toggle buttons, NSTextView for the Note,
  NSTextField for Settings (with a real secure text field), NSOutlineView sidebar, NSBox
  group boxes, NSAlert confirmations, a real menu bar with an app menu. Focus rings, accessibility
  (VoiceOver sees real controls), text services, spell checking, dictation and the
  services menu come with them.
- **The egui workarounds are gone and not needed.** The Note is an NSTextView: the emoji picker,
  IME composition, umlauts, and grapheme-aware editing (ZWJ sequences, flags, skin tones) are
  AppKit's. Verified: synthetic AppKit key events typed `Hallo ä 👋` into the Note, and the
  Note renders `🧖‍♀️` (a ZWJ sequence) as one glyph (`shots/light-emoji.png`). Emoji render in
  colour everywhere, including labels, the window title and the sidebar.
- System font (SF Pro) at the system size, system colours, and light/dark mode follow the OS
  without any palette in the code; the accent colour is `NSColor.controlAccentColor`.
- Markdown renders in WKWebView: tables, code blocks, lists, links (opened in the browser), with
  `color-scheme: light dark`, the system font and size. Text is selectable and copyable.
- `⌘↩` etc. are real menu key equivalents, discoverable in the Round menu.
- Menu bar verified by reading it back from AppKit: app menu with Settings… ⌘, / Services /
  Hide / Quit ⌘Q (moved there by wx), Edit with Undo…Select All plus AppKit's own AutoFill,
  Start Dictation… and **Emoji & Symbols**, Round, View, and wx's standard Window menu.

### Compromises

- The page is a wx `ScrolledWindow` of controls, not an NSScrollView with a flipped document
  view: scrolling works but has no rubber-banding over the native controls (the web views do).
- The sidebar is an outline view, but not a vibrant "source list"; wx does not expose
  `NSTableViewStyleSourceList`.
- Static text does not wrap by itself in wx, so the page re-wraps labels on every resize.
- Web views size themselves by posting their content height to the app (a script message
  handler); a long body scrolls inside a view of at most about a third of the window.
- Warning orange and the macOS secondary-label grey are literal colours, picked per
  appearance (wx has no system warning colour, and maps grey text to the much fainter
  `disabledControlTextColor`).
- One radio button per Option with `wxRB_SINGLE` instead of a radio group, so selection state
  lives in the desk alone (GTK would otherwise force one button of a group on).

### Per platform

- **macOS (verified on this machine, Darwin 27):** as above.
- **Windows (expected, not built):** Win32 controls with Common Controls 6 visual styles and
  per-monitor-v2 DPI awareness from the embedded manifest (`build.rs` +
  `assets/windows/question-desk.{rc,manifest}`, `embed-resource`); no console window in release
  builds. `set_appearance(System)` turns on wx 3.3's dark mode for Win32 controls, which is
  still marked experimental in wxWidgets. The web views are WebView2 (Edge) if wxWidgets is
  built with it and the runtime is installed; wxDragon's build may fall back to the IE
  backend, which would break `color-scheme`, `color-mix()` and the height messages (needs a
  check on Windows). The sidebar's `wxDataViewCtrl` is wx's generic implementation on Windows,
  not a native list view. Accelerators read `Ctrl+…`, and the footer hints say so.
- **Linux (expected, not built):** GTK 3 controls and theme (including dark themes), WebKitGTK
  for the web views (needs `libwebkit2gtk` at build and run time), GtkTreeView sidebar.
  The `wxRB_SINGLE` radios avoid GTK's "one radio of a group is always on".

## Build time and size (macOS, Apple Silicon, release)

Both measured back to back from an empty target directory, same machine and toolchain,
`cargo build --release --bin question-desk`:

| | egui (original) | wxDragon (this port) |
|---|---|---|
| Clean release build, wall | 33 s | 105 s (1 min 45 s) |
| Clean release build, CPU (user+sys) | 368 s | 685 s |
| Incremental rebuild after touching a UI file | — | 4 s |
| Binary size | 27.5 MB | 15.2 MB |
| Binary size, stripped | 21.4 MB | 11.2 MB |
| Crates in `Cargo.lock` | 598 | 312 |

The wx binary contains all of wxWidgets (static) but none of egui's font rasterizer,
wgpu/glow, winit, the bundled fonts and icons; WebKit is the system's.

The wx build compiles wxWidgets 3.3.4 from source with CMake on the first build (wxDragon
downloads the source zip; no prebuilt libraries for this target), then caches it in the target
directory. Incremental rebuilds of the app take seconds.

## Dependencies and licensing

- `wxdragon`, `wxdragon-sys` 0.9.25: MIT OR Apache-2.0. They statically link
  **wxWidgets 3.3.4**, licensed under the wxWindows Library Licence (LGPL 2 with an exception
  that allows distributing statically linked binaries under your own terms). wxWidgets bundles
  third-party code with its own permissive licences (libpng, zlib, expat, PCRE2, …); the
  default features that pull in TIFF/JPEG/printing/Tango icons are switched off here.
- `pulldown-cmark` (MIT) was already a dependency; it now also renders HTML.
- `objc2` (MIT) on macOS for the accent colour; `block2` and `objc2-foundation` (MIT) only with
  the `snapshot` feature.
- `embed-resource` (MIT), build-time only, for the Windows manifest.
- Dropped: Inter (OFL) and Lucide icons (ISC), so no font or icon licences to ship.
- System libraries at run time: none extra on macOS (WebKit is part of the OS); WebView2
  runtime on Windows; GTK 3 and WebKitGTK on Linux.
- Build requirements: CMake and a C++ compiler (plus libclang for bindgen), which egui did
  not need.

## Risks

- **Crate maturity.** wxDragon is a young, mostly single-maintainer binding with frequent
  releases (0.9.x); some wx API is missing (text hints on multiline fields, row height on
  `wxDataViewTreeCtrl`, a few `wxID_*` constants, which are hard-coded here), and some
  behaviour is surprising (child `destroy()` is deferred to idle time, so a page being
  replaced must be hidden first). wxWidgets itself is mature (30 years), but 3.3 is a new
  stable line.
- **Threading.** wx is single-threaded. The desk's `on_change` fires on server threads; the UI
  marshals it with `wxdragon::call_after` + `wake_up_idle`, coalesced by an atomic flag.
  Creating a web view runs a nested event loop inside wx (it injects a script and waits), so
  a refresh can be re-entered; `refresh` guards against that and reschedules. Every user
  intent is followed by a deferred refresh, so no handler destroys the widget it runs in.
  Modal dialogs also run nested loops in which refreshes keep happening.
- **Markdown.** pulldown-cmark → HTML → WebKit/WebView2/WebKitGTK is faithful (tables, code,
  links), but it means three web engines to test, a WebView2 runtime dependency on Windows,
  and WebKit creating its website-data folders under `~/Library/WebKit/question-desk` (the
  nono sandbox denies that; it only logs). Raw HTML in a body is shown as text, the page has
  a CSP, and links open in the browser.
- **Build.** First build compiles wxWidgets (CMake, C++), which needs a toolchain on every CI
  runner; cross-compiling is harder than for egui.
- **Keyboard on macOS.** `Tab`/`Esc` in and out of the Note move focus after the key event
  (AppKit would otherwise undo the change). Verified with synthetic AppKit key events in a
  window that was not key; real typing in an active window was not testable in the sandbox.
- **Screenshots in the sandbox.** Window capture is denied here for every app (the egui build
  fails the same way). `tools/screenshot.sh` therefore builds with the `snapshot` feature and,
  when `screencapture` fails, asks the app to render its own window (AppKit
  `cacheDisplayInRect:` plus WKWebView snapshots). Those images show the window in its
  inactive state (grey selection, unpressed default button), because the app is never
  activated. The feature is off in normal builds.

## Rating (macOS)

- **Native look: 8/10.** Everything the user touches is an AppKit control in the system font,
  colours, accent and appearance; dialogs are NSAlert; the menu bar is real. Points off for
  the non-source-list sidebar with one-line rows, the hand-tuned spacing and orange of a
  wx sizer layout, and the toggle-button step bar, which no Mac app would use.
- **Feel: 7/10.** Text editing, IME, emoji, spell check, copy from the Markdown, menus and
  accelerators feel like a Mac app, because they are. Points off for the custom scrolling
  page (no rubber band, labels re-wrap on resize), single-click radios that cannot be
  unchecked, and the web views' small load-in delay when switching Questions.

## Screenshots

In `shots/` (dark is the system appearance here; light forced with `QD_APPEARANCE=light`):

- `all.png`, `all-light.png`: `screenshot.sh … all` (five rounds from five sources; the
  first one, an open question, is shown).
- `grilling-emoji.png`, `grilling-emoji-light.png`: `screenshot.sh … grilling emoji`.
- `previews.png`: Options with previews beside them, first Option selected.
- `review.png`: Review Page with answers, a Note with emoji and Unanswered Questions.
- `light-emoji.png`: light mode, selected radio, Note containing a ZWJ emoji.
- `submit-unanswered.png`, `settings.png`: the NSAlert confirmation and the Settings dialog.
- `empty.png`: empty state with the MCP URL.

## How to run

```sh
XDG_CONFIG_HOME=$PWD/.xdg cargo run --release          # port 7461 from .xdg/question-desk/config.toml
cargo run --release --bin demo-client -- --url http://127.0.0.1:7461/mcp all
tools/screenshot.sh -o shots/all.png all               # QD_APPEARANCE=light|dark, QD_AUTOPILOT=…
```
