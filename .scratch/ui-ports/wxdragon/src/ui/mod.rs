//! The Question Desk window, built from native wxWidgets controls via wxDragon: renders the
//! `desk` read model and dispatches user intents.
//!
//! The desk notifies from server threads; [`schedule_refresh`] hands that to the UI thread
//! with `call_after` and coalesces it. Everything the user does goes through a desk
//! intent followed by a deferred refresh, so no event handler destroys the widget it runs
//! in.

mod markdown;
mod page;
mod settings;
#[cfg(all(target_os = "macos", feature = "snapshot"))]
mod snapshot;
mod style;
mod window_state;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use wxdragon::prelude::*;
use wxdragon::event::KeyboardEvent;
use wxdragon::widgets::dataview::DataViewTreeCtrlStyle;

use crate::{
    config::{self, Config},
    desk::{Desk, DeskEvent, RoundView, SessionView, Step},
    server::ServerControl,
};
use page::{Page, PageKey, WebKind};
use window_state::WindowState;

// Standard wx ids (wx/defs.h): wx maps these to the native items, e.g. `copy:` and the
// app menu's Settings and Quit on macOS.
const ID_UNDO: i32 = 5007;
const ID_REDO: i32 = 5008;
const ID_PREFERENCES: i32 = 5022;
const ID_CUT: i32 = 5031;
const ID_COPY: i32 = 5032;
const ID_PASTE: i32 = 5033;
const ID_SELECTALL: i32 = 5037;
const ID_ZOOM_100: i32 = 5135;
const ID_ZOOM_IN: i32 = 5137;
const ID_ZOOM_OUT: i32 = 5138;
// Our own commands, above wxID_HIGHEST.
const ID_NEXT: i32 = 6001;
const ID_BACK: i32 = 6002;
const ID_REVIEW: i32 = 6003;
const ID_CANCEL_ROUND: i32 = 6004;
const ID_TOGGLE_SIDEBAR: i32 = 6005;
/// wxDV_NO_HEADER: the sidebar is a source list, not a table.
const DV_NO_HEADER: i64 = 0x0002;
/// wxDV_VARIABLE_LINE_HEIGHT: rows hold a title and a status line.
const DV_VARIABLE_LINE_HEIGHT: i64 = 0x0020;
/// wxBORDER_NONE: the outline view sits flush in the sidebar.
const BORDER_NONE: i64 = 0x0020_0000;

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs `f` with the app, if it is up. Event handlers reach the app this way instead of
/// capturing it, so no handler keeps it alive or borrows it across a modal loop.
fn with_app(f: impl FnOnce(&App)) {
    let app = APP.with(|a| a.borrow().clone());
    if let Some(app) = app {
        f(&app);
    }
}

/// Refreshes the window on the UI thread soon; callable from any thread.
fn schedule_refresh(pending: &Arc<AtomicBool>) {
    if !pending.swap(true, Ordering::AcqRel) {
        let pending = pending.clone();
        wxdragon::call_after(Box::new(move || {
            pending.store(false, Ordering::Release);
            with_app(App::refresh);
        }));
        wxdragon::wake_up_idle();
    }
}

/// Starts the MCP server with `config`, then the window, and blocks until it is closed. If
/// the config could not be loaded, the server stays off until the user saves the settings.
pub fn run(
    desk: Desk,
    mut server: ServerControl,
    config: Result<Config, String>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let state = window_state::load();
    wxdragon::main(move |app| {
        // Follow the system (this also turns on dark mode for Win32 controls); the
        // QD_APPEARANCE override exists for screenshots of both modes.
        let appearance = match std::env::var("QD_APPEARANCE").as_deref() {
            Ok("dark") => Appearance::Dark,
            Ok("light") => Appearance::Light,
            _ => Appearance::System,
        };
        let _ = set_appearance(appearance);
        if let Some(instance) = get_app_instance() {
            instance.set_app_name("question-desk");
            instance.set_app_display_name("Question Desk");
        }
        let _ = app;
        let ui = App::build(desk, server, config, status, state);
        APP.with(|a| *a.borrow_mut() = Some(ui.clone()));
        ui.install_listener();
        #[cfg(all(target_os = "macos", feature = "snapshot"))]
        snapshot::install(&ui.frame);
        ui.refresh();
        ui.frame.show(true);
        if state.maximized {
            ui.frame.maximize(true);
        }
    })?;
    APP.with(|a| a.borrow_mut().take());
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Cancel,
    SubmitUnanswered,
    Close,
}

struct TopBar {
    title: StaticText,
    session: StaticText,
    disconnected: StaticText,
    progress: StaticText,
}

struct Footer {
    panel: Panel,
    line: StaticLine,
    back: Button,
    next: Button,
    hints: StaticText,
}

/// Sessions, their ages and the selected Ticket, as last shown in the sidebar.
type SidebarShown = (Vec<SessionView>, Vec<String>, Option<String>);

