//! The Question Desk window, in Slint (`app.slint`): maps the `desk` read model into Slint
//! properties and models, and turns Slint callbacks into desk intents.
//!
//! Backend and renderer: winit + Skia over OpenGL by default (colour emoji; Skia's default
//! Metal/wgpu surface left the window transparent on macOS 27, see PORT_NOTES.md). With the `qt`
//! cargo feature the Qt backend draws the `qt` style through QStyle. `SLINT_BACKEND` overrides.

mod grapheme;
#[cfg(target_os = "macos")]
mod macos_text_input;
mod markdown;
mod snapshot;

slint::include_modules!();

use std::{
    cell::RefCell,
    net::IpAddr,
    path::PathBuf,
    rc::Rc,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    config::{self, Config},
    desk::{Desk, DeskEvent, QuestionState, RoundView, SessionView, Step},
    server::{DEFAULT_HOST, DEFAULT_PORT, ServerControl},
};

/// Starts the MCP server with `config`, then the window, and blocks until it is closed. If
/// the config could not be loaded, the server stays off until the user saves the settings.
pub fn run(
    desk: Desk,
    mut server: ServerControl,
    config: Result<Config, String>,
) -> Result<(), slint::PlatformError> {
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

    if std::env::var_os("SLINT_BACKEND").is_none() && !cfg!(feature = "qt") {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia-opengl".into())
            .select()?;
    }
    let app = AppWindow::new()?;
    let saved = WindowState::load();
    let models = Models::default();
    models.attach(&app);
    let controller = Rc::new(Controller {
        app: app.as_weak(),
        desk: desk.clone(),
        server: RefCell::new(server),
        config: RefCell::new(config),
        status: RefCell::new(status),
        models,
        dialog: RefCell::new(None),
        settings: RefCell::new(None),
        about: RefCell::new(None),
        closing: RefCell::new(false),
        note: RefCell::new((String::new(), String::new())),
        window: RefCell::new(saved.clone()),
        base_scale: RefCell::new(None),
        restore_level: slint::Timer::default(),
        startup: slint::Timer::default(),
    });

    app.set_cmd(
        if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        }
        .into(),
    );
    app.set_sidebar_visible(saved.sidebar_visible);
    app.set_sidebar_width(saved.sidebar_width);
    if let Some((w, h)) = saved.size {
        app.window().set_size(slint::LogicalSize::new(w, h));
    }
    controller.wire(&app);
    controller.refresh();

    // Agent-side changes arrive on server threads; hop onto the UI thread.
    let weak = app.as_weak();
    desk.on_change(move |event| {
        let opened = event == DeskEvent::RoundOpened;
        let _ = weak.upgrade_in_event_loop(move |app| app.invoke_desk_changed(opened));
    });
    // Keeps "5 min ago" current.
    let ticker = slint::Timer::default();
    {
        let c = controller.clone();
        ticker.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(30),
            move || c.refresh(),
        );
    }

    let _snapshot = {
        let (c, d) = (controller.clone(), controller.clone());
        snapshot::install(
            move |dialog| c.open_for_snapshot(dialog),
            move |write| d.capture(write),
        )
    };
    app.show()?;
    controller.after_show();
    if let Ok(appearance) = std::env::var("QUESTION_DESK_APPEARANCE") {
        let value = match appearance.as_str() {
            "light" => 1,
            "dark" => 2,
            _ => 0,
        };
        controller.set_appearance(value);
    } else if saved.appearance != 0 {
        controller.set_appearance(saved.appearance);
    }

    slint::run_event_loop()?;
    controller.save_window_state();
    // Cmd+Q from the App menu ends the loop without the close confirmation.
    desk.cancel_all();
    Ok(())
}

/// Persistent models, updated row by row so repeated elements keep their state.
#[derive(Default)]
struct Models {
    sidebar: Rc<VecModel<SidebarRow>>,
    steps: Rc<VecModel<StepEntry>>,
    body: Rc<VecModel<MdBlock>>,
    options: Rc<VecModel<OptionEntry>>,
    review: Rc<VecModel<ReviewEntry>>,
}

