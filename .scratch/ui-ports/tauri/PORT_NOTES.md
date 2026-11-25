# Tauri v2 port: notes

The egui UI (`src/ui/mod.rs`, `theme.rs`, `grapheme.rs`, `macos_text_input.rs`, `icons.rs`, the Inter fonts) is replaced by a Tauri 2.12 window. The page is plain HTML/CSS/JS in `ui/`, with no framework, no npm and no build step; `tauri::generate_context!` embeds it in the binary. `desk.rs`, `server.rs`, `format.rs`, `schema.rs`, `config.rs` and `tests/` are unchanged.

## Architecture

- `src/ui/mod.rs`: the Tauri builder, window, menu, native dialogs, window state, and the commands. Every command is synchronous, so Tauri runs them on the main thread in the order the page sent them.
- `src/ui/view.rs`: serialisable mirrors of the desk views (`DeskState`, `Round`, `Question`, …). **`desk.rs` did not get `Serialize`.** The mirror adds the rendered Markdown, the age text and an `acceptsRecommendation` flag.
- `src/ui/markdown.rs`: Markdown is rendered with `pulldown-cmark` and sanitised with `ammonia`, using an allow-list (see Security below).
- `ui/app.js` (~600 lines): it fetches `state`, builds fresh DOM, and morphs it into the live tree with a small keyed morph, so focus, the caret, IME composition, selection and scroll positions survive re-renders. Events are delegated through `data-action` attributes. The page re-fetches after every intent, and on the `desk-changed` event Rust emits from `Desk::on_change`, from dialog outcomes and from a 30 s tick for the "5 min ago" texts.
- **Runtimes.** The MCP server keeps its own tokio runtime inside `ServerControl`. `restart()` uses `block_on`, so it is called only from threads outside any runtime: from `ui::run` before Tauri starts, and from the synchronous `save_settings` command on the main thread. It is never called from Tauri's async runtime, and they don't clash.

## What was ported

| Feature | Status |
|---|---|
| Sidebar: Agent Sessions, open rounds, answered/total, age, "⚠ disconnected", "New" badge, open count | done; resizable by dragging the edge, can be hidden (toolbar button, View → Toggle Sidebar ⌃⌘S) |
| History | not in this code base (only in GLOSSARY), so nothing to port |
| Top bar | unified macOS toolbar: round title and session as title/subtitle, "Agent disconnected" with tooltip, the header chip, "n / N", Settings gear |
| Step bar | done; tooltips are native `title`s; current, answered and unanswered states |
| Question page | Markdown body (scrolls at 35 vh), Recommendation text with chip, Option cards with real `<input type=radio>` / `checkbox`, key-cap digits, "Recommended" badge, preview panel (two columns at ≥720 px of content width via a container query, otherwise below the cards and only when there is a preview), Note `<textarea>` with the same three placeholder texts |
| Review Page | done: Unanswered highlighted in orange, click to edit, Submit |
| Confirmations | Cancel / Submit-with-Unanswered / Close are **native** message dialogs (sheets on macOS, via `tauri-plugin-dialog`), shown from Rust |
| Settings | in-page `<dialog>` styled as a sheet: host, port, token with "Show token", the loopback warning, error text, "Saved in ~/…"; saving restarts the server as before |
| Empty state | inbox icon, "No open questions", the MCP URL line (selectable, so it can be copied) |
| Window attention | on `RoundOpened`: unminimise, raise above other windows for 300 ms, `request_user_attention(Informational)` (Dock bounce or taskbar flash) without taking focus |
| Keyboard | `1`–`9`, `Tab` → Note, `Esc` leaves the Note or cancels, `Enter` accepts the Recommendation, `⌘↩` next/Submit, `⌘⇧↩` back, `⌘,` Settings. IME-safe: keys during composition are ignored, and the Note is sent only once the composition is committed |
| Menu bar | macOS: Question Desk (About, Settings…, Services, Hide, Quit), Edit (gives **Emoji & Symbols** and dictation for free), View (sidebar, zoom, Appearance: System/Light/Dark), Round (Next, Previous, Review, Submit, Cancel…), Window. On Windows/Linux, a File menu replaces the app menu |
| Zoom and window size | `⌘+`, `⌘-`, `⌘0`; zoom, size, position and maximised state are kept in `window-state.json` next to `config.toml`, so they follow `XDG_CONFIG_HOME`. I wrote this instead of using `tauri-plugin-window-state`, which writes to `~/Library/Application Support/<identifier>` and ignores the sandboxed config dir |
| Close / Quit | ⌘Q and ⌘W go through `CloseRequested`; with open rounds a native sheet asks first, then `cancel_all` runs and the window closes after a 300 ms grace, so the cancelled results reach the waiting calls |