struct App {
    desk: Desk,
    server: RefCell<ServerControl>,
    /// The settings the server runs with.
    config: RefCell<Config>,
    /// Shown in the empty state: the MCP URL or why the server is not running.
    status: RefCell<String>,
    frame: Frame,
    splitter: SplitterWindow,
    sidebar: Panel,
    open_count: StaticText,
    tree: DataViewTreeCtrl,
    /// Sidebar row ids (the `DataViewItem` pointer) and their Tickets.
    tree_items: RefCell<Vec<(usize, String)>>,
    /// What the sidebar shows, to skip rebuilding it when nothing changed.
    tree_shown: RefCell<Option<SidebarShown>>,
    main: Panel,
    holder: Panel,
    top: TopBar,
    footer: Footer,
    page: RefCell<Option<Page>>,
    state: Cell<WindowState>,
    /// Set by the desk when a new round arrives.
    attention: Arc<AtomicBool>,
    pending: Arc<AtomicBool>,
    relayout_pending: Cell<bool>,
    in_refresh: Cell<bool>,
    refresh_again: Cell<bool>,
    /// Programmatic changes in progress: ignore the events they cause.
    updating: Cell<bool>,
    /// The user confirmed closing; let the next close through.
    closing: Cell<bool>,
    modal: Cell<bool>,
}