impl Models {
    fn attach(&self, app: &AppWindow) {
        app.set_sidebar_rows(ModelRc::from(self.sidebar.clone()));
        app.set_steps(ModelRc::from(self.steps.clone()));
        app.set_body(ModelRc::from(self.body.clone()));
        app.set_options(ModelRc::from(self.options.clone()));
        app.set_review(ModelRc::from(self.review.clone()));
    }
}

/// Replaces the rows of `model` with `rows`, touching only rows that changed.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    for (i, row) in rows.iter().enumerate() {
        if i < model.row_count() {
            if model.row_data(i).as_ref() != Some(row) {
                model.set_row_data(i, row.clone());
            }
        } else {
            model.push(row.clone());
        }
    }
    while model.row_count() > rows.len() {
        model.remove(model.row_count() - 1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialog {
    Cancel,
    SubmitUnanswered,
    Close,
}

struct Controller {
    app: slint::Weak<AppWindow>,
    desk: Desk,
    server: RefCell<ServerControl>,
    /// The settings the server runs with.
    config: RefCell<Config>,
    /// Shown in the empty state: the MCP URL or why the server is not running.
    status: RefCell<String>,
    models: Models,
    dialog: RefCell<Option<(Dialog, ConfirmDialog)>>,
    settings: RefCell<Option<SettingsDialog>>,
    about: RefCell<Option<AboutDialog>>,
    /// The user confirmed closing.
    closing: RefCell<bool>,
    /// Page key and Note text last shown, to tell a Backspace edit from a page change.
    note: RefCell<(String, String)>,
    window: RefCell<WindowState>,
    /// The display's scale factor without zoom.
    base_scale: RefCell<Option<f32>>,
    /// Puts the window back to its normal level after raising it for a new round.
    restore_level: slint::Timer,
    /// Waits for the native window after `show()`.
    startup: slint::Timer,
}

impl Controller {
    fn wire(self: &Rc<Self>, app: &AppWindow) {
        let c = self.clone();
        app.on_desk_changed(move |opened| {
            c.refresh();
            if opened {
                c.request_attention();
            }
        });
        macro_rules! intent {
            ($on:ident, |$c:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {{
                let $c = self.clone();
                app.$on(move |$($arg: $ty),*| {
                    if $c.modal_open() {
                        return;
                    }
                    $body;
                    $c.refresh();
                });
            }};
        }
        intent!(on_select_round, |c, ticket: SharedString| c
            .desk
            .select(&ticket));
        intent!(on_toggle_option, |c, i: i32| c
            .desk
            .toggle_option(i as usize));
        intent!(on_next, |c| c.desk.next());
        intent!(on_back, |c| c.desk.back());
        intent!(on_go_to, |c, i: i32| c.desk.go_to(i as usize));
        intent!(on_review_requested, |c| c.desk.review());
        intent!(on_accept_recommendation, |c| c.desk.accept_recommendation());
        intent!(on_submit_requested, |c| c.submit());
        intent!(on_cancel_requested, |c| c.open_dialog(Dialog::Cancel));
        intent!(on_settings_requested, |c| c.open_settings());

        let c = self.clone();
        app.on_note_edited(move |text| c.note_edited(&text));
        let c = self.clone();
        app.on_close_requested(move || c.close_requested());
        let c = self.clone();
        app.window().on_close_requested(move || {
            c.close_requested();
            slint::CloseRequestResponse::KeepWindowShown
        });
        let c = self.clone();
        app.on_about_requested(move || c.open_about());
        let c = self.clone();
        app.on_zoom(move |step| c.zoom(step));
        let c = self.clone();
        app.on_appearance_changed(move |value| c.set_appearance(value));
        app.on_emoji_picker(|| {
            #[cfg(target_os = "macos")]
            macos_text_input::open_picker();
        });
        app.global::<Bridge>().on_open_url(|url| {
            if let Err(e) = open_url(&url) {
                log::warn!("cannot open {url}: {e}");
            }
        });
    }

    /// Slint creates the native window only once the event loop runs; the rest of the setup
    /// waits for it (the Qt backend has no winit window, so it gives up after two seconds).
    fn after_show(self: &Rc<Self>) {
        use slint::winit_030::WinitWindowAccessor as _;
        let c = self.clone();
        let tries = std::cell::Cell::new(0);
        self.startup.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                let Some(app) = c.app.upgrade() else { return };
                tries.set(tries.get() + 1);
                if app.window().with_winit_window(|_| ()).is_some() || tries.get() > 40 {
                    c.startup.stop();
                    c.window_ready(&app);
                }
            },
        );
    }

    fn window_ready(self: &Rc<Self>, app: &AppWindow) {
        *self.base_scale.borrow_mut() = Some(app.window().scale_factor());
        let zoom = self.window.borrow().zoom;
        if (zoom - 1.0).abs() > f32::EPSILON {
            self.apply_zoom(zoom);
        }
        #[cfg(target_os = "macos")]
        {
            let weak = self.app.clone();
            macos_text_input::install(move |text| {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let text = SharedString::from(text);
                    app.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                            text: text.clone(),
                        });
                    app.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
                });
            });
        }
        app.set_system_scheme(system_scheme(app));
    }

    fn modal_open(&self) -> bool {
        self.dialog.borrow().is_some() || self.settings.borrow().is_some()
    }

    // ---- Read model → Slint ---------------------------------------------------------------

    fn refresh(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sessions = self.desk.sessions();
        let round = self.desk.current();

        sync(&self.models.sidebar, sidebar_rows(&sessions));
        app.set_open_count(sessions.iter().map(|s| s.rounds.len()).sum::<usize>() as i32);
        app.set_selected_ticket(
            round
                .as_ref()
                .map(|r| r.ticket.clone())
                .unwrap_or_default()
                .into(),
        );
        app.set_status(self.status.borrow().as_str().into());
        app.set_modal_open(self.modal_open());

        let Some(round) = round else {
            app.set_has_round(false);
            return;
        };
        app.set_has_round(true);
        app.set_session(round.session.as_str().into());
        app.set_round_title(round.title.clone().unwrap_or_default().into());
        app.set_connected(round.connected);
        sync(
            &self.models.steps,
            round
                .questions
                .iter()
                .map(|q| StepEntry {
                    state: match q.state {
                        QuestionState::Unanswered => 0,
                        QuestionState::Answered => 1,
                        QuestionState::Current => 2,
                    },
                    tip: q.question.as_str().into(),
                })
                .collect(),
        );
        match round.step {
            Step::Question(index) => self.show_question(&app, &round, index),
            Step::Review => self.show_review(&app, &round),
        }
    }

    fn show_question(&self, app: &AppWindow, round: &RoundView, index: usize) {
        let question = &round.questions[index];
        let page_key = format!("{}#{index}", round.ticket);
        let new_page = app.get_page_key() != page_key.as_str();
        app.set_step_index(index as i32);
        app.set_header(question.header.as_str().into());
        app.set_question(question.question.as_str().into());
        if new_page {
            sync(
                &self.models.body,
                question
                    .body
                    .as_deref()
                    .map(markdown::blocks)
                    .unwrap_or_default(),
            );
        }
        app.set_recommendation_text(
            question
                .recommendation_text
                .clone()
                .unwrap_or_default()
                .into(),
        );
        sync(
            &self.models.options,
            question
                .options
                .iter()
                .map(|o| OptionEntry {
                    label: o.label.as_str().into(),
                    description: o.description.as_str().into(),
                    preview: o.preview.clone().unwrap_or_default().into(),
                    selected: o.selected,
                    recommended: o.recommended,
                })
                .collect(),
        );
        app.set_multi_select(question.multi_select);
        app.set_options_selected(question.options.iter().any(|o| o.selected));
        app.set_options_have_previews(question.options.iter().any(|o| o.preview.is_some()));
        app.set_selected_preview(
            question
                .options
                .iter()
                .filter(|o| o.selected)
                .find_map(|o| o.preview.clone())
                .unwrap_or_default()
                .into(),
        );
        // Enter accepts the Recommendation only while nothing is selected or noted.
        app.set_accepts_recommendation(
            question.answer.is_none()
                && (question.recommendation_text.is_some()
                    || question.options.iter().any(|o| o.recommended)),
        );
        *self.note.borrow_mut() = (page_key.clone(), question.note.clone());
        app.set_note_text(question.note.as_str().into());
        app.set_page_key(page_key.into());
    }

    fn show_review(&self, app: &AppWindow, round: &RoundView) {
        app.set_step_index(-1);
        app.set_unanswered_count(round.unanswered().len() as i32);
        sync(
            &self.models.review,
            round
                .questions
                .iter()
                .map(|q| ReviewEntry {
                    header: q.header.as_str().into(),
                    question: q.question.as_str().into(),
                    answer: q.answer.clone().unwrap_or_default().into(),
                    answered: q.answer.is_some(),
                    note: q.answer_note.clone().unwrap_or_default().into(),
                })
                .collect(),
        );
        app.set_page_key(format!("{}#review", round.ticket).into());
    }

    fn note_edited(&self, text: &str) {
        if self.modal_open() {
            return;
        }
        let previous = self.note.borrow().1.clone();
        let text = match grapheme::repair_backspace(&previous, text) {
            Some((repaired, cursor)) => {
                if let Some(app) = self.app.upgrade() {
                    app.invoke_set_note(repaired.as_str().into(), cursor as i32);
                }
                repaired
            }
            None => text.to_owned(),
        };
        self.note.borrow_mut().1 = text.clone();
        self.desk.set_note(&text);
        self.refresh();
    }

    /// On a new round: bring the window to the front and ask for attention (Dock bounce,
    /// taskbar flash) without taking keyboard focus.
    fn request_attention(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        use slint::winit_030::{WinitWindowAccessor as _, winit};
        let raised = app.window().with_winit_window(|w| {
            w.set_minimized(false);
            w.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            w.request_user_attention(Some(winit::window::UserAttentionType::Informational));
        });
        if raised.is_some() {
            let weak = self.app.clone();
            self.restore_level.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(300),
                move || {
                    if let Some(app) = weak.upgrade() {
                        app.window().with_winit_window(|w| {
                            w.set_window_level(winit::window::WindowLevel::Normal)
                        });
                    }
                },
            );
        } else {
            #[cfg(target_os = "macos")]
            macos_text_input::request_attention();
        }
    }

    // ---- Dialogs --------------------------------------------------------------------------

    fn submit(self: &Rc<Self>) {
        match self.desk.current() {
            Some(round) if round.unanswered().is_empty() => self.desk.submit(),
            Some(_) => self.open_dialog(Dialog::SubmitUnanswered),
            None => {}
        }
    }

    fn close_requested(self: &Rc<Self>) {
        if *self.closing.borrow() || self.desk.sessions().is_empty() {
            self.quit();
        } else if !self.modal_open() {
            self.open_dialog(Dialog::Close);
        }
    }

    fn quit(&self) {
        self.save_window_state();
        if let Some(app) = self.app.upgrade() {
            let _ = app.hide();
        }
        let _ = slint::quit_event_loop();
    }

    fn open_dialog(self: &Rc<Self>, kind: Dialog) {
        if self.modal_open() {
            return;
        }
        let unanswered = self.desk.current().map_or(0, |r| r.unanswered().len());
        let (heading, message, confirm) = match kind {
            Dialog::Cancel => (
                "Cancel this round?",
                "The Agent receives the answers you gave so far, marked as cancelled.".to_owned(),
                "Cancel round",
            ),
            Dialog::SubmitUnanswered => (
                "Submit with unanswered questions?",
                format!("{unanswered} question(s) have no answer. The Agent is told which ones."),
                "Submit anyway",
            ),
            Dialog::Close => (
                "Close the Question Desk?",
                "All open rounds are cancelled; their Agents receive the answers given so far."
                    .to_owned(),
                "Cancel rounds and quit",
            ),
        };
        let Ok(dialog) = ConfirmDialog::new() else {
            return;
        };
        dialog.set_scheme(self.scheme());
        dialog.set_heading(heading.into());
        dialog.set_message(message.into());
        dialog.set_confirm_label(confirm.into());
        dialog.set_destructive(kind != Dialog::SubmitUnanswered);
        dialog.set_enter_confirms(kind == Dialog::SubmitUnanswered);
        let c = self.clone();
        dialog.on_confirmed(move || c.close_dialog(true));
        let c = self.clone();
        dialog.on_dismissed(move || c.close_dialog(false));
        let c = self.clone();
        dialog.window().on_close_requested(move || {
            c.close_dialog(false);
            slint::CloseRequestResponse::KeepWindowShown
        });
        self.show_over_main(dialog.window());
        *self.dialog.borrow_mut() = Some((kind, dialog));
        self.refresh();
    }

    fn close_dialog(&self, confirmed: bool) {
        let Some((kind, dialog)) = self.dialog.borrow_mut().take() else {
            return;
        };
        let _ = dialog.hide();
        if confirmed {
            match kind {
                Dialog::Cancel => self.desk.cancel(),
                Dialog::SubmitUnanswered => self.desk.submit(),
                Dialog::Close => {
                    self.desk.cancel_all();
                    *self.closing.borrow_mut() = true;
                    self.quit();
                    return;
                }
            }
        }
        self.refresh();
        if let Some(app) = self.app.upgrade() {
            app.invoke_focus_keys();
        }
    }

    /// Shows a dialog window centred over the main window.
    fn show_over_main(&self, window: &slint::Window) {
        let _ = window.show();
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let main = app.window();
        let (pos, size) = (main.position(), main.size());
        let own = window.size();
        let x = pos.x + (size.width as i32 - own.width as i32) / 2;
        let y = pos.y + (size.height as i32 - own.height as i32) / 3;
        window.set_position(slint::PhysicalPosition::new(x, y.max(pos.y)));
    }

    fn open_about(&self) {
        if let Some(about) = self.about.borrow().as_ref() {
            let _ = about.show();
            return;
        }
        let Ok(about) = AboutDialog::new() else {
            return;
        };
        about.set_scheme(self.scheme());
        let weak = about.as_weak();
        about.on_dismissed(move || {
            if let Some(about) = weak.upgrade() {
                let _ = about.hide();
            }
        });
        self.show_over_main(about.window());
        *self.about.borrow_mut() = Some(about);
    }

    // ---- Settings -------------------------------------------------------------------------

    fn open_settings(self: &Rc<Self>) {
        if self.modal_open() {
            return;
        }
        let Ok(dialog) = SettingsDialog::new() else {
            return;
        };
        dialog.set_scheme(self.scheme());
        let config = self.config.borrow().clone();
        dialog.set_host(config.host.as_str().into());
        dialog.set_port(config.port.to_string().into());
        dialog.set_token(config.bearer_token.clone().unwrap_or_default().into());
        dialog.set_default_host(DEFAULT_HOST.into());
        dialog.set_default_port(DEFAULT_PORT.to_string().into());
        dialog.set_saved_in(
            config::path()
                .map(|p| home_relative(&p))
                .unwrap_or_default()
                .into(),
        );
        let weak = dialog.as_weak();
        dialog.on_edited(move || {
            if let Some(d) = weak.upgrade() {
                set_warnings(&d, None);
            }
        });
        let c = self.clone();
        dialog.on_save(move || c.save_settings());
        let c = self.clone();
        dialog.on_cancel(move || c.close_settings());
        let c = self.clone();
        dialog.window().on_close_requested(move || {
            c.close_settings();
            slint::CloseRequestResponse::KeepWindowShown
        });
        set_warnings(&dialog, None);
        self.show_over_main(dialog.window());
        *self.settings.borrow_mut() = Some(dialog);
        self.refresh();
    }

    fn close_settings(&self) {
        if let Some(dialog) = self.settings.borrow_mut().take() {
            let _ = dialog.hide();
        }
        self.refresh();
        if let Some(app) = self.app.upgrade() {
            app.invoke_focus_keys();
        }
    }

    /// Restarts the server with the form's settings and writes them to the config file;
    /// keeps the dialog open with the reason if either fails.
    fn save_settings(&self) {
        let Some(dialog) = self.settings.borrow().as_ref().map(|d| d.clone_strong()) else {
            return;
        };
        let config = match form_config(&dialog) {
            Ok(config) => config,
            Err(e) => return set_warnings(&dialog, Some(e)),
        };
        let changed = config != *self.config.borrow();
        if changed || !self.server.borrow().is_running() {
            let result = self.server.borrow_mut().restart(config.server_settings());
            if let Err(e) = result {
                if !self.server.borrow().is_running() {
                    *self.status.borrow_mut() = server_status(Err(e.clone()));
                }
                return set_warnings(&dialog, Some(e));
            }
            *self.status.borrow_mut() = server_status(result);
            *self.config.borrow_mut() = config.clone();
        }
        match config::save(&config) {
            Ok(path) => {
                log::info!("saved settings to {}", path.display());
                self.close_settings();
            }
            Err(e) => set_warnings(
                &dialog,
                Some(format!(
                    "The server runs with these settings, but saving failed: {e}"
                )),
            ),
        }
    }

    // ---- Screenshots (see snapshot.rs) ---------------------------------------------------

    fn open_for_snapshot(self: &Rc<Self>, dialog: &str) {
        match dialog {
            "settings" => self.open_settings(),
            "cancel" => self.open_dialog(Dialog::Cancel),
            "submit" => self.open_dialog(Dialog::SubmitUnanswered),
            "close" => self.open_dialog(Dialog::Close),
            "about" => self.open_about(),
            "review" => {
                self.desk.review();
                self.refresh();
            }
            _ => {}
        }
    }

    /// Dialogs first: the script picks up the files as soon as the main one exists.
    fn capture(&self, write: &dyn Fn(&str, &slint::Window)) {
        if let Some((_, dialog)) = self.dialog.borrow().as_ref() {
            write("dialog", dialog.window());
        }
        if let Some(dialog) = self.settings.borrow().as_ref() {
            write("dialog", dialog.window());
        }
        if let Some(about) = self.about.borrow().as_ref() {
            write("dialog", about.window());
        }
        if let Some(app) = self.app.upgrade() {
            write("", app.window());
        }
    }

    // ---- View -----------------------------------------------------------------------------

    /// `step`: 1 zoom in, -1 zoom out, 0 actual size.
    fn zoom(&self, step: i32) {
        let current = self.window.borrow().zoom;
        let zoom = match step {
            0 => 1.0,
            s if s > 0 => (current * 1.1).min(3.0),
            _ => (current / 1.1).max(0.5),
        };
        self.window.borrow_mut().zoom = zoom;
        self.apply_zoom(zoom);
    }

    /// Zooms the whole UI by changing the window's scale factor; the physical size stays.
    fn apply_zoom(&self, zoom: f32) {
        let (Some(app), Some(base)) = (self.app.upgrade(), *self.base_scale.borrow()) else {
            return;
        };
        let window = app.window();
        let physical = window.size();
        let scale = base * zoom;
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.dispatch_event(slint::platform::WindowEvent::Resized {
            size: physical.to_logical(scale),
        });
    }

    /// The explicit colour scheme for new dialog windows (each has its own `Palette`).
    fn scheme(&self) -> slint::language::ColorScheme {
        match self.window.borrow().appearance {
            1 => slint::language::ColorScheme::Light,
            2 => slint::language::ColorScheme::Dark,
            _ => slint::language::ColorScheme::Unknown,
        }
    }

    /// 0 system, 1 light, 2 dark.
    fn set_appearance(&self, value: i32) {
        if let Some(app) = self.app.upgrade() {
            app.set_appearance(value);
        }
        self.window.borrow_mut().appearance = value;
    }

    fn save_window_state(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let mut state = self.window.borrow_mut();
        let base = self
            .base_scale
            .borrow()
            .unwrap_or(app.window().scale_factor());
        let size = app.window().size().to_logical(base);
        if size.width > 0.0 && size.height > 0.0 {
            state.size = Some((size.width, size.height));
        }
        state.sidebar_visible = app.get_sidebar_visible();
        state.sidebar_width = app.get_sidebar_width();
        state.save();
    }
}

