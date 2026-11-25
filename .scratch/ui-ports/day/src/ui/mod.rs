//! The Question Desk window on [Day](https://daybrite.dev): the `desk` read model rendered with
//! the platform's own controls (AppKit on macOS, WinUI 3 on Windows, GTK 4 on Linux), and the
//! user's intents dispatched back to it.
//!
//! Threading: Day's signals live on the main thread. The desk's `on_change` fires on the
//! server's tokio threads, so it only posts a refresh to the main loop (`on_main`); everything
//! the window shows is derived from one revision counter that the refresh bumps.

mod markdown;
mod native;
mod page;
mod settings;
#[cfg(feature = "snapshot")]
mod snapshot;

use std::{
    cell::RefCell,
    net::IpAddr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use day::prelude::*;
use day::reactive::{Memo, Signal, bind, on_main, on_main_delayed};

use crate::{
    config::Config,
    desk::{Desk, DeskEvent, RoundView, SessionView, Step},
    server::ServerControl,
};

/// The window's first size; the size the user leaves it at is remembered.
const WINDOW_SIZE: (f64, f64) = (1100.0, 800.0);
const WINDOW_MIN: (f64, f64) = (620.0, 460.0);

static DESK: OnceLock<Desk> = OnceLock::new();
/// Set by the desk (on a server thread) when a new round arrives.
static ATTENTION: AtomicBool = AtomicBool::new(false);
/// A refresh is already posted to the main loop.
static REFRESH_POSTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SERVER: RefCell<Option<ServerControl>> = const { RefCell::new(None) };
    static CONFIG: RefCell<Config> = RefCell::new(Config::default());
    static STATE: std::cell::Cell<Option<State>> = const { std::cell::Cell::new(None) };
}

fn desk() -> &'static Desk {
    DESK.get().expect("the desk is set before the window opens")
}

/// The window's reactive state: `Copy` handles, created once on the main thread.
#[derive(Clone, Copy)]
pub(crate) struct State {
    /// Bumped whenever the desk may have changed.
    rev: Signal<u64>,
    pub(crate) sessions: Memo<Vec<SessionView>>,
    pub(crate) round: Memo<Option<RoundView>>,
    /// The sidebar selection: the Ticket of the shown round.
    selected: Signal<Option<String>>,
    /// The empty state's line: the MCP URL, or why the server is not running.
    pub(crate) status: Signal<String>,
    /// The Option under the mouse on the current page, for the preview panel.
    pub(crate) hovered: Signal<Option<usize>>,
    /// Whether the Note has the keyboard.
    pub(crate) note_focused: Signal<bool>,
    /// Whether the page's key catcher has the keyboard (digits, Enter, Tab).
    pub(crate) keys_focused: Signal<bool>,
}

pub(crate) fn state() -> State {
    STATE
        .with(std::cell::Cell::get)
        .expect("state is created in root()")
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
    desk.on_change(|event| {
        if event == DeskEvent::RoundOpened {
            ATTENTION.store(true, Ordering::Relaxed);
        }
        // Coalesce bursts (heartbeats, many rounds at once) into one refresh.
        if !REFRESH_POSTED.swap(true, Ordering::AcqRel) {
            on_main(|| {
                REFRESH_POSTED.store(false, Ordering::Release);
                refresh();
            });
        }
    });
    DESK.set(desk).ok().expect("run is called once");
    SERVER.with(|s| *s.borrow_mut() = Some(server));
    CONFIG.with(|c| *c.borrow_mut() = config);
    let mut options = WindowOptions::default()
        .title("Question Desk")
        .size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_size(WINDOW_MIN.0, WINDOW_MIN.1)
        .remember_frame("main");
    options.app_name = Some("Question Desk".to_owned());
    options.version = Some(env!("CARGO_PKG_VERSION").to_owned());
    day::launch(options, move || root(status));
}

/// Re-reads the desk: after every intent, and when the server side changed something.
fn refresh() {
    let Some(s) = STATE.with(std::cell::Cell::get) else {
        return;
    };
    s.rev.update(|r| *r += 1);
    // Follow the desk to the round it shows; with none open, the sidebar falls back to its
    // placeholder row by itself.
    if let Some(current) = desk().current().map(|r| r.ticket)
        && s.selected.get_untracked().as_ref() != Some(&current)
    {
        s.selected.set(Some(current));
    }
    if ATTENTION.swap(false, Ordering::Relaxed) {
        request_attention();
    }
}

/// Runs a user intent on the desk and refreshes the window.
pub(crate) fn act(f: impl FnOnce(&Desk)) {
    f(desk());
    refresh();
}