impl App {
    fn build(
        desk: Desk,
        server: ServerControl,
        config: Config,
        status: String,
        state: WindowState,
    ) -> Rc<App> {
        style::set_zoom(state.zoom);
        let frame = Frame::builder()
            .with_title("Question Desk")
            .with_size(Size::new(state.width, state.height))
            .build();
        frame.set_min_size(frame.from_dip(Size::new(520, 400)));
        frame.set_menu_bar(menu_bar());
        let dip = |v: i32| frame.from_dip_int(v);

        let splitter = SplitterWindow::builder(&frame)
            .with_style(SplitterWindowStyle::LiveUpdate | SplitterWindowStyle::ThinSash)
            .build();
        splitter.set_minimum_pane_size(dip(160));

        // Sidebar: a native outline view (NSOutlineView, GtkTreeView, generic on Win32).
        let sidebar = Panel::builder(&splitter).build();
        let side_sizer = BoxSizer::builder(Orientation::Vertical).build();
        let open_count = StaticText::builder(&sidebar).with_label("Open Rounds").build();
        style::bold(&open_count);
        open_count.set_foreground_color(style::muted_colour());
        side_sizer.add(&open_count, 0, SizerFlag::All, dip(style::SPACE_S));
        let tree = DataViewTreeCtrl::builder(&sidebar)
            .with_style(DataViewTreeCtrlStyle::from_bits_retain(
                DV_NO_HEADER | DV_VARIABLE_LINE_HEIGHT | BORDER_NONE,
            ))
            .build();
        side_sizer.add(&tree, 1, SizerFlag::Expand, 0);
        sidebar.set_sizer(side_sizer, true);

        // Main area: top bar, page, footer.
        let main = Panel::builder(&splitter).build();
        let main_sizer = BoxSizer::builder(Orientation::Vertical).build();
        let top_panel = Panel::builder(&main).build();
        let top_sizer = BoxSizer::builder(Orientation::Horizontal).build();
        let title = StaticText::builder(&top_panel).with_label("Question Desk").build();
        style::bold(&title);
        let session = StaticText::builder(&top_panel).build();
        session.set_foreground_color(style::muted_colour());
        let disconnected = StaticText::builder(&top_panel)
            .with_label("⚠ Agent disconnected")
            .build();
        disconnected.set_foreground_color(style::warning_colour());
        disconnected.set_tooltip(
            "No Agent call is waiting for this round. Your answers are kept; \
             the Agent can pick them up with its ticket.",
        );
        let progress = StaticText::builder(&top_panel).build();
        progress.set_foreground_color(style::muted_colour());
        let settings = Button::builder(&top_panel)
            .with_label("Settings…")
            .build();
        settings.set_tooltip(&format!("Settings ({}{})", cmd_symbol(), ","));
        settings.on_click(|_| with_app(App::open_settings));
        let center = SizerFlag::AlignCenterVertical;
        top_sizer.add(&title, 0, center | SizerFlag::Right, dip(style::SPACE_S));
        top_sizer.add(&session, 0, center | SizerFlag::Right, dip(style::SPACE_S));
        top_sizer.add(&disconnected, 0, center, 0);
        top_sizer.add_stretch_spacer(1);
        top_sizer.add(&progress, 0, center | SizerFlag::Right, dip(style::SPACE_M));
        top_sizer.add(&settings, 0, center, 0);
        top_panel.set_sizer(top_sizer, true);
        main_sizer.add(
            &top_panel,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dip(style::SPACE_S),
        );
        main_sizer.add(&StaticLine::builder(&main).build(), 0, SizerFlag::Expand, 0);

        let holder = Panel::builder(&main).build();
        holder.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), true);
        main_sizer.add(&holder, 1, SizerFlag::Expand, 0);

        let line = StaticLine::builder(&main).build();
        main_sizer.add(&line, 0, SizerFlag::Expand, 0);
        let footer_panel = Panel::builder(&main).build();
        let footer_sizer = BoxSizer::builder(Orientation::Vertical).build();
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&footer_panel)
            .with_label("Cancel Round…")
            .build();
        cancel.on_click(|_| with_app(|app| app.confirm(Confirm::Cancel)));
        let back = Button::builder(&footer_panel).with_label("Back").build();
        back.on_click(|_| with_app(App::back));
        let next = Button::builder(&footer_panel).with_label("Next").build();
        next.on_click(|_| with_app(App::next_or_submit));
        next.set_default();
        buttons.add(&cancel, 0, center, 0);
        buttons.add_stretch_spacer(1);
        buttons.add(&back, 0, center | SizerFlag::Right, dip(style::SPACE_S));
        buttons.add(&next, 0, center, 0);
        footer_sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        let hints = StaticText::builder(&footer_panel).build();
        style::small(&hints);
        hints.set_foreground_color(style::muted_colour());
        footer_sizer.add(&hints, 0, SizerFlag::Top, dip(style::SPACE_S));
        footer_panel.set_sizer(footer_sizer, true);
        main_sizer.add(
            &footer_panel,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dip(style::SPACE_M),
        );
        main.set_sizer(main_sizer, true);

        if state.sidebar_open {
            splitter.split_vertically(&sidebar, &main, dip(state.sidebar_width));
        } else {
            sidebar.show(false);
            splitter.initialize(&main);
        }

        let app = Rc::new(App {
            desk,
            server: RefCell::new(server),
            config: RefCell::new(config),
            status: RefCell::new(status),
            frame,
            splitter,
            sidebar,
            open_count,
            tree,
            tree_items: RefCell::default(),
            tree_shown: RefCell::default(),
            main,
            holder,
            top: TopBar {
                title,
                session,
                disconnected,
                progress,
            },
            footer: Footer {
                panel: footer_panel,
                line,
                back,
                next,
                hints,
            },
            page: RefCell::default(),
            state: Cell::new(state),
            attention: Arc::default(),
            pending: Arc::default(),
            relayout_pending: Cell::new(false),
            in_refresh: Cell::new(false),
            refresh_again: Cell::new(false),
            updating: Cell::new(false),
            closing: Cell::new(false),
            modal: Cell::new(false),
        });
        app.bind_events();
        app
    }

    fn install_listener(&self) {
        let attention = self.attention.clone();
        let pending = self.pending.clone();
        self.desk.on_change(move |event| {
            if event == DeskEvent::RoundOpened {
                attention.store(true, Ordering::Release);
            }
            schedule_refresh(&pending);
        });
        // Ages in the sidebar ("5 min ago") move on without desk events.
        let timer = Timer::new(&self.frame);
        let pending = self.pending.clone();
        timer.on_tick(move |_| schedule_refresh(&pending));
        timer.start(30_000, false);
        std::mem::forget(timer);
    }

    fn bind_events(&self) {
        self.frame.on_menu(|e| {
            let id = e.get_id();
            with_app(|app| app.menu(id, &e));
        });
        self.frame.on_close(|e| with_app(|app| app.close_requested(&e)));
        self.frame.on_char_hook(|e| {
            if let WindowEventData::Keyboard(key) = &e {
                let mut handled = false;
                with_app(|app| handled = app.key(key));
                if handled {
                    return;
                }
            }
            e.skip(true);
        });
        self.frame.on_size(|e| {
            e.skip(true);
            with_app(|app| {
                app.remember_size();
                app.schedule_relayout();
            });
        });
        self.splitter.on_sash_position_changed(|e| {
            if let Some(pos) = e.get_sash_position() {
                with_app(|app| {
                    let mut state = app.state.get();
                    state.sidebar_width = app.frame.to_dip_int(pos);
                    app.state.set(state);
                    app.schedule_relayout();
                });
            }
        });
        self.holder.on_size(|e| {
            e.skip(true);
            with_app(App::schedule_relayout);
        });
        self.tree.on_selection_changed(|e| {
            let id = e.get_item().and_then(|i| i.get_id::<u8>()).map(|p| p as usize);
            with_app(|app| app.sidebar_selected(id));
        });
    }

    // ---- Refresh ----------------------------------------------------------------------

    /// Brings every widget in line with the desk.
    fn refresh(&self) {
        // Creating a web view runs a nested event loop (wx injects a script and waits for
        // it), in which queued refreshes would run halfway through this one.
        if self.in_refresh.replace(true) {
            self.refresh_again.set(true);
            return;
        }
        self.refresh_inner();
        self.in_refresh.set(false);
        if self.refresh_again.replace(false) {
            schedule_refresh(&self.pending);
        }
    }

    fn refresh_inner(&self) {
        #[cfg(feature = "snapshot")]
        self.autopilot();
        let sessions = self.desk.sessions();
        let round = self.desk.current();
        self.updating.set(true);
        self.frame.freeze();
        self.update_top_bar(round.as_ref());
        self.update_sidebar(&sessions, round.as_ref());
        self.update_page(round.as_ref());
        self.update_footer(round.as_ref());
        self.update_menus(round.as_ref());
        self.frame.thaw();
        self.updating.set(false);
        self.request_attention();
    }

    fn update_top_bar(&self, round: Option<&RoundView>) {
        let top = &self.top;
        match round {
            Some(round) => {
                let title = round.title.as_deref().unwrap_or("Question Round");
                set_label(&top.title, &page::esc(title));
                set_label(&top.session, &page::esc(&format!("— {}", round.session)));
                top.session.show(true);
                top.disconnected.show(!round.connected);
                let progress = match round.step {
                    Step::Question(i) => format!(
                        "Question {} of {} · {}",
                        i + 1,
                        round.questions.len(),
                        round.questions[i].header
                    ),
                    Step::Review => "Review".to_owned(),
                };
                set_label(&top.progress, &page::esc(&progress));
                self.frame.set_title(&format!("{title} — Question Desk"));
            }
            None => {
                set_label(&top.title, "Question Desk");
                top.session.show(false);
                top.disconnected.show(false);
                set_label(&top.progress, "");
                self.frame.set_title("Question Desk");
            }
        }
        if let Some(parent) = top.title.get_parent() {
            parent.layout();
        }
    }

    fn update_sidebar(&self, sessions: &[SessionView], current: Option<&RoundView>) {
        let open: usize = sessions.iter().map(|s| s.rounds.len()).sum();
        let count = if open > 0 {
            format!("Open Rounds ({open})")
        } else {
            "Open Rounds".to_owned()
        };
        set_label(&self.open_count, &count);
        let ages: Vec<String> = sessions
            .iter()
            .flat_map(|s| s.rounds.iter().map(|r| age(r.opened_at)))
            .collect();
        let selected = current.map(|r| r.ticket.clone());
        let shown = (sessions.to_vec(), ages, selected.clone());
        if self.tree_shown.borrow().as_ref() == Some(&shown) {
            return;
        }
        *self.tree_shown.borrow_mut() = Some(shown);

        let tree = self.tree;
        tree.delete_all_items();
        let mut items = Vec::new();
        let root = DataViewItem::default();
        if sessions.is_empty() {
            tree.append_item(&root, "Nothing waiting", -1);
        }
        for session in sessions {
            let container = tree.append_container(&root, &session.label, -1, -1);
            for round in &session.rounds {
                // One line per row: NSOutlineView rows in wxDataViewTreeCtrl do not wrap,
                // so the age moves out and the status shrinks to marks.
                let name = round.title.as_deref().unwrap_or(&round.ticket);
                let mark = match (round.new, round.connected) {
                    (_, false) => "⚠ ",
                    (true, true) => "● ",
                    (false, true) => "",
                };
                let text = format!("{mark}{name}   {}/{}", round.answered, round.total);
                let item = tree.append_item(&container, &text, -1);
                if let Some(id) = item.get_id::<u8>() {
                    items.push((id as usize, round.ticket.clone()));
                    if selected.as_deref() == Some(round.ticket.as_str()) {
                        tree.select(&item);
                    }
                }
            }
            tree.expand(&container);
        }
        *self.tree_items.borrow_mut() = items;
    }

    fn sidebar_selected(&self, id: Option<usize>) {
        if self.updating.get() {
            return;
        }
        let ticket = id.and_then(|id| {
            self.tree_items
                .borrow()
                .iter()
                .find(|(item, _)| *item == id)
                .map(|(_, t)| t.clone())
        });
        match ticket {
            Some(ticket) => self.desk.select(&ticket),
            // A session row: keep showing the selected round.
            None => {
                self.tree_shown.borrow_mut().take();
            }
        }
        schedule_refresh(&self.pending);
    }

    fn page_width(&self) -> i32 {
        page::column_width(&self.holder, self.holder.get_client_size().width)
    }

    fn update_page(&self, round: Option<&RoundView>) {
        let wide = page::is_wide(&self.holder, self.page_width());
        let key = PageKey::new(round, &self.status.borrow(), wide);
        let rebuild = self.page.borrow().as_ref().is_none_or(|p| p.key != key);
        if rebuild {
            if let Some(old) = self.page.borrow_mut().take() {
                // wxDragon destroys child windows at idle time; hidden, the old page
                // already leaves the layout.
                old.panel.show(false);
                old.panel.destroy();
            }
            let page = Page::build(&self.holder, key, round, &self.status.borrow());
            if let Some(sizer) = self.holder.get_sizer() {
                sizer.add(&page.panel, 1, SizerFlag::Expand, 0);
            }
            self.holder.layout();
            page.rewrap(self.page_width());
            page.focus_default();
            *self.page.borrow_mut() = Some(page);
        } else if let (Some(page), Some(round)) = (self.page.borrow().as_ref(), round) {
            page.update(round);
        }
    }

    fn update_footer(&self, round: Option<&RoundView>) {
        let footer = &self.footer;
        footer.panel.show(round.is_some());
        footer.line.show(round.is_some());
        if let Some(round) = round {
            let (label, hints) = match round.step {
                Step::Question(_) => ("Next", QUESTION_HINTS),
                Step::Review => ("Submit", REVIEW_HINTS),
            };
            if footer.next.get_label() != label {
                footer.next.set_label(label);
            }
            footer.back.enable(round.step != Step::Question(0));
            set_label(&footer.hints, &hints.replace('⌘', cmd_symbol()));
        }
        self.main.layout();
    }

    fn update_menus(&self, round: Option<&RoundView>) {
        let Some(bar) = self.frame.get_menu_bar() else { return };
        let has_round = round.is_some();
        for id in [ID_NEXT, ID_REVIEW, ID_CANCEL_ROUND] {
            bar.enable_item(id, has_round);
        }
        bar.enable_item(
            ID_BACK,
            round.is_some_and(|r| r.step != Step::Question(0)),
        );
        if let Some(item) = bar.find_item(ID_NEXT) {
            let label = match round.map(|r| r.step) {
                Some(Step::Review) => "Submit Answers\tCtrl+Return",
                _ => "Next Question\tCtrl+Return",
            };
            if !item.get_label().starts_with(&label[..6]) {
                item.set_label(label);
            }
        }
        if let Some(item) = bar.find_item(ID_TOGGLE_SIDEBAR) {
            let label = if self.splitter_split() {
                "Hide Sidebar\tCtrl+Alt+S"
            } else {
                "Show Sidebar\tCtrl+Alt+S"
            };
            item.set_label(label);
        }
    }

    fn splitter_split(&self) -> bool {
        self.sidebar.is_shown()
    }

    fn schedule_relayout(&self) {
        if self.relayout_pending.replace(true) {
            return;
        }
        wxdragon::call_after(Box::new(|| {
            with_app(|app| {
                app.relayout_pending.set(false);
                app.relayout();
            })
        }));
        wxdragon::wake_up_idle();
    }

    /// After a resize: re-wrap the page for the new width, or rebuild it when the
    /// preview moves between beside and below the Options.
    fn relayout(&self) {
        if self.in_refresh.get() {
            self.refresh_again.set(true);
            return;
        }
        let width = self.page_width();
        let wide = page::is_wide(&self.holder, width);
        let key = PageKey::new(self.desk.current().as_ref(), &self.status.borrow(), wide);
        let stale = match self.page.try_borrow().as_deref() {
            Ok(Some(page)) if page.key == key => {
                if page.col_width() != width {
                    page.rewrap(width);
                }
                false
            }
            Ok(_) => true,
            Err(_) => false,
        };
        if stale {
            self.refresh();
        }
    }

    fn web_height(&self, kind: WebKind, height: i32) {
        if let Ok(page) = self.page.try_borrow()
            && let Some(page) = page.as_ref()
        {
            page.web_height(kind, height);
        }
    }

    /// Screenshot aid (`snapshot` feature): `QD_AUTOPILOT="2;note:Hi;next;review"` applies
    /// desk intents to the first round that arrives, so the script can show other states.
    /// Steps: `select:N`, `note:TEXT`, `next`, `back`, `review`, `goto:N`, `hover:N`,
    /// `key:NAME` (macOS), and `settings`, `cancel`, `submit` to open those dialogs.
    #[cfg(feature = "snapshot")]
    fn autopilot(&self) {
        thread_local! {
            static STEPS: RefCell<Option<String>> =
                RefCell::new(std::env::var("QD_AUTOPILOT").ok());
        }
        if self.desk.current().is_none() {
            return;
        }
        let Some(steps) = STEPS.with(|s| s.borrow_mut().take()) else { return };
        // Let the other demo rounds arrive first, so the sidebar shows them all.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut keys = Vec::new();
        for step in steps.split(';').map(str::trim) {
            let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
            let n = arg.trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
            match verb {
                "select" => self.desk.toggle_option(n),
                "note" => self.desk.set_note(arg),
                "next" => self.desk.next(),
                "back" => self.desk.back(),
                "review" => self.desk.review(),
                "goto" => self.desk.go_to(n),
                "hover" => wxdragon::call_after(Box::new(move || {
                    with_app(|app| app.hover_option(Some(n)))
                })),
                "key" => keys.push(arg.to_owned()),
                "settings" => wxdragon::call_after(Box::new(|| with_app(App::open_settings))),
                "cancel" => wxdragon::call_after(Box::new(|| {
                    with_app(|app| app.confirm(Confirm::Cancel))
                })),
                "submit" => wxdragon::call_after(Box::new(|| with_app(App::next_or_submit))),
                other => log::warn!("autopilot: unknown step {other:?}"),
            }
        }
        // Keys go through AppKit once the page exists, one every 300 ms.
        #[cfg(target_os = "macos")]
        if !keys.is_empty() {
            let keys = RefCell::new(keys.into_iter());
            let timer = Timer::new(&self.frame);
            let frame = self.frame;
            timer.on_tick(move |_| {
                if let Some(key) = keys.borrow_mut().next() {
                    log::info!("autopilot: key {key}");
                    snapshot::send_key(&frame, &key);
                }
            });
            timer.start(300, false);
            std::mem::forget(timer);
        }
    }

    // ---- Intents ----------------------------------------------------------------------

    fn go_to(&self, index: usize) {
        if self.updating.get() {
            return;
        }
        self.desk.go_to(index);
        schedule_refresh(&self.pending);
    }

    fn review(&self) {
        if self.updating.get() {
            return;
        }
        self.desk.review();
        schedule_refresh(&self.pending);
    }

    fn back(&self) {
        self.desk.back();
        schedule_refresh(&self.pending);
    }

    fn next_or_submit(&self) {
        let Some(round) = self.desk.current() else { return };
        match round.step {
            Step::Question(_) => {
                self.desk.next();
                schedule_refresh(&self.pending);
            }
            Step::Review => self.submit(&round),
        }
    }

    fn toggle_option(&self, index: usize) {
        if self.updating.get() {
            return;
        }
        self.desk.toggle_option(index);
        schedule_refresh(&self.pending);
    }

    /// A radio button turned on: select its Option (a native radio never turns off by a
    /// click; digits and the description toggle it off again).
    fn select_option(&self, index: usize) {
        if self.updating.get() {
            return;
        }
        let selected = self.desk.current().is_some_and(|r| match r.step {
            Step::Question(q) => r.questions[q].options.get(index).is_some_and(|o| o.selected),
            Step::Review => true,
        });
        if !selected {
            self.desk.toggle_option(index);
        }
        schedule_refresh(&self.pending);
    }

    fn click_description(&self, index: usize) {
        self.toggle_option(index);
    }

    fn hover_option(&self, option: Option<usize>) {
        if let Ok(page) = self.page.try_borrow()
            && let Some(page) = page.as_ref()
        {
            page.hover(option);
        }
        // Deferred, so leaving the label for its own description does not flash the
        // selected Option's preview in between.
        wxdragon::call_after(Box::new(|| {
            with_app(|app| {
                let Some(round) = app.desk.current() else { return };
                let Step::Question(i) = round.step else { return };
                if let Ok(page) = app.page.try_borrow()
                    && let Some(page) = page.as_ref()
                {
                    page.update_preview(&round.questions[i]);
                }
            })
        }));
        wxdragon::wake_up_idle();
    }

    fn note_changed(&self) {
        if self.updating.get() {
            return;
        }
        let note = self.page.try_borrow().ok().and_then(|p| p.as_ref()?.note_value());
        if let Some(note) = note {
            self.desk.set_note(&note);
            schedule_refresh(&self.pending);
        }
    }

    fn submit(&self, round: &RoundView) {
        if round.unanswered().is_empty() {
            self.desk.submit();
            schedule_refresh(&self.pending);
        } else {
            self.confirm(Confirm::SubmitUnanswered);
        }
    }

    // ---- Keyboard ---------------------------------------------------------------------

    /// The README's shortcuts that are not menu accelerators. Returns whether the key was
    /// used; unused keys go on to the focused control.
    fn key(&self, key: &KeyboardEvent) -> bool {
        if self.modal.get() {
            return false;
        }
        let Some(code) = key.get_key_code() else { return false };
        log::debug!("key {code} cmd={} shift={}", key.event.cmd_down(), key.shift_down());
        let plain = !key.event.cmd_down() && !key.shift_down() && !key.alt_down();
        let Some(round) = self.desk.current() else { return false };
        let enter = code == WXK_RETURN || code == WXK_NUMPAD_ENTER;
        // Accelerators also arrive here on some platforms before the menu sees them.
        if enter && key.event.cmd_down() {
            if key.shift_down() {
                self.back();
            } else {
                self.next_or_submit();
            }
            return true;
        }
        let Step::Question(index) = round.step else {
            if code == WXK_ESCAPE && plain {
                self.confirm(Confirm::Cancel);
                return true;
            }
            return false;
        };
        let note_focused = self
            .page
            .try_borrow()
            .ok()
            .is_some_and(|p| p.as_ref().is_some_and(Page::note_focused));
        if note_focused {
            // Esc leaves the Note; everything else is typing.
            if code == WXK_ESCAPE && plain {
                wxdragon::call_after(Box::new(|| {
                    with_app(|app| {
                        if let Some(page) = app.page.borrow().as_ref() {
                            page.focus_default();
                        }
                    })
                }));
                wxdragon::wake_up_idle();
                return true;
            }
            return false;
        }
        if !plain {
            return false;
        }
        if code == WXK_TAB {
            // After the key event is done: AppKit restores the focused control's first
            // responder status when a key handler moves focus in the middle of it.
            wxdragon::call_after(Box::new(|| {
                with_app(|app| {
                    if let Some(q) = app.page.borrow().as_ref().and_then(|p| p.question.as_ref()) {
                        q.note.set_focus();
                        q.note.set_insertion_point_end();
                    }
                })
            }));
            wxdragon::wake_up_idle();
            return true;
        }
        if code == WXK_ESCAPE {
            self.confirm(Confirm::Cancel);
            return true;
        }
        let question = &round.questions[index];
        let accepts = question.answer.is_none()
            && (question.recommendation_text.is_some()
                || question.options.iter().any(|o| o.recommended));
        if enter && accepts {
            self.desk.accept_recommendation();
            schedule_refresh(&self.pending);
            return true;
        }
        if (i32::from(b'1')..=i32::from(b'9')).contains(&code) {
            self.desk.toggle_option((code - i32::from(b'1')) as usize);
            schedule_refresh(&self.pending);
            return true;
        }
        false
    }

    // ---- Menus ------------------------------------------------------------------------

    fn menu(&self, id: i32, event: &Event) {
        match id {
            ID_NEXT => self.next_or_submit(),
            ID_BACK => self.back(),
            ID_REVIEW => {
                self.desk.review();
                schedule_refresh(&self.pending);
            }
            ID_CANCEL_ROUND => self.confirm(Confirm::Cancel),
            ID_PREFERENCES => self.open_settings(),
            ID_EXIT => {
                self.frame.close(false);
            }
            ID_TOGGLE_SIDEBAR => self.toggle_sidebar(),
            ID_ZOOM_IN => self.zoom(style::zoom() + 0.1),
            ID_ZOOM_OUT => self.zoom(style::zoom() - 0.1),
            ID_ZOOM_100 => self.zoom(1.0),
            // Edit commands belong to the focused text control.
            _ => event.skip(true),
        }
    }

    fn toggle_sidebar(&self) {
        let mut state = self.state.get();
        if self.sidebar.is_shown() {
            state.sidebar_width = self.frame.to_dip_int(self.splitter.sash_position());
            self.splitter.unsplit(Some(&self.sidebar));
            state.sidebar_open = false;
        } else {
            self.sidebar.show(true);
            self.splitter.split_vertically(
                &self.sidebar,
                &self.main,
                self.frame.from_dip_int(state.sidebar_width),
            );
            state.sidebar_open = true;
        }
        self.state.set(state);
        schedule_refresh(&self.pending);
    }

    fn zoom(&self, zoom: f32) {
        let zoom = (zoom * 10.0).round() / 10.0;
        let zoom = zoom.clamp(window_state::MIN_ZOOM, window_state::MAX_ZOOM);
        style::set_zoom(zoom);
        let mut state = self.state.get();
        state.zoom = zoom;
        self.state.set(state);
        schedule_refresh(&self.pending);
    }

    // ---- Window -----------------------------------------------------------------------

    fn remember_size(&self) {
        let mut state = self.state.get();
        state.maximized = self.frame.is_maximized();
        if !state.maximized && !self.frame.is_iconized() {
            let size = self.frame.get_size();
            state.width = size.width;
            state.height = size.height;
        }
        self.state.set(state);
    }

    /// On a new round: bring the window to the front and ask for attention (dock bounce,
    /// taskbar flash) without taking keyboard focus from the app the user is in.
    fn request_attention(&self) {
        if self.attention.swap(false, Ordering::AcqRel) {
            if self.frame.is_iconized() {
                self.frame.iconize(false);
            }
            self.frame.raise();
            self.frame.request_user_attention(UserAttentionFlag::Info);
        }
    }

    fn close_requested(&self, event: &WindowEventData) {
        if !self.closing.get() && !self.desk.sessions().is_empty() {
            if let WindowEventData::General(e) = event
                && e.can_veto()
            {
                e.veto();
            }
            self.confirm(Confirm::Close);
            return;
        }
        if self.sidebar.is_shown() {
            let mut state = self.state.get();
            state.sidebar_width = self.frame.to_dip_int(self.splitter.sash_position());
            self.state.set(state);
        }
        window_state::save(&self.state.get());
        event.skip(true);
    }

    // ---- Dialogs ----------------------------------------------------------------------

    fn confirm(&self, which: Confirm) {
        if self.modal.get() {
            return;
        }
        let round = self.desk.current();
        if which != Confirm::Close && round.is_none() {
            return;
        }
        let (title, text, confirm) = match which {
            Confirm::Cancel => (
                "Cancel this round?",
                "The Agent receives the answers you gave so far, marked as cancelled.".to_owned(),
                "Cancel Round",
            ),
            Confirm::SubmitUnanswered => (
                "Submit with unanswered questions?",
                format!(
                    "{} question(s) have no answer. The Agent is told which ones.",
                    round.as_ref().map_or(0, |r| r.unanswered().len())
                ),
                "Submit Anyway",
            ),
            Confirm::Close => (
                "Close the Question Desk?",
                "All open rounds are cancelled; their Agents receive the answers given so far."
                    .to_owned(),
                "Cancel Rounds and Quit",
            ),
        };
        // Enter confirms only the harmless dialog; cancelling needs a click.
        let mut style = MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning;
        if which != Confirm::SubmitUnanswered {
            style |= MessageDialogStyle::NoDefault;
        }
        let dialog = MessageDialog::builder(&self.frame, &text, title)
            .with_style(style)
            .build();
        dialog.set_yes_no_labels(confirm, "Keep Answering");
        self.modal.set(true);
        let answer = dialog.show_modal();
        self.modal.set(false);
        drop(dialog);
        if answer != ID_YES {
            return;
        }
        match which {
            Confirm::Cancel => self.desk.cancel(),
            Confirm::SubmitUnanswered => self.desk.submit(),
            Confirm::Close => {
                self.desk.cancel_all();
                self.closing.set(true);
                self.frame.close(true);
                return;
            }
        }
        schedule_refresh(&self.pending);
    }

    fn open_settings(&self) {
        if self.modal.get() {
            return;
        }
        self.modal.set(true);
        let config = self.config.borrow().clone();
        settings::show(&self.frame, &config, |config| {
            let mut result = Ok(());
            with_app(|app| result = app.save_settings(config));
            result
        });
        self.modal.set(false);
        schedule_refresh(&self.pending);
    }

    /// Restarts the server with `config` and writes it to the config file.
    fn save_settings(&self, config: &Config) -> Result<(), String> {
        let running = self.server.borrow().is_running();
        if *config != *self.config.borrow() || !running {
            let result = self.server.borrow_mut().restart(config.server_settings());
            if let Err(e) = &result
                && !self.server.borrow().is_running()
            {
                *self.status.borrow_mut() = server_status(Err(e.clone()));
            }
            let url = result?;
            *self.status.borrow_mut() = server_status(Ok(url));
            *self.config.borrow_mut() = config.clone();
        }
        match config::save(config) {
            Ok(path) => {
                log::info!("saved settings to {}", path.display());
                Ok(())
            }
            Err(e) => Err(format!(
                "The server runs with these settings, but saving failed: {e}"
            )),
        }
    }
}