/// The OS colour scheme, for returning to "System" after an explicit choice.
fn system_scheme(app: &AppWindow) -> slint::language::ColorScheme {
    use slint::winit_030::{WinitWindowAccessor as _, winit};
    match app.window().with_winit_window(|w| w.theme()).flatten() {
        Some(winit::window::Theme::Dark) => slint::language::ColorScheme::Dark,
        Some(winit::window::Theme::Light) => slint::language::ColorScheme::Light,
        None => slint::language::ColorScheme::Unknown,
    }
}

fn sidebar_rows(sessions: &[SessionView]) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    for session in sessions {
        rows.push(SidebarRow {
            is_header: true,
            label: session.label.as_str().into(),
            ..Default::default()
        });
        for round in &session.rounds {
            let mut meta = format!(
                "{}/{} answered · {}",
                round.answered,
                round.total,
                age(round.opened_at)
            );
            if !round.connected {
                meta.push_str(" · ⚠ disconnected");
            }
            rows.push(SidebarRow {
                is_header: false,
                label: round.title.as_deref().unwrap_or(&round.ticket).into(),
                ticket: round.ticket.as_str().into(),
                meta: meta.into(),
                connected: round.connected,
                new: round.new,
            });
        }
    }
    rows
}

/// The config the settings fields describe, or what is wrong with them.
fn form_config(dialog: &SettingsDialog) -> Result<Config, String> {
    let port = dialog
        .get_port()
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|&p| p != 0)
        .ok_or("The port must be a number between 1 and 65535.")?;
    let token = dialog.get_token();
    let token = token.trim();
    let config = Config {
        host: dialog.get_host().trim().to_owned(),
        port,
        bearer_token: (!token.is_empty()).then(|| token.to_owned()),
    };
    config::validate(&config)?;
    Ok(config)
}

