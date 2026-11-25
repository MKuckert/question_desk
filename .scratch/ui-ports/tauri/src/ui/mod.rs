//! The Question Desk window: a Tauri webview (`ui/` holds its HTML, CSS and JS) over the
//! `desk` read model.
//!
//! The frontend fetches the whole [`view::DeskState`] with the `state` command and re-renders;
//! it re-fetches after each intent it sends and whenever Rust emits `desk-changed` (Agent-side
//! changes, dialog outcomes, the age ticking). Confirmations are native message dialogs shown
//! from Rust, so the destructive intents never depend on the frontend asking first.
//!
//! The MCP server keeps its own tokio runtime inside [`ServerControl`]; Tauri has its own.
//! `ServerControl::restart` uses `block_on`, so it is only called from threads outside any
//! runtime: `run` before Tauri starts, and the synchronous `save_settings` command, which
//! Tauri runs on the main thread.

mod markdown;
mod view;

use std::{
    net::IpAddr,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, State, Theme, UserAttentionType,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::{
    config::{self, Config},
    desk::{Desk, DeskEvent},
    server::ServerControl,
};

const MAIN: &str = "main";
/// Rust → frontend: re-fetch the state.
const CHANGED: &str = "desk-changed";
/// Rust → frontend: a menu item the frontend handles (payload: its id).
const MENU: &str = "menu";

struct App {
    desk: Desk,
    server: Mutex<ServerControl>,
    /// The settings the server runs with.
    config: Mutex<Config>,
    /// Shown in the empty state: the MCP URL or why the server is not running.
    status: Mutex<String>,
    /// The user confirmed closing; let the next close request through.
    closing: AtomicBool,
    /// A confirmation dialog is showing; a second one would stack.
    dialog_open: AtomicBool,
    window: Mutex<WindowState>,
}

/// Window size, position and zoom, kept next to `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct WindowState {
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
    maximized: bool,
    zoom: f64,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1100.0,
            height: 800.0,
            x: None,
            y: None,
            maximized: false,
            zoom: 1.0,
        }
    }
}

impl WindowState {
    fn path() -> Option<PathBuf> {
        Some(config::path()?.parent()?.join("window-state.json"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = Self::path() else { return };
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, serde_json::to_vec_pretty(self)?));
        if let Err(e) = result {
            log::warn!("cannot save the window state to {}: {e}", path.display());
        }
    }

    /// Takes size, position and maximized state from the live window.
    fn capture(&mut self, window: &tauri::Window) {
        let Ok(scale) = window.scale_factor() else {
            return;
        };
        self.maximized = window.is_maximized().unwrap_or(false);
        if self.maximized {
            return;
        }
        if let Ok(size) = window.inner_size() {
            let size = size.to_logical::<f64>(scale);
            (self.width, self.height) = (size.width, size.height);
        }
        if let Ok(position) = window.outer_position() {
            let position = position.to_logical::<f64>(scale);
            (self.x, self.y) = (Some(position.x), Some(position.y));
        }
    }
}