fn root(status: String) -> impl Piece {
    let rev = Signal::new(0_u64);
    let s = State {
        rev,
        sessions: Memo::new(move || {
            let _ = rev.get();
            desk().sessions()
        }),
        round: Memo::new(move || {
            let _ = rev.get();
            desk().current()
        }),
        selected: Signal::new(desk().current().map(|r| r.ticket)),
        status: Signal::new(status),
        hovered: Signal::new(None),
        note_focused: Signal::new(false),
        keys_focused: Signal::new(false),
    };
    STATE.with(|c| c.set(Some(s)));

    if let Ok(appearance) = std::env::var("QD_APPEARANCE") {
        match appearance.as_str() {
            "light" => day::set_appearance(Some(false)),
            "dark" => day::set_appearance(Some(true)),
            _ => {}
        }
    }

    // The sidebar writes the selection; the desk follows it.
    bind(
        move || s.selected.get(),
        move |ticket: &Option<String>| {
            if let Some(ticket) = ticket
                && ticket != WAITING
                && desk().current().is_none_or(|r| &r.ticket != ticket)
            {
                act(|d| d.select(ticket));
            }
        },
    );
    // Leaving a page forgets which Option was hovered.
    bind(
        move || s.round.with(|r| r.as_ref().map(|r| (r.ticket.clone(), r.step))),
        move |_| s.hovered.set(None),
    );

    let mut settings = WindowOptions::default()
        .title("Settings")
        .size(480.0, 600.0)
        .kind(day::WindowKind::Preferences);
    settings.size_to_fit = true;
    day::register_preferences_with(settings, settings::settings_page);
    app_menu(menus());
    day::window_title(move || match s.round.get() {
        Some(round) => match &round.title {
            Some(title) => format!("{title} — {}", round.session),
            None => round.session.clone(),
        },
        None => "Question Desk".to_owned(),
    });
    install_close_confirmation();
    #[cfg(feature = "snapshot")]
    snapshot::install();

    let sidebar = nav(s.selected)
        .style(NavStyle::Sidebar)
        .title("Question Desk")
        .header(move || sidebar_header(s))
        .items(nav_items(move || sidebar_rows(&s.sessions.get()), move |row| {
            sidebar_item(s, row)
        }))
        .destination(move |_ticket: &Option<String>| page::detail())
        .local();
    // One toolbar for the window's lifetime: declared on a page, it would be re-registered
    // (and its first registration kept) on every page switch.
    Decorate::toolbar(sidebar, page::toolbar_items)
}

// ---- Sidebar ------------------------------------------------------------------------------

/// The sidebar key of the row shown while no round is open. A split view always shows a
/// selected row's page, so the empty state needs a row; Tickets never contain `#`.
const WAITING: &str = "#waiting";

/// One round in the sidebar, flattened with the Agent Session it belongs to.
#[derive(Clone, PartialEq)]
struct SidebarRow {
    session: String,
    first_of_session: bool,
    ticket: String,
    title: String,
}

fn sidebar_rows(sessions: &[SessionView]) -> Vec<SidebarRow> {
    if sessions.is_empty() {
        return vec![SidebarRow {
            session: String::new(),
            first_of_session: false,
            ticket: WAITING.to_owned(),
            title: "Waiting for Agents".to_owned(),
        }];
    }
    sessions
        .iter()
        .flat_map(|session| {
            session.rounds.iter().enumerate().map(|(i, round)| SidebarRow {
                session: session.label.clone(),
                first_of_session: i == 0,
                ticket: round.ticket.clone(),
                title: round.title.clone().unwrap_or_else(|| round.ticket.clone()),
            })
        })
        .collect()
}

fn sidebar_item(s: State, row: &SidebarRow) -> NavItem<Option<String>> {
    let ticket = row.ticket.clone();
    let mut item = item(Some(row.ticket.clone()), row.title.clone()).badge(move || {
        s.sessions.with(|sessions| {
            sessions
                .iter()
                .flat_map(|s| &s.rounds)
                .find(|r| r.ticket == ticket)
                .map(|r| {
                    let status = if r.new {
                        "New".to_owned()
                    } else {
                        format!("{}/{}", r.answered, r.total)
                    };
                    if r.connected {
                        status
                    } else {
                        format!("⚠ {status}")
                    }
                })
                .unwrap_or_default()
        })
    });
    if row.first_of_session {
        item = item.section(row.session.clone());
    }
    item
}

fn sidebar_header(s: State) -> impl Piece {
    label(move || {
        let open: usize = s.sessions.with(|s| s.iter().map(|s| s.rounds.len()).sum());
        match open {
            0 => "No open rounds".to_owned(),
            1 => "1 open round".to_owned(),
            n => format!("{n} open rounds"),
        }
    })
    .font(Font::Footnote)
    .secondary()
    .padding(Insets::new(0.0, 12.0, 4.0, 12.0))
}

// ---- Menus and window behaviour -----------------------------------------------------------