**Missing or weaker than egui**
- The "Recommended" chip is no longer shown on hover; there are no hover effects at all (deliberately).
- The light/dark switcher is in View → Appearance rather than in the top bar. `QUESTION_DESK_THEME=light|dark` forces it at start.
- The settings sheet is in-page HTML, not a separate native Settings window.
- With the native NSAlert, **Return confirms "Cancel Round" and "Cancel Rounds and Quit"**. The egui dialog required a click for destructive actions. Esc still means "Keep Answering". `rfd` can't move the default button; fixing this needs a custom sheet or an in-page dialog for the two destructive confirmations.

## Native-ness

**macOS (verified that it builds, launches, renders and runs the frontend without errors; the offscreen renders below are from the real WKWebView engine)**
- Wins: the system font (`-apple-system-body`) and real system colours (`-apple-system-label`, `-secondary-label`, `-separator`, `-control-accent`, `-orange`, `-green`), following the user's accent and appearance. Native push buttons and the blue **default button** (`-webkit-appearance: default-button`), native radios, checkboxes, text fields and textarea with `accent-color`, and the native focus ring only on keyboard focus. Text input is AppKit's: IME with candidate window placement, the emoji picker (Edit menu and ⌃⌘Space), spellcheck with the native context menu (suggestions, Look Up), dictation, and correct grapheme editing for ZWJ, flag and skin-tone emoji. The `grapheme.rs` and `macos_text_input.rs` workarounds are gone. Also: a unified toolbar with the sidebar running under the traffic lights, a translucent `Sidebar` vibrancy material that dims when the window is inactive, native sheets for confirmations, native tooltips, overlay scrollbars and elastic scrolling in the panes, and VoiceOver through real form controls, `radiogroup`, `aria-current` and labels.
- Webview tells I suppressed: text selection on chrome (content stays selectable), the default context menu outside text, rubber-banding of the window itself, focus rings on click, the pointer cursor and hover effects, and navigation away from the app (`on_navigation` allow-list; links open in the system browser through `open_link`).
- Compromises: icons are Lucide SVGs, not SF Symbols. `macos-private-api` is enabled for the transparent window, which is not allowed on the Mac App Store. The toolbar is drawn by the page under a hidden title bar, not as an AppKit `NSToolbar`, so it has no customisation or overflow menu. Option cards are a web idiom; the closest AppKit equivalent is a grouped form. Under nono, WebKit can't write its data store, so `localStorage` (sidebar width and visibility) is not kept between launches.

**Windows (expected, not verified)**: WebView2 (Chromium). Segoe UI Variable via `system-ui`, Fluent-styled form controls, `AccentColor`. The window is transparent with **Mica**; the sidebar sits on a 45 % Canvas tint. **Risk:** on Windows 10, which has no Mica, the transparent area may show black or unblurred; test it, and drop `transparent(true)` there if needed. The menu bar sits inside the window. The settings buttons follow Windows order (Save first). WebView2 must be installed; it ships with Windows 11. No `icons/icon.ico` exists yet, and tauri-build needs one on Windows (`cargo tauri icon icons/icon.png`).

**Linux (expected, not verified)**: WebKitGTK 4.1. No window effects; the sidebar gets a flat Canvas/CanvasText mix. Caveats: WebKitGTK must be ≥ 2.40 for container queries and `color-mix()`; older distros lay out the preview panel in one column. `AccentColor` is often missing there, which falls back to `Highlight`. IME goes through IBus/Fcitx and works, but candidate window placement varies. Emoji need a colour emoji font (Noto Color Emoji). WebKitGTK has known DMA-BUF and compositing issues on NVIDIA (`WEBKIT_DISABLE_DMABUF_RENDERER=1`). GTK form controls are styled by Adwaita, not the GTK theme.

## Build, size, memory (this Mac, release, measured in the sandbox)

| | egui (original, built in /tmp) | Tauri |
|---|---|---|
| clean `cargo build --release --bins` (wall) | 30.3 s | 26.6 s |
| incremental release after touching `ui/mod.rs` | 5.0 s | 8.5 s |
| binary size (`question-desk`) | 27.5 MB | 21.2 MB |
| packages in Cargo.lock | 598 | 572 |
| memory | not measured | not measured |

Memory could not be measured: `ps`, `top` and `footprint` are blocked in the sandbox. Expect the Tauri build to use more in total, because WKWebView runs separate WebContent and Networking processes (typically 30–80 MB extra), while egui needs a GPU context. Measure with Activity Monitor outside the sandbox.