/// Starts the MCP server with `config`, then the window, and blocks until it is closed. If
/// the config could not be loaded, the server stays off until the user saves the settings.
pub fn run(desk: Desk, mut server: ServerControl, config: Result<Config, String>) {
    let (config, status) = match config {
        Ok(config) => {
            let status = server_status(server.restart(config.server_settings()));
            (config, status)
        }
        Err(e) => {
            log::error!("{e}");
            (
                Config::default(),
                format!("The settings could not be read: {e}"),
            )
        }
    };
    let app_state = App {
        desk: desk.clone(),
        server: Mutex::new(server),
        config: Mutex::new(config),
        status: Mutex::new(status),
        closing: AtomicBool::new(false),
        dialog_open: AtomicBool::new(false),
        window: Mutex::new(WindowState::load()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .menu(build_menu)
        .on_menu_event(on_menu_event)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            state,
            select,
            toggle_option,
            set_note,
            next,
            back,
            go_to,
            review,
            accept_recommendation,
            request_cancel,
            request_submit,
            settings,
            settings_warning,
            save_settings,
            open_link,
            frontend_log,
        ])
        .setup(move |app| {
            let window = create_window(app.handle())?;
            listen_to_desk(&desk, app.handle().clone());
            tick(app.handle().clone());
            window.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the Tauri app runs");
}

fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let saved = app.state::<App>().window.lock().expect("unpoisoned").clone();
    let mut builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Question Desk")
        .inner_size(saved.width, saved.height)
        .min_inner_size(520.0, 400.0)
        .maximized(saved.maximized)
        .visible(false)
        // Only the app's own pages; links go to the system browser via `open_link`.
        .on_navigation(|url| {
            url.scheme() == "tauri"
                || (url.scheme() == "http" || url.scheme() == "https")
                    && url.host_str() == Some("tauri.localhost")
        });
    if let (Some(x), Some(y)) = (saved.x, saved.y)
        && on_some_monitor(app, x, y)
    {
        builder = builder.position(x, y);
    }
    match std::env::var("QUESTION_DESK_THEME").as_deref() {
        Ok("dark") => builder = builder.theme(Some(Theme::Dark)),
        Ok("light") => builder = builder.theme(Some(Theme::Light)),
        _ => {}
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::{
            LogicalPosition, TitleBarStyle,
            window::{Effect, EffectState, EffectsBuilder},
        };
        // A unified toolbar: the sidebar runs under the traffic lights, as in Mail or Finder,
        // and its translucent material shows through the transparent page background.
        builder = builder
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(LogicalPosition::new(20.0, 22.0))
            .transparent(true)
            .effects(
                EffectsBuilder::new()
                    .effect(Effect::Sidebar)
                    .state(EffectState::FollowsWindowActiveState)
                    .build(),
            );
    }
    #[cfg(target_os = "windows")]
    {
        use tauri::window::{Effect, EffectsBuilder};
        // Mica behind the sidebar on Windows 11; Windows 10 ignores the effect.
        builder = builder
            .transparent(true)
            .effects(EffectsBuilder::new().effect(Effect::Mica).build());
    }
    let window = builder.build()?;
    // Readable by screenshot tools (and screen sharing); tao leaves macOS windows unshared.
    window.set_content_protected(false)?;
    if (saved.zoom - 1.0).abs() > f64::EPSILON {
        window.set_zoom(saved.zoom)?;
    }
    Ok(window)
}

fn on_some_monitor(app: &AppHandle, x: f64, y: f64) -> bool {
    app.available_monitors().is_ok_and(|monitors| {
        monitors.iter().any(|m| {
            let scale = m.scale_factor();
            let pos = m.position().to_logical::<f64>(scale);
            let size = m.size().to_logical::<f64>(scale);
            x >= pos.x - 50.0
                && y >= pos.y - 50.0
                && x < pos.x + size.width - 50.0
                && y < pos.y + size.height - 50.0
        })
    })
}

/// Agent-side changes re-render the window; a new round also asks for attention.
fn listen_to_desk(desk: &Desk, app: AppHandle) {
    desk.on_change(move |event| {
        let _ = app.emit(CHANGED, ());
        if event == DeskEvent::RoundOpened {
            request_attention(&app);
        }
    });
}

/// Brings the window to the front and asks for attention (Dock bounce, taskbar flash)
/// without taking keyboard focus from the app the user is in.
fn request_attention(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let _ = window.set_always_on_top(false);
    });
}

/// Keeps the "5 min ago" texts fresh.
fn tick(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let _ = app.emit(CHANGED, ());
        }
    });
}

