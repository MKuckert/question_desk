# Port notes: Question Desk on Day

The egui/eframe GUI is replaced by [Day](https://daybrite.dev) 0.7.10 (`day-app` from
crates.io; the `marcprux/day` fork's sources, 0.7.6, were used as documentation). Day is a
declarative Rust framework whose pieces are the platform's own widgets: AppKit on macOS,
WinUI 3 on Windows, GTK 4 or Qt 6 on Linux. `desk`, `server`, `format`, `schema`, `config`,
`lib.rs`, the demo client and `tests/` are byte-identical to the original. Only `src/ui/`,
`src/main.rs`, `Cargo.toml` and `tools/screenshot.sh` changed.

Status: macOS verified (plain `cargo build --release`, `cargo test`, clippy clean, screenshots,
keyboard and click paths driven with synthetic AppKit events). Windows and Linux not built
here; see "Per platform".

**Feasibility gate (done first): passed.**

1. *Threads.* Day's signals are main-thread only, but `day::reactive::on_main` (and `Setter`)
   take `Send` closures from any thread. `Desk::on_change` posts one coalesced `on_main`
   refresh that bumps a revision signal; everything on screen derives from it via memos.
2. *tokio.* Day has its own main-thread executor (`day::task`) and needs no tokio; the
   existing `ServerControl` keeps its own multi-threaded runtime unchanged. Day's docs
   describe exactly this pattern (`docs/async.md`, "Keep other async runtimes in app crates").
3. *Widgets.* Present: source-list sidebar (`nav` = `NSSplitViewController` + `NSOutlineView`),
   scroll, split, segmented control, text field / secure field / text view, buttons, native
   window toolbar, `NSAlert` sheets, menu bar with roles, a Settings window
   (`register_preferences`), Dock bounce (`request_attention`), remembered window frame,
   own-window capture. Missing: a checkbox (Day's `toggle` is an `NSSwitch`) and a radio button
   with a description; done with a native tweak (below).
4. *Markdown.* Day 0.7.10 ships a `markdown` piece built from native labels (headings,
   paragraphs with inline styles and links, lists, quotes, code blocks, rules). No tables, so
   the port cuts GitHub tables out with pulldown-cmark and draws them as a `grid` of labels.
   No web view involved (`day-piece-webview` exists if ever needed).
5. *Build.* `day build` for macOS goes through a generated Xcode project and produces a real
   `.app`. A plain `cargo build` with `day-app`'s `appkit` feature also works and runs as an
   unbundled binary; the port selects the backend per target in `Cargo.toml`, so `cargo
   build`/`cargo test` need no `day` CLI. Tests are unaffected (they never touch the UI).

## What was ported

| egui feature | Day port |
|---|---|
| Sidebar: Agent Sessions, open rounds, counts, "New" | `nav(...).style(Sidebar)`: the real macOS source list (sidebar material, full height under the title bar, native collapse button). One section header per Agent Session, one row per round, the row badge shows `New`, else `answered/total`, prefixed `⚠` when disconnected. "n open rounds" above the list. |
| Top bar | Native window toolbar: the window title is "Round title — Agent Session", the trailing toolbar label "Question 2 of 5 · Header" (or "Review · n questions"), "⚠ Agent disconnected" when no call waits, and a Settings gear (SF Symbol). |
| Step bar | A native segmented control (`NSSegmentedControl`): `1 ✓ | 2 | … | Review`, the current step selected; clicking a segment jumps. |
| Question page | Question title, Markdown `body` on a native card, Recommendation that matches no Option (badge + text), Options, Note. Centered column up to 900 pt, the whole page scrolls (`NSScrollView`). |
| Option cards | Single-select: native radio buttons; `multiSelect`: native checkboxes (`NSButton` of type Radio/Switch). Title = Option label, a "Recommended" capsule badge, the digit key at the right, description below (clicking it toggles too). The desk alone decides selection, so radios can be cleared again, as in egui. |
| Preview panel | A card with the hovered Option's `preview` (else the selected one's), monospaced and selectable; beside the Options on a regular width, under them on a compact one (`adaptive`). |
| Note | Native `NSTextView` (Day `text_area`), with a bezel and live placeholder (both added by tweak, see below). |
| Review Page | One card per Question: ✓/⚠, number, header, question, Answer or *Unanswered* in the system warning colour, Note, and an Edit button. Unanswered count on top. |
| Cancel / Submit-with-Unanswered / Close confirmations | Native `NSAlert` sheets with the egui texts. Cancel and Close put "Keep Answering" first (it takes Return) and the destructive button second, drawn red (`hasDestructiveAction`), so they need a click. "Submit Anyway" takes Return. ⌘Q asks the same question as the close button. |
| Settings dialog | A separate macOS Settings window (App menu ▸ Settings… ⌘, or the toolbar gear): a `form` with Host, Port, Access token (secure field, "Show token" checkbox), loopback / error warnings, Cancel / Save, "Saved in …". Saving restarts the server; errors keep the window open. |
| Empty state | "No open questions" and the MCP URL (or why the server did not start), with a "Waiting for Agents" row in the sidebar (see Compromises). |
| Window attention | On a new round: un-minimize, float the window above others for 200 ms, `request_attention(Informational)` (Dock bounce), without activating the app or taking focus. |
| Keyboard (README "Answering") | `1`–`9`, `Tab` into the Note, `↩` accepts the Recommendation: an invisible full-keyboard canvas behind the page holds focus while the Note does not. `Esc` is a menu key equivalent (Round ▸ Cancel Round… ⎋): leaves the Note if it has focus, closes Settings if that is in front, else asks to cancel. `⌘↩` / `⌘⇧↩` are Round menu items. Settings `⌘,`. |
| Window size remembered | `WindowOptions::remember_frame("main")` (Day writes `day-windows.txt` in its config dir). |
| Menu bar | App menu (About, Settings…, Services, Hide, Quit with our confirmation), Edit (Undo…Select All via menu roles; AppKit adds AutoFill, Start Dictation and **Emoji & Symbols**), View (Full Screen, from AppKit), Round (Next, Previous, Review ⌘R, Accept Recommendation, Cancel Round… ⎋), Window, Help. Verified by reading the menu back from AppKit. |

Missing or reduced compared with egui:

- **Zoom (`⌘+`/`⌘-`)** is not ported: Day has no app-wide text scale, and scaling every label
  by hand would leave the native controls at system size. The app follows the system text
  size instead.
- **Sidebar rows are one line:** a Day nav row is title + badge, so the "5 min ago" age is
  gone (as in the wx port).
- **No tooltips** on the step bar or the disconnected warning (Day keeps tooltips in a
  separate `day-tweak-tooltip` crate; not added).
- **Light/dark/system switch** in the window is gone; the app follows the system appearance
  (`QD_APPEARANCE=light|dark` forces one, for screenshots).
- The preview wraps long lines instead of scrolling horizontally.
- History does not exist in this code base, so there was nothing to port.

Removed: `theme.rs`, `icons.rs`, `grapheme.rs`, `macos_text_input.rs`, the Inter fonts and
Lucide icons (`assets/`), and the `eframe`, `egui_extras`, `egui_commonmark` and
`unicode-segmentation` dependencies.

## Native-ness

### Wins (macOS, verified)

- Real AppKit everywhere: `NSSplitViewController` source list with the system sidebar
  material and collapse button, `NSToolbar` with an SF Symbol gear, `NSSegmentedControl`,
  `NSButton` radios/checkboxes/push buttons (a prominent default button), `NSTextView`,
  `NSTextField`/`NSSecureTextField`, `NSAlert` sheets, a separate Settings window, a real menu
  bar. Of the four ports this is the one closest to how a Mac app is built (sidebar + toolbar
  + Settings window rather than an in-window top bar and modal dialogs).
- System font, colours, accent and appearance with no palette in the code: colours are
  semantic (`SystemColor::Accent`, `Warning`, `Success`, secondary/tertiary text). The
  screenshots show a grey accent because this machine's accent colour is Graphite.
- **The egui workarounds are gone and not needed.** The Note is an `NSTextView`: the emoji
  picker, IME composition, umlauts and grapheme-aware editing are AppKit's. Verified with
  `insertText:` (as the Character Viewer delivers it): `Hallo ä 👋🏽 🧖‍♀️ Familie 👨‍👩‍👧`
  lands in the Note and the desk, ZWJ sequences render as one glyph (`shots/light-emoji.png`,
  `shots/review.png`). Emoji render in colour in labels, the sidebar and the window title.
- Markdown is native text: selectable, links open in the browser, tables are aligned label
  grids on the system card surface.
- Menu bar read back from AppKit: Edit carries Start Dictation… and Emoji & Symbols.

### Compromises

- **Raw AppKit (objc2) was needed in five places**, all in `src/ui/native.rs` behind
  `cfg(target_os = "macos")`, using Day's documented tweak hook (`.tweak(appkit(..))`,
  `NativeRef`, `with_native`):
  1. Option radios/checkboxes: a Day `button` whose `NSButton` is switched to Radio/Switch
     type; its state is driven from the desk and set again after each click, because Day
     restores its own "selected" state after a press.
  2. The Note's bezel, text background and full-width placeholder (Day draws the text area
     borderless and pins the placeholder at 320 pt, which cut the longer hints), plus a live
     placeholder (Day sets it once).
  3. No focus ring on the invisible key-catcher canvas.
  4. Red destructive buttons in the alerts (Day's `Alert` has the role but does not tell
     AppKit).
  5. **The close button**: Day's `WindowHandle::on_close_request` never fires for the primary
     window on AppKit in 0.7.10 (the delegate asks about `WINDOW_NODE`, the handler is stored
     under the window's own node id; verified by logging). The port retargets the close button
     to an `objc2::define_class!` target. A Day bug worth reporting.
- **Toolbar labels do not retitle**: Day keeps a toolbar item by id and ignores a new text,
  so the progress label's id is keyed by a hash of its text (a new text is a new item).
  Toolbar items declared on a page were also re-registered per page; they now live on the
  window root.
- **AppKit matches `⌘↩` and `⌘⇧↩` key equivalents without regard to Shift** (whichever item
  comes first fires), so "Next Question" checks `day::modifiers().shift()` itself.
- **Empty state needs a sidebar row**: a split nav always selects a row and shows nothing
  without one, so the sidebar shows "Waiting for Agents" while no round is open.
- Keys follow focus in Day; there is no window-level key handler. Digits, Return and Tab
  therefore go to a full-keyboard canvas behind the page that holds focus when the Note does
  not. Clicking empty page space does not take focus back from the Note (the scroll view gets
  the click), which is the normal Mac behaviour; `Esc` does.
- Return on the Review Page triggers the prominent Submit button (Mac default-button idiom);
  egui needed ⌘↩ there. With Unanswered Questions it still asks first.

### Per platform

- **macOS (verified on this machine, Darwin 27, Xcode 27.2):** as above. A plain `cargo
  build` gives an unbundled binary; `day build -t macos-appkit` would give a signed-ready
  `.app` via Xcode (not set up: it needs a `Day.toml` and the `platform/macos` runner).
- **Windows (expected, not built):** WinUI 3 through Day's `winui` feature: `NavigationView`
  sidebar, `ContentDialog` alerts, `TextBox`. Declared tier 2 ("demi-supported"), runtime pass
  pending per Day's own docs; needs the Windows App SDK. Options fall back to Day's `toggle`
  (a switch) beside a label, since the AppKit tweaks are macOS-only; no bezel/placeholder
  fixes, no red destructive button. The close-button fix is macOS-only, so closing with open
  rounds depends on Day's `on_close_request` working there.
- **Linux (expected, not built):** GTK 4 / libadwaita (`AdwAlertDialog`, `GtkPaned`,
  `GtkListBox` sidebar), tier 2; Qt 6 is the alternative backend. Same Option fallback.
  GTK 4 has no urgency hint, so window attention is a no-op there.

## Build time and size (macOS, Apple Silicon, release)

`cargo build --release --bin question-desk` from an empty target directory, same machine:

| | egui (original) | Day (this port) |
|---|---|---|
| Clean release build, wall | ~30 s (33 s measured by the wx port) | 29 s |
| Clean release build, CPU (user+sys) | 368 s | 278 s |
| Incremental rebuild after touching a UI file | — | 5 s |
| Binary size | 27.5 MB | 17.9 MB |
| Binary size, stripped | 21.4 MB | 12.6 MB |
| Crates in `Cargo.lock` | 598 | 407 |

No C/C++ build, no CMake, no Swift for the plain cargo path; AppKit is the system's. The
binary carries Day's runtime for one backend plus its dayscript/automation engine (Day
starts it in every app).