fn menus() -> Vec<MenuEntry> {
    let s = state();
    let on_question = move || s.round.with(|r| matches!(r, Some(r) if matches!(r.step, Step::Question(_))));
    let has_round = move || s.round.with(Option::is_some);
    vec![
        // Day moves the Quit role into the App menu; a custom action replaces `terminate:`.
        menu_item("Quit Question Desk")
            .role(MenuRole::Quit)
            .shortcut(Shortcut::primary("q"))
            .action(confirm_quit),
        sub_menu(
            "Edit",
            vec![
                menu_role(MenuRole::Undo),
                menu_role(MenuRole::Redo),
                menu_separator(),
                menu_role(MenuRole::Cut),
                menu_role(MenuRole::Copy),
                menu_role(MenuRole::Paste),
                menu_role(MenuRole::SelectAll),
            ],
        ),
        sub_menu(
            "Round",
            vec![
                // AppKit matches a Return key equivalent without regard to Shift (⌘⇧↩ fires
                // the first ⌘↩ item and vice versa), so Next looks at Shift itself.
                menu_item("Next Question")
                    .shortcut(Shortcut::primary(day::Key::Enter))
                    .enabled(has_round)
                    .action(|| {
                        if day::modifiers().shift() {
                            act(Desk::back);
                        } else {
                            page::next_or_submit();
                        }
                    }),
                menu_item("Previous Question")
                    .shortcut(Shortcut::primary(day::Key::Enter).shift())
                    .enabled(has_round)
                    .action(|| act(Desk::back)),
                menu_item("Review")
                    .shortcut(Shortcut::primary("r"))
                    .enabled(on_question)
                    .action(|| act(Desk::review)),
                menu_separator(),
                menu_item("Accept Recommendation")
                    .enabled(on_question)
                    .action(page::accept_recommendation),
                menu_separator(),
                // Escape reaches the menu even while the Note has the keyboard; there it only
                // leaves the Note (README "Answering").
                menu_item("Cancel Round…")
                    .shortcut(Shortcut::plain(day::Key::Escape))
                    .enabled(has_round)
                    .action(page::escape),
            ],
        ),
    ]
}

/// On a new round: bring the window to the front and ask for attention (Dock bounce, taskbar
/// flash) without taking keyboard focus.
fn request_attention() {
    let Some(window) = day::initial_window() else {
        return;
    };
    if window.state().get_untracked() == day::WindowState::Minimized {
        window.set_state(day::WindowState::Normal);
    }
    window.set_level(day::WindowLevel::Floating);
    window.request_attention(day::Attention::Informational);
    on_main_delayed(200, || {
        if let Some(window) = day::initial_window() {
            window.set_level(day::WindowLevel::Normal);
        }
    });
}

/// Closing the window with rounds open asks first; confirming cancels them all.
fn install_close_confirmation() {
    day::task(async {
        // The first window is adopted after the root is built.
        let window = loop {
            if let Some(window) = day::initial_window() {
                break window;
            }
            day::sleep(50).await;
        };
        window.on_close_request(confirm_quit);
        native::intercept_close_button(confirm_quit);
    });
}

/// The close button and Quit (⌘Q): with rounds open, ask first; the Agents get the answers so
/// far, marked as cancelled.
pub(crate) fn confirm_quit() {
    if desk().sessions().is_empty() {
        day::quit();
        return;
    }
    native::mark_destructive("Cancel Rounds and Quit");
    day::task(async move {
        let choice = Alert::new("Close the Question Desk?")
            .message(
                "All open rounds are cancelled; their Agents receive the answers given so far.",
            )
            // The first button answers Return: keep the harmless choice there, so quitting
            // needs a click.
            .button("Keep Answering", false)
            .destructive("Cancel Rounds and Quit", true)
            .present()
            .await;
        if choice == Some(true) {
            desk().cancel_all();
            day::quit();
        }
    });
}

// ---- Settings and server ------------------------------------------------------------------

pub(crate) fn config() -> Config {
    CONFIG.with(|c| c.borrow().clone())
}

/// Restarts the server with `config` (if it changed or is not running) and saves it.
pub(crate) fn apply_config(config: Config) -> Result<(), String> {
    let s = state();
    let (changed, running) = SERVER.with(|server| {
        let server = server.borrow();
        let server = server.as_ref().expect("server is set before launch");
        (config != self::config(), server.is_running())
    });
    if changed || !running {
        let result = SERVER.with(|server| {
            server
                .borrow_mut()
                .as_mut()
                .expect("server is set before launch")
                .restart(config.server_settings())
        });
        if let Err(e) = &result {
            let running = SERVER.with(|s| s.borrow().as_ref().is_some_and(ServerControl::is_running));
            if !running {
                s.status.set(server_status(Err(e.clone())));
            }
            return Err(e.clone());
        }
        s.status.set(server_status(result));
        CONFIG.with(|c| *c.borrow_mut() = config.clone());
    }
    match crate::config::save(&config) {
        Ok(path) => {
            log::info!("saved settings to {}", path.display());
            Ok(())
        }
        Err(e) => Err(format!(
            "The server runs with these settings, but saving failed: {e}"
        )),
    }
}

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
pub(crate) fn is_loopback(host: &str) -> bool {
    let host = host.trim();
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `path` with the home directory shown as `~`.
pub(crate) fn home_relative(path: &std::path::Path) -> String {
    directories::BaseDirs::new()
        .and_then(|d| {
            path.strip_prefix(d.home_dir())
                .ok()
                .map(|p| format!("~/{}", p.display()))
        })
        .unwrap_or_else(|| path.display().to_string())
}

pub(crate) fn cmd_symbol() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}