// ---- Menu ---------------------------------------------------------------------------------

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str, accelerator: Option<&str>| {
        let mut builder = MenuItemBuilder::with_id(id, text);
        if let Some(accelerator) = accelerator {
            builder = builder.accelerator(accelerator);
        }
        builder.build(app)
    };
    let settings = item("settings", "Settings…", Some("CmdOrCtrl+,"))?;
    let quit = item("quit", "Quit Question Desk", Some("CmdOrCtrl+Q"))?;

    // Standard Edit items; on macOS, AppKit adds "Emoji & Symbols" and dictation to a menu
    // with these, which the egui build had to bridge by hand.
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let appearance = SubmenuBuilder::new(app, "Appearance")
        .item(&item("theme-system", "Use System Setting", None)?)
        .item(&item("theme-light", "Light", None)?)
        .item(&item("theme-dark", "Dark", None)?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item("toggle-sidebar", "Toggle Sidebar", Some("Ctrl+CmdOrCtrl+S"))?)
        .separator()
        .item(&item("zoom-reset", "Actual Size", Some("CmdOrCtrl+0"))?)
        .item(&item("zoom-in", "Zoom In", Some("CmdOrCtrl+="))?)
        .item(&item("zoom-out", "Zoom Out", Some("CmdOrCtrl+-"))?)
        .separator()
        .item(&appearance)
        .build()?;
    // The round's actions, for discoverability; the frontend also handles these keys and
    // ignores a menu echo of a key it already handled.
    let round = SubmenuBuilder::new(app, "Round")
        .item(&item("next", "Next Question", Some("CmdOrCtrl+Enter"))?)
        .item(&item("back", "Previous Question", Some("CmdOrCtrl+Shift+Enter"))?)
        .item(&item("review", "Review Answers", None)?)
        .separator()
        .item(&item("submit", "Submit Round", None)?)
        .item(&item("cancel", "Cancel Round…", None)?)
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window();
    #[cfg(target_os = "macos")]
    let window = window.separator().bring_all_to_front();
    let window = window.build()?;

    let menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    let menu = menu.item(
        &SubmenuBuilder::new(app, "Question Desk")
            .about(None)
            .separator()
            .item(&settings)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .item(&quit)
            .build()?,
    );
    #[cfg(not(target_os = "macos"))]
    let menu = menu.item(
        &SubmenuBuilder::new(app, "File")
            .item(&settings)
            .separator()
            .item(&quit)
            .build()?,
    );
    menu.item(&edit)
        .item(&view)
        .item(&round)
        .item(&window)
        .build()
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let zoom = |f: &dyn Fn(f64) -> f64| {
        let state = app.state::<App>();
        let mut saved = state.window.lock().expect("unpoisoned");
        saved.zoom = f(saved.zoom).clamp(0.5, 3.0);
        let _ = window.set_zoom(saved.zoom);
        saved.save();
    };
    match event.id().as_ref() {
        "quit" => {
            let _ = window.close();
        }
        "zoom-in" => zoom(&|z| z + 0.1),
        "zoom-out" => zoom(&|z| z - 0.1),
        "zoom-reset" => zoom(&|_| 1.0),
        "theme-system" => {
            let _ = window.set_theme(None);
        }
        "theme-light" => {
            let _ = window.set_theme(Some(Theme::Light));
        }
        "theme-dark" => {
            let _ = window.set_theme(Some(Theme::Dark));
        }
        id => {
            let _ = window.emit(MENU, id);
        }
    }
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let state = window.state::<App>();
    {
        let mut saved = state.window.lock().expect("unpoisoned");
        saved.capture(window);
        saved.save();
    }
    if state.closing.load(Ordering::SeqCst) || state.desk.sessions().is_empty() {
        return;
    }
    api.prevent_close();
    let Some(webview) = window.get_webview_window(MAIN) else {
        return;
    };
    let desk = state.desk.clone();
    let app = window.app_handle().clone();
    confirm(
        &webview,
        "Close the Question Desk?",
        "All open rounds are cancelled; their Agents receive the answers given so far.",
        "Cancel Rounds and Quit",
        MessageDialogKind::Warning,
        move || {
            desk.cancel_all();
            app.state::<App>().closing.store(true, Ordering::SeqCst);
            // Give the server a moment to write the cancelled results to the waiting calls.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                if let Some(window) = app.get_webview_window(MAIN) {
                    let _ = window.close();
                }
            });
        },
    );
}

/// A native message dialog, as a sheet on macOS. `on_confirm` runs if the user chooses
/// `confirm_label`; "Keep Answering" (also Esc) dismisses it.
fn confirm(
    window: &WebviewWindow,
    title: &str,
    text: &str,
    confirm_label: &str,
    kind: MessageDialogKind,
    on_confirm: impl FnOnce() + Send + 'static,
) {
    let app = window.app_handle().clone();
    if app.state::<App>().dialog_open.swap(true, Ordering::SeqCst) {
        return;
    }
    app.dialog()
        .message(text)
        .title(title)
        .kind(kind)
        .parent(window)
        .buttons(MessageDialogButtons::OkCancelCustom(
            confirm_label.to_owned(),
            "Keep Answering".to_owned(),
        ))
        .show(move |confirmed| {
            app.state::<App>().dialog_open.store(false, Ordering::SeqCst);
            if confirmed {
                on_confirm();
            }
            let _ = app.emit(CHANGED, ());
        });
}

// ---- Commands -----------------------------------------------------------------------------
//
// All synchronous: Tauri runs them on the main thread, in the order the frontend sent them.

#[tauri::command]
fn state(app: State<'_, App>) -> view::DeskState {
    let status = app.status.lock().expect("unpoisoned").clone();
    view::desk_state(status, app.desk.sessions(), app.desk.current())
}

#[tauri::command]
fn select(app: State<'_, App>, ticket: String) {
    app.desk.select(&ticket);
}

#[tauri::command]
fn toggle_option(app: State<'_, App>, index: usize) {
    app.desk.toggle_option(index);
}

#[tauri::command]
fn set_note(app: State<'_, App>, note: String) {
    app.desk.set_note(&note);
}

#[tauri::command]
fn next(app: State<'_, App>) {
    app.desk.next();
}

#[tauri::command]
fn back(app: State<'_, App>) {
    app.desk.back();
}

#[tauri::command]
fn go_to(app: State<'_, App>, index: usize) {
    app.desk.go_to(index);
}

#[tauri::command]
fn review(app: State<'_, App>) {
    app.desk.review();
}

#[tauri::command]
fn accept_recommendation(app: State<'_, App>) {
    app.desk.accept_recommendation();
}