## Security

- **CSP** (in `tauri.conf.json`; Tauri adds hashes for its own scripts): `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'`. No inline scripts and no `unsafe-inline` styles. `freezePrototype: true`.
- **Markdown** from Agents is untrusted. `ammonia` uses an explicit tag allow-list with no `img`, `iframe`, `style`, `script` or forms, and no generic attributes such as `style`, `on*` or `class`. URL schemes are limited to http, https and mailto, and links get `rel="noopener noreferrer"`. Unit tests cover script, handler, image, `javascript:`, iframe and style stripping. The only `innerHTML` sinks are that sanitised HTML and the static icon strings. Option labels, descriptions, previews and Notes go through `textContent`.
- **Navigation:** the webview may only load the app's own origin. `open_link` passes only http, https and mailto to the OS.
- **IPC surface:** app commands (desk intents, settings, `open_link`, `frontend_log`). The capability (`capabilities/default.json`) grants the page only event listen/unlisten and window dragging; no plugin commands (dialog, opener, fs, shell) are exposed to JS. The dialog and opener plugins are called from Rust only.
- **New dependencies:** `tauri` 2.12 (with wry, tao, muda), `tauri-plugin-dialog` (rfd), `tauri-plugin-opener`, `ammonia` (html5ever). Removed: `eframe`, `egui_extras`, `egui_commonmark`, `objc2*`, `unicode-segmentation`.

## Risks

- Return confirms the destructive native dialogs (see above).
- Menu accelerators against page keys: the page handles ⌘↩ / ⌘⇧↩ itself and ignores the menu echo within 250 ms. On macOS WKWebView gives the page the key first, so normally only one fires. This is not verified interactively.
- Quit from the Dock or at logout goes through `terminate:` and might bypass the close confirmation. The menu's own Quit is wired to the confirmation.
- Transparency and Mica on Windows 10 (see above). `macos-private-api` rules out the App Store.
- The window state is saved on close only, so a killed process keeps the previous size.

## Verification and screenshots

- `cargo build`, `cargo build --release`, `cargo test` (all `tests/` pass untouched, plus 4 new Markdown tests and 1 ignored state dump) and `cargo clippy --all-targets` (clean) all pass.
- The app launches in the sandbox, serves MCP on 7462, takes the demo rounds, and the page renders with no JS errors. Frontend errors are forwarded to the log by `frontend_log`.
- **Window screenshots are blocked.** `tools/screenshot.sh` is adapted (port 7462, `XDG_CONFIG_HOME=.xdg`, finds *this* binary's window by PID, `QUESTION_DESK_THEME`, `QUESTION_DESK_BIN`). In this session `screencapture -l` fails with "could not create image from window" for the original egui binary too, so it is a capture permission issue in this session, not the port. Run `tools/screenshot.sh -o shots/all.png all` and `… -o shots/grilling-emoji.png grilling emoji` outside the sandbox.
- **Offscreen renders instead** (`shots/offscreen/`): `tools/render-offscreen.swift` loads `ui/` into an off-screen WKWebView with a stubbed Tauri bridge and real `DeskState` JSON from `QD_DUMP_DIR=/tmp/qd-states cargo test --lib dump_states -- --ignored`. It doesn't read the screen. The native parts are not in these pictures: the traffic lights, the vibrancy (replaced by a flat grey) and the sheets. The controls draw in their inactive-window state, so the checked radio and checkbox show grey, not accent. Files: `question-light/dark`, `preview-light`, `preview-narrow-light`, `multi-light/dark` (emoji, ZWJ, flags, umlauts), `review-light/dark`, `empty-light`, `settings-light`.

## Rating (macOS)

- **Native look: 7/10.** The system font, the system palette following accent and appearance, real AppKit-drawn buttons, default button, radios, checkboxes and text fields, a unified toolbar with vibrancy, and native sheets and menus. Points lost for the Lucide icons instead of SF Symbols, web-style cards and step pills that no AppKit app has, and the HTML settings sheet.
- **Feel: 8/10.** Text input is the real thing: IME, emoji picker, spellcheck, dictation, grapheme editing, Look Up and Services. Scrolling, focus and keyboard behaviour are native, and VoiceOver gets real controls. Points lost for the Return-confirms-destructive dialogs, the 250 ms debounce hack between menu and page keys, a round-trip render per keystroke (not noticeable here, but it is a webview), and startup showing an empty frame for about 100 ms before the first render.