## Dependencies and licensing

- `day-app`, `day-core`, `day-spec`, `day-toolkit-appkit` (and Day's other crates): **MPL-2.0**.
  File-level copyleft: modified Day source files must stay MPL and be offered in source;
  linking them into a closed or differently licensed app is fine, and static linking is
  explicitly allowed. Unmodified use, as here, imposes only notice obligations.
- `objc2`, `objc2-app-kit`, `objc2-foundation`: MIT (macOS only).
- `pulldown-cmark`: MIT (already a dependency; now also finds tables).
- Dropped: Inter (OFL) and Lucide icons (ISC).
- System libraries at run time: none extra on macOS; Windows App SDK runtime on Windows;
  GTK 4 + libadwaita (or Qt 6) on Linux.
- Build requirements: just Rust ≥ 1.100 (Day's MSRV) for the cargo path; Xcode for `day
  build`'s `.app`.

## Risks

- **Maturity and API churn (the big one).** Day is about four months old with seven
  breaking releases; the crates.io release (0.7.10) is already ahead of the fork's checkout
  (0.7.6), and its docs and API differ in places (e.g. the `markdown` piece is new in the
  release). The port pins `~0.7.10`. Expect each 0.x bump to break something; this port
  touches ~40 Day APIs.
- **Bugs found while porting** (all worked around, see Compromises): primary-window close
  request ignored on AppKit, toolbar labels not retitled, button state restored over a
  native type change, text-area placeholder width fixed at 320 pt, destructive role not
  passed to `NSAlert`. Each workaround depends on a Day internal (the native class behind a
  piece), which Day's docs explicitly say is not API.
- **Per-backend parity.** macOS is Day's tier-1 target; Windows and Linux are tier 2 and were
  not built here. Native tweaks are macOS-only, so the other platforms get the plainer
  fallbacks.
- **Single-maintainer, young project**, a large framework surface (13 targets, CLI,
  packaging, store submission); bus factor and long-term direction are unknowns.
- **Threading** is clean: one `on_main` post per burst of desk events, coalesced with an
  atomic flag; no nested event loops (alerts are async sheets).
- **Screenshots in the sandbox.** `screencapture` is denied here, and Day's own
  `window_image()` asks the window server first, which the sandbox answers with a **blank
  white image**. `tools/screenshot.sh` therefore builds with the `snapshot` feature and, when
  `screencapture` fails, asks the app to render **its own frontmost window only** with AppKit
  `cacheDisplayInRect:` (never the screen). Those images show the window inactive (grey
  selection, unhighlighted default button). The same feature drives the window for the shots
  (`QD_SNAP_COMMANDS`: synthetic key events, `insertText:`, clicks, menu dump). Off in normal
  builds.
- **Keyboard testing** used synthetic AppKit events in a window that was not key (activating
  the app would take the user's focus); real typing in an active window was not testable.

## Rating (macOS)

- **Native look: 9/10.** The structure is a Mac app's: source-list sidebar with sections and
  badges, unified toolbar with title and SF Symbol, segmented step control, radio buttons and
  checkboxes, red destructive alert buttons, a real Settings window, system colours and
  fonts throughout. Points off for the "Waiting for Agents" placeholder row, the hand-made
  Recommended capsule and key hints, and the card look of the review page, which is more iOS
  than Mac.
- **Feel: 7/10.** Text input, IME, emoji, menus, sheets, sidebar selection and window
  behaviour are AppKit's and feel right. Points off for things that took workarounds and
  could break on a Day update (state flicker risk on radio/checkbox clicks, the key-catcher
  canvas, Shift handling for ⌘⇧↩), the missing zoom and tooltips, and Return submitting on
  the Review Page.

## Screenshots

In `shots/` (own-window renders, see Risks; dark is the system appearance here, light forced
with `QD_APPEARANCE=light`):

- `all.png`, `all-light.png`: `screenshot.sh … all` (five rounds from five sources).
- `grilling-emoji.png`, `grilling-emoji-light.png`: `screenshot.sh … grilling emoji`.
- `light-emoji.png`: radio selected with `1`, Note with umlaut, skin-tone and ZWJ emoji typed
  after `Tab`, left with `Esc`.
- `previews.png`: Options with a preview panel beside them, second Option hovered.
- `review.png`: Review Page after digit, Note, `⌘↩` and `↩` (accept Recommendation).
- `submit-unanswered.png`, `cancel.png`, `close.png`: the `NSAlert` sheets.
- `settings.png`: the Settings window. `empty.png`: empty state with the MCP URL.

## How to run

```sh
XDG_CONFIG_HOME=$PWD/.xdg cargo run --release          # port 7464 from .xdg/question-desk/config.toml
cargo run --release --bin demo-client -- --url http://127.0.0.1:7464/mcp all
tools/screenshot.sh -o shots/all.png all               # QD_APPEARANCE=light|dark, QD_SNAP_COMMANDS=…
```

Set `DAY_DATA_DIR` to keep Day's remembered window frame somewhere other than
`~/Library/Application Support/question-desk/` (the screenshot script uses `.xdg/day`).