#[tauri::command]
fn request_cancel(app: State<'_, App>, window: WebviewWindow) {
    if app.desk.current().is_none() {
        return;
    }
    let desk = app.desk.clone();
    confirm(
        &window,
        "Cancel this round?",
        "The Agent receives the answers you gave so far, marked as cancelled.",
        "Cancel Round",
        MessageDialogKind::Warning,
        move || desk.cancel(),
    );
}

#[tauri::command]
fn request_submit(app: State<'_, App>, window: WebviewWindow) {
    let Some(round) = app.desk.current() else {
        return;
    };
    let unanswered = round.unanswered().len();
    if unanswered == 0 {
        app.desk.submit();
        return;
    }
    let desk = app.desk.clone();
    confirm(
        &window,
        "Submit with unanswered questions?",
        &format!("{unanswered} question(s) have no answer. The Agent is told which ones."),
        "Submit Anyway",
        MessageDialogKind::Info,
        move || desk.submit(),
    );
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsForm {
    host: String,
    port: String,
    token: String,
    /// Where they are saved, with the home directory as `~`.
    path: Option<String>,
    default_host: &'static str,
    default_port: u16,
}

#[tauri::command]
fn settings(app: State<'_, App>) -> SettingsForm {
    let config = app.config.lock().expect("unpoisoned").clone();
    SettingsForm {
        host: config.host,
        port: config.port.to_string(),
        token: config.bearer_token.unwrap_or_default(),
        path: config::path().map(|p| home_relative(&p)),
        default_host: crate::server::DEFAULT_HOST,
        default_port: crate::server::DEFAULT_PORT,
    }
}

/// The warning under the host field, if the address is reachable from other machines.
#[tauri::command]
fn settings_warning(host: String, token: String) -> Option<String> {
    if is_loopback(&host) {
        return None;
    }
    let mut warning = "Other machines can reach this address.".to_owned();
    if token.trim().is_empty() {
        warning.push_str(" Set an access token.");
    }
    Some(warning)
}

/// Restarts the server with the form's settings and writes them to the config file. An
/// error says why the dialog must stay open.
#[tauri::command]
fn save_settings(
    app: State<'_, App>,
    handle: AppHandle,
    host: String,
    port: String,
    token: String,
) -> Result<(), String> {
    let port = port
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|&p| p != 0)
        .ok_or("The port must be a number between 1 and 65535.")?;
    let token = token.trim();
    let config = Config {
        host: host.trim().to_owned(),
        port,
        bearer_token: (!token.is_empty()).then(|| token.to_owned()),
    };
    config::validate(&config)?;
    let mut server = app.server.lock().expect("unpoisoned");
    let mut current = app.config.lock().expect("unpoisoned");
    if config != *current || !server.is_running() {
        let result = server.restart(config.server_settings());
        let running = server.is_running();
        drop(server);
        if let Err(e) = result {
            if !running {
                *app.status.lock().expect("unpoisoned") = server_status(Err(e.clone()));
                let _ = handle.emit(CHANGED, ());
            }
            return Err(e);
        }
        *app.status.lock().expect("unpoisoned") = server_status(result);
        *current = config.clone();
        let _ = handle.emit(CHANGED, ());
    }
    match config::save(&config) {
        Ok(path) => {
            log::info!("saved settings to {}", path.display());
            Ok(())
        }
        Err(e) => Err(format!(
            "The server runs with these settings, but saving failed: {e}"
        )),
    }
}

/// Opens a link from a Question's Markdown in the system browser.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    let allowed = ["https://", "http://", "mailto:"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return Err(format!("not opening {url:?}"));
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Frontend errors and milestones end up in the app's log, next to the server's.
#[tauri::command]
fn frontend_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!(target: "frontend", "{message}"),
        _ => log::info!(target: "frontend", "{message}"),
    }
}

// ---- Helpers ------------------------------------------------------------------------------

/// The empty state's line for the outcome of starting the server.
fn server_status(started: Result<String, String>) -> String {
    match started {
        Ok(url) => {
            log::info!("listening on {url}");
            format!("Agents connect to {url}")
        }
        Err(e) => {
            log::error!("{e}");
            format!("The MCP server did not start: {e}")
        }
    }
}

/// Whether `host` is reachable from this machine only.
fn is_loopback(host: &str) -> bool {
    let host = host.trim();
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `path` with the home directory shown as `~`.
fn home_relative(path: &std::path::Path) -> String {
    directories::BaseDirs::new()
        .and_then(|d| {
            path.strip_prefix(d.home_dir())
                .ok()
                .map(|p| format!("~/{}", p.display()))
        })
        .unwrap_or_else(|| path.display().to_string())
}