fn set_warnings(dialog: &SettingsDialog, error: Option<String>) {
    let mut warnings: Vec<SharedString> = Vec::new();
    if !is_loopback(&dialog.get_host()) {
        let mut warning = "Other machines can reach this address.".to_owned();
        if dialog.get_token().trim().is_empty() {
            warning.push_str(" Set an access token.");
        }
        warnings.push(warning.into());
    }
    warnings.extend(error.map(SharedString::from));
    dialog.set_warnings(ModelRc::new(VecModel::from(warnings)));
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

/// Opens a link from a Markdown body in the default browser.
fn open_url(url: &str) -> std::io::Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err(std::io::Error::other(
            "only http(s) and mailto links are opened",
        ));
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
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

/// Window size, zoom and view choices, next to `config.toml` as `window.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct WindowState {
    size: Option<(f32, f32)>,
    zoom: f32,
    appearance: i32,
    sidebar_visible: bool,
    sidebar_width: f32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            size: None,
            zoom: 1.0,
            appearance: 0,
            sidebar_visible: true,
            sidebar_width: 260.0,
        }
    }
}

impl WindowState {
    fn path() -> Option<PathBuf> {
        config::path().map(|p| p.with_file_name("window.json"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = Self::path() else { return };
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, serde_json::to_string_pretty(self).unwrap()));
        if let Err(e) = result {
            log::warn!("cannot save {}: {e}", path.display());
        }
    }
}