const QUESTION_HINTS: &str =
    "1–9 select   ·   Tab note   ·   ↩ accept recommendation   ·   ⌘↩ next   ·   ⌘⇧↩ back   ·   Esc leave note / cancel";
const REVIEW_HINTS: &str = "⌘↩ submit   ·   ⌘⇧↩ back   ·   Esc cancel";

fn set_label(label: &StaticText, text: &str) {
    if label.get_label() != text {
        label.set_label(text);
    }
}

fn menu_bar() -> MenuBar {
    let edit = Menu::builder()
        .append_item(ID_UNDO, "&Undo\tCtrl+Z", "")
        .append_item(ID_REDO, "&Redo\tCtrl+Shift+Z", "")
        .append_separator()
        .append_item(ID_CUT, "Cu&t\tCtrl+X", "")
        .append_item(ID_COPY, "&Copy\tCtrl+C", "")
        .append_item(ID_PASTE, "&Paste\tCtrl+V", "")
        .append_item(ID_SELECTALL, "Select &All\tCtrl+A", "")
        .build();
    let round = Menu::builder()
        .append_item(ID_NEXT, "Next Question\tCtrl+Return", "")
        .append_item(ID_BACK, "Previous Question\tCtrl+Shift+Return", "")
        .append_item(ID_REVIEW, "Review Answers", "")
        .append_separator()
        .append_item(ID_CANCEL_ROUND, "Cancel Round…", "")
        .build();
    let view = Menu::builder()
        .append_item(ID_TOGGLE_SIDEBAR, "Hide Sidebar\tCtrl+Alt+S", "")
        .append_separator()
        .append_item(ID_ZOOM_100, "Actual Size\tCtrl+0", "")
        .append_item(ID_ZOOM_IN, "Zoom In\tCtrl++", "")
        .append_item(ID_ZOOM_OUT, "Zoom Out\tCtrl+-", "")
        .build();
    let mut bar = MenuBar::builder();
    if cfg!(target_os = "macos") {
        // wx moves Settings and Quit into the application menu.
        round.append(ID_PREFERENCES, "Settings…\tCtrl+,", "", ItemKind::Normal);
        round.append(ID_EXIT, "Quit Question Desk\tCtrl+Q", "", ItemKind::Normal);
    } else {
        let file = Menu::builder()
            .append_item(ID_PREFERENCES, "&Settings…\tCtrl+,", "")
            .append_separator()
            .append_item(ID_EXIT, "&Quit\tCtrl+Q", "")
            .build();
        bar = bar.append(file, "&File");
    }
    bar.append(edit, "&Edit")
        .append(round, "&Round")
        .append(view, "&View")
        .build()
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

fn cmd_symbol() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// "just now", "5 min ago", "2 h ago".
fn age(since: SystemTime) -> String {
    let secs = since.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}
