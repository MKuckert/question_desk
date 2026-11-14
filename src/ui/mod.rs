//! The Question Desk window: renders the `desk` read model and dispatches user intents.

mod appearance;
mod grapheme;
#[cfg(target_os = "macos")]
mod macos_text_input;
#[cfg(test)]
mod screenshots;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Image, Key, KeyboardShortcut, Modifiers, RichText,
    ScrollArea, Sense, Shadow, Stroke, TextEdit,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_elements::{
    Badge, Button, HoldBurst, Label, Modal, egui_lucide::Lucide, theme::ThemeColors,
    visuals::FrameVisuals,
};

use appearance::Appearance;

use crate::desk::{
    Desk, DeskEvent, OptionView, QuestionState, QuestionView, RoundView, SessionView, Step,
};
use grapheme::GraphemeText;

const ZOOM_KEY: &str = "zoom_factor";
const THEME_KEY: &str = "theme";
/// Below this width of the page the preview panel moves under the Options.
const WIDE_LAYOUT: f32 = 720.0;

/// Starts the window and blocks until it is closed. `status` is shown in the empty state,
/// e.g. the MCP URL or why the server could not start.
pub fn run(desk: Desk, status: String) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_min_inner_size([520.0, 400.0])
            .with_title("Question Desk"),
        ..Default::default()
    };
    eframe::run_native(
        "question-desk",
        options,
        Box::new(move |cc| {
            #[cfg(target_os = "macos")]
            macos_text_input::install(&cc.egui_ctx);
            let zoom = cc
                .storage
                .and_then(|s| s.get_string(ZOOM_KEY))
                .and_then(|z| z.parse::<f32>().ok())
                .unwrap_or(1.0);
            cc.egui_ctx.set_zoom_factor(zoom);
            let theme = cc.storage.and_then(|s| s.get_string(THEME_KEY));
            Ok(Box::new(App::new(
                &cc.egui_ctx,
                desk,
                status,
                theme.as_deref(),
            )))
        }),
    )
}

/// The Option card under the mouse: Ticket, Question index, Option index.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HoveredOption {
    ticket: String,
    question: usize,
    option: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteFocus {
    Enter,
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialog {
    Cancel,
    SubmitUnanswered,
    Close,
}

struct App {
    desk: Desk,
    status: String,
    /// Set by the desk when a new round arrives.
    attention: Arc<AtomicBool>,
    /// The window was raised above others last frame and must go back to normal.
    restore_window_level: bool,
    sidebar_open: bool,
    dialog: Option<Dialog>,
    /// The user confirmed closing; let the next close request through.
    closing: bool,
    /// The Option card under the mouse, for the preview panel.
    hovered_option: Option<HoveredOption>,
    /// Focus change for the Note field, decided before egui handles Tab/Esc itself.
    note_focus: Option<NoteFocus>,
    markdown: CommonMarkCache,
    /// Last seen zoom factor (`Cmd +/-`), saved with the window size.
    zoom: Option<f32>,
    appearance: Appearance,
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        #[cfg(target_os = "macos")]
        raw_input.events.extend(
            macos_text_input::take_pending()
                .into_iter()
                .map(egui::Event::Text),
        );
        self.take_note_keys(ctx, raw_input);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Window size is persisted by eframe; the zoom we keep ourselves.
        if let Some(zoom) = self.zoom {
            storage.set_string(ZOOM_KEY, zoom.to_string());
        }
        storage.set_string(THEME_KEY, self.appearance.theme.kind.to_str().to_owned());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl App {
    /// `theme` is the stored [`egui_elements::ThemeKind`] name; Tokyo Night if absent.
    fn new(ctx: &egui::Context, desk: Desk, status: String, theme: Option<&str>) -> Self {
        egui_extras::install_image_loaders(ctx);
        let appearance = Appearance::new(ctx, theme);
        let attention = Arc::new(AtomicBool::new(false));
        let repaint = ctx.clone();
        let flag = attention.clone();
        desk.on_change(move |event| {
            if event == DeskEvent::RoundOpened {
                flag.store(true, Ordering::Relaxed);
            }
            repaint.request_repaint();
        });
        Self {
            desk,
            status,
            attention,
            restore_window_level: false,
            sidebar_open: true,
            dialog: None,
            closing: false,
            hovered_option: None,
            note_focus: None,
            markdown: CommonMarkCache::default(),
            zoom: None,
            appearance,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.zoom = Some(ctx.zoom_factor());
        self.request_attention(&ctx);
        self.handle_close_request(&ctx);

        let sessions = self.desk.sessions();
        let round = self.desk.current();

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui, &sessions, round.as_ref()));
        if let Some(round) = &round {
            egui::Panel::bottom("footer").show(ui, |ui| self.footer(ui, round));
        }
        let mut sidebar_open = self.sidebar_open;
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(240.0)
            .show_collapsible(ui, &mut sidebar_open, |ui| {
                self.sidebar(ui, &sessions, round.as_ref())
            });
        self.sidebar_open = sidebar_open;
        egui::CentralPanel::default().show(ui, |ui| match &round {
            None => self.empty_state(ui),
            Some(round) => match round.step {
                Step::Question(i) => self.question_page(ui, round, i),
                Step::Review => self.review_page(ui, round),
            },
        });

        if let Some(round) = &round {
            self.shortcuts(&ctx, round);
        }
        self.dialogs(&ctx, round.as_ref());
        self.appearance.show(ui);
    }

    /// On a new round: bring the window to the front and ask for attention (dock bounce,
    /// taskbar flash) without taking keyboard focus.
    fn request_attention(&mut self, ctx: &egui::Context) {
        if self.restore_window_level {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            self.restore_window_level = false;
        }
        if self.attention.swap(false, Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            self.restore_window_level = true;
            ctx.request_repaint();
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.closing
            && !self.desk.sessions().is_empty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Close);
        }
    }

    // ---- Chrome ---------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui, sessions: &[SessionView], round: Option<&RoundView>) {
        let colors = self.appearance.colors();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let (icon, hint) = if self.sidebar_open {
                (Lucide::PanelLeftClose, "Hide open rounds")
            } else {
                (Lucide::PanelLeftOpen, "Show open rounds")
            };
            let mut toggle = Button::new(icon_image(icon, colors.text));
            let unseen = sessions
                .iter()
                .flat_map(|s| &s.rounds)
                .filter(|r| r.new)
                .count();
            if unseen > 0 {
                toggle = toggle.badge(Badge::new(unseen.to_string()));
            }
            if ui.add(toggle).on_hover_text(hint).clicked() {
                self.sidebar_open = !self.sidebar_open;
            }
            match round {
                None => {
                    ui.label(RichText::new("Question Desk").strong());
                }
                Some(round) => {
                    ui.label(RichText::new(&round.session).color(colors.text_muted));
                    if let Some(title) = &round.title {
                        ui.label(RichText::new(title).strong());
                    }
                    if !round.connected {
                        ui.add(Label::new(
                            RichText::new("Agent disconnected").color(colors.warning),
                            Some(icon_image(Lucide::TriangleAlert, colors.warning)),
                        ))
                        .on_hover_text(
                            "No Agent call is waiting for this round. Your answers are kept; \
                             the Agent can pick them up with its ticket.",
                        );
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(Button::new(icon_image(Lucide::Settings, colors.text)))
                    .on_hover_text("Appearance")
                    .clicked()
                {
                    self.appearance.window_open = !self.appearance.window_open;
                }
                let (icon, hint) = if self.appearance.theme.dark {
                    (Lucide::Sun, "Switch to light mode")
                } else {
                    (Lucide::Moon, "Switch to dark mode")
                };
                if ui
                    .add(Button::new(icon_image(icon, colors.text)))
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.appearance.toggle_dark(ui.ctx());
                }
                let Some(round) = round else { return };
                ui.add_space(8.0);
                match round.step {
                    Step::Question(i) => {
                        ui.label(
                            RichText::new(format!("{} / {}", i + 1, round.questions.len()))
                                .strong(),
                        );
                        chip(ui, &round.questions[i].header, colors.accent);
                    }
                    Step::Review => {
                        ui.label(RichText::new("Review").strong());
                    }
                }
            });
        });
        ui.add_space(2.0);
    }

    fn sidebar(
        &mut self,
        ui: &mut egui::Ui,
        sessions: &[SessionView],
        current: Option<&RoundView>,
    ) {
        let colors = self.appearance.colors();
        ui.add_space(8.0);
        ui.label(RichText::new("Open rounds").strong());
        ui.separator();
        ScrollArea::vertical().show(ui, |ui| {
            if sessions.is_empty() {
                ui.label(RichText::new("None").color(colors.text_muted));
            }
            for session in sessions {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&session.label)
                        .small()
                        .strong()
                        .color(colors.text_muted),
                );
                for round in &session.rounds {
                    let selected = current.is_some_and(|c| c.ticket == round.ticket);
                    let name = round.title.as_deref().unwrap_or(&round.ticket);
                    let mut text = format!(
                        "{name}\n{}/{} answered · {}",
                        round.answered,
                        round.total,
                        age(round.opened_at)
                    );
                    if !round.connected {
                        text.push_str(" · ⚠ disconnected");
                    }
                    let mut button = Button::selectable(selected, text)
                        .wrap()
                        .min_size(egui::vec2(ui.available_width(), 0.0));
                    if round.new {
                        button =
                            button.badge(Badge::dot().min_diameter(9.0).padding(0.0).inset(8.0));
                    }
                    if ui.add(button).clicked() {
                        self.desk.select(&round.ticket);
                    }
                }
            }
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let colors = self.appearance.colors();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.add(Image::new(
                Lucide::Inbox
                    .size(48.0)
                    .stroke_width(1.5)
                    .color(colors.accent)
                    .source(),
            ));
            ui.add_space(8.0);
            ui.heading("No open questions");
            ui.add_space(8.0);
            ui.label(RichText::new(&self.status).color(colors.text_muted));
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        let colors = self.appearance.colors();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let cancel = Button::new((
                icon_image(Lucide::X, colors.error),
                RichText::new("Cancel round").color(colors.error),
            ));
            if ui.add(cancel).clicked() {
                self.dialog = Some(Dialog::Cancel);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match round.step {
                    Step::Question(_) => {
                        let next = Button::new((
                            RichText::new("Next").strong().color(colors.bg),
                            icon_image(Lucide::ChevronRight, colors.bg),
                        ))
                        .bg_color(colors.accent);
                        if ui.add(next).clicked() {
                            self.desk.next();
                        }
                    }
                    Step::Review => {
                        let submit = Button::new((
                            icon_image(Lucide::Send, colors.bg),
                            RichText::new("Submit").strong().color(colors.bg),
                        ))
                        .bg_color(colors.success);
                        if ui.add(submit).clicked() {
                            self.submit(round);
                        }
                    }
                }
                let at_start = round.step == Step::Question(0);
                let back = Button::new((icon_image(Lucide::ChevronLeft, colors.text), "Back"));
                if ui.add_enabled(!at_start, back).clicked() {
                    self.desk.back();
                }
            });
        });
        let hint = match round.step {
            Step::Question(_) => {
                "1–9 select · Tab note · ↩ accept recommendation · ⌘↩ next · ⌘⇧↩ back · Esc leave note / cancel"
            }
            Step::Review => "⌘↩ submit · ⌘⇧↩ back · Esc cancel",
        };
        ui.label(
            RichText::new(hint.replace('⌘', cmd_symbol()))
                .small()
                .color(colors.text_muted),
        );
        ui.add_space(4.0);
    }

    fn step_bar(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        let colors = self.appearance.colors();
        let size = egui::vec2(34.0, 28.0);
        ui.horizontal_wrapped(|ui| {
            for (i, question) in round.questions.iter().enumerate() {
                let number = RichText::new((i + 1).to_string());
                let button = match question.state {
                    QuestionState::Current => Button::new(number.strong()).selected(true),
                    QuestionState::Answered => Button::new(number.color(colors.bg))
                        .bg_color(colors.success.gamma_multiply(0.85)),
                    QuestionState::Unanswered => Button::new(number.color(colors.text_muted)),
                };
                if ui
                    .add(button.min_size(size).label_align(egui::Align::Center))
                    .on_hover_text(&question.question)
                    .clicked()
                {
                    self.desk.go_to(i);
                }
            }
            // The Review step carries the number of Unanswered Questions as a badge.
            let unanswered = round.unanswered().len();
            // Room on the right for the badge, which is painted over the corner.
            let mut review = Button::new("Review")
                .selected(round.step == Step::Review)
                .min_size(egui::vec2(104.0, size.y));
            if unanswered > 0 {
                review = review.badge(Badge::new(unanswered.to_string()).min_diameter(14.0));
            }
            if ui.add(review).clicked() {
                self.desk.review();
            }
        });
    }

    // ---- Question page --------------------------------------------------------------------

    fn question_page(&mut self, ui: &mut egui::Ui, round: &RoundView, index: usize) {
        let question = &round.questions[index];
        self.step_bar(ui, round);
        ui.add_space(8.0);
        ScrollArea::vertical()
            .id_salt(("page", &round.ticket, index))
            .show(ui, |ui| {
                ui.label(RichText::new(&question.question).heading().strong());
                ui.add_space(4.0);
                if let Some(body) = &question.body {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt(("body", &round.ticket, index))
                            .max_height((ui.ctx().content_rect().height() * 0.35).max(120.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                CommonMarkViewer::new().show(ui, &mut self.markdown, body);
                            });
                    });
                    ui.add_space(6.0);
                }
                if let Some(recommendation) = &question.recommendation_text {
                    ui.horizontal_wrapped(|ui| {
                        chip(ui, "Recommended", self.appearance.colors().success);
                        ui.label(recommendation);
                    });
                    ui.add_space(4.0);
                }
                self.options_and_preview(ui, round, question, index);
                ui.add_space(8.0);
                self.note_field(ui, round, question, index);
            });
    }

    fn options_and_preview(
        &mut self,
        ui: &mut egui::Ui,
        round: &RoundView,
        question: &QuestionView,
        index: usize,
    ) {
        if question.options.is_empty() {
            ui.label(
                RichText::new("Open question: answer in the note below.")
                    .color(self.appearance.colors().text_muted),
            );
            return;
        }
        let preview = self.preview(round, question, index);
        // The layout must not depend on what is hovered: switching to columns moves and
        // re-identifies the cards, which drops the hover and switches back, every frame.
        let has_previews = question.options.iter().any(|o| o.preview.is_some());
        let wide = ui.available_width() >= WIDE_LAYOUT;
        if has_previews && wide {
            ui.columns(2, |columns| {
                self.option_cards(&mut columns[0], round, question, index);
                preview_panel(&mut columns[1], preview.as_deref());
            });
        } else {
            self.option_cards(ui, round, question, index);
            if let Some(preview) = &preview {
                ui.add_space(6.0);
                preview_panel(ui, Some(preview));
            }
        }
    }

    /// The preview of the hovered Option, else of the first selected one that has one.
    fn preview(&self, round: &RoundView, question: &QuestionView, index: usize) -> Option<String> {
        let hovered = self
            .hovered_option
            .as_ref()
            .filter(|h| h.ticket == round.ticket && h.question == index)
            .and_then(|h| question.options.get(h.option))
            .and_then(|o| o.preview.clone());
        hovered.or_else(|| {
            question
                .options
                .iter()
                .filter(|o| o.selected)
                .find_map(|o| o.preview.clone())
        })
    }

    fn option_cards(
        &mut self,
        ui: &mut egui::Ui,
        round: &RoundView,
        question: &QuestionView,
        index: usize,
    ) {
        let mut hovered = None;
        for (i, option) in question.options.iter().enumerate() {
            let response = option_card(
                ui,
                &self.appearance.colors(),
                option,
                i,
                question.multi_select,
            );
            if response.hovered() {
                hovered = Some(i);
            }
            if response.clicked() {
                self.desk.toggle_option(i);
            }
            ui.add_space(4.0);
        }
        let on_this_question = |h: &HoveredOption| h.ticket == round.ticket && h.question == index;
        if hovered.is_some() || self.hovered_option.as_ref().is_some_and(on_this_question) {
            self.hovered_option = hovered.map(|option| HoveredOption {
                ticket: round.ticket.clone(),
                question: index,
                option,
            });
        }
    }

    fn note_field(
        &mut self,
        ui: &mut egui::Ui,
        round: &RoundView,
        question: &QuestionView,
        index: usize,
    ) {
        let hint = if question.options.is_empty() {
            "Your answer"
        } else if question.options.iter().any(|o| o.selected) {
            "Note (optional): caveats, reasons, “yes, but …”"
        } else {
            "Note: without a selected option, this text is your answer"
        };
        ui.label(RichText::new("Note").strong());
        let mut note = question.note.clone();
        let mut buffer = GraphemeText(&mut note);
        let edit = TextEdit::multiline(&mut buffer)
            .id(note_id(&round.ticket, index))
            .hint_text(hint)
            .desired_rows(2)
            .desired_width(f32::INFINITY);
        let response = grapheme::show(ui, edit);
        if response.changed() {
            self.desk.set_note(&note);
        }
    }

    // ---- Review page ----------------------------------------------------------------------

    fn review_page(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        let colors = self.appearance.colors();
        self.step_bar(ui, round);
        ui.add_space(8.0);
        ui.heading("Review your answers");
        let unanswered = round.unanswered().len();
        if unanswered > 0 {
            ui.label(
                RichText::new(format!("{unanswered} question(s) unanswered")).color(colors.warning),
            );
        }
        ui.add_space(6.0);
        ScrollArea::vertical()
            .id_salt(("review", &round.ticket))
            .show(ui, |ui| {
                for (i, question) in round.questions.iter().enumerate() {
                    let warn = question.answer.is_none().then_some(colors.warning);
                    let response = egui_elements::Frame::new()
                        .visuals(card_visuals(&colors, warn))
                        .inner_margin(12)
                        .interactive(true)
                        .sense(Sense::click())
                        .fill_width(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("{}.", i + 1)).color(colors.text_muted),
                                );
                                chip(ui, &question.header, colors.accent);
                                ui.label(RichText::new(&question.question).strong());
                            });
                            match &question.answer {
                                Some(answer) => {
                                    ui.label(answer);
                                }
                                None => {
                                    ui.label(
                                        RichText::new("Unanswered").italics().color(colors.warning),
                                    );
                                }
                            }
                            if let Some(note) = &question.answer_note {
                                ui.label(
                                    RichText::new(format!("Note: {note}")).color(colors.text_muted),
                                );
                            }
                        })
                        .response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Edit this answer");
                    if response.clicked() {
                        self.desk.go_to(i);
                    }
                    ui.add_space(4.0);
                }
            });
    }

    fn submit(&mut self, round: &RoundView) {
        if round.unanswered().is_empty() {
            self.desk.submit();
        } else {
            self.dialog = Some(Dialog::SubmitUnanswered);
        }
    }

    // ---- Keyboard -------------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context, round: &RoundView) {
        if self.dialog.is_some() {
            return;
        }
        let note_focused = match round.step {
            Step::Question(i) => ctx.memory(|m| m.has_focus(note_id(&round.ticket, i))),
            Step::Review => false,
        };
        let back = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter);
        let next = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        if ctx.input_mut(|i| i.consume_shortcut(&back)) {
            self.desk.back();
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&next)) {
            match round.step {
                Step::Question(_) => self.desk.next(),
                Step::Review => self.submit(round),
            }
            return;
        }
        let Step::Question(index) = round.step else {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.dialog = Some(Dialog::Cancel);
            }
            return;
        };
        let note = note_id(&round.ticket, index);
        match self.note_focus.take() {
            Some(NoteFocus::Enter) => ctx.memory_mut(|m| m.request_focus(note)),
            Some(NoteFocus::Leave) => ctx.memory_mut(|m| m.surrender_focus(note)),
            None => {}
        }
        if note_focused {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.dialog = Some(Dialog::Cancel);
            return;
        }
        // Enter accepts the Recommendation only while nothing is selected or noted.
        let question = &round.questions[index];
        let accepts = question.answer.is_none()
            && (question.recommendation_text.is_some()
                || question.options.iter().any(|o| o.recommended));
        if accepts && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            self.desk.accept_recommendation();
            return;
        }
        const DIGITS: [Key; 9] = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (i, key) in DIGITS.into_iter().enumerate() {
            if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, key)) {
                self.desk.toggle_option(i);
            }
        }
    }

    /// Tab moves into the Note and Esc leaves it. egui would otherwise move focus to
    /// the next widget on Tab, and clear focus on Esc before `shortcuts` can tell that
    /// Esc was meant for the Note rather than for cancelling the round.
    fn take_note_keys(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.dialog.is_some() {
            return;
        }
        let Some(round) = self.desk.current() else {
            return;
        };
        let Step::Question(index) = round.step else {
            return;
        };
        let note_focused = ctx.memory(|m| m.has_focus(note_id(&round.ticket, index)));
        let (key, focus) = if note_focused {
            (Key::Escape, NoteFocus::Leave)
        } else {
            (Key::Tab, NoteFocus::Enter)
        };
        let is_key = |event: &egui::Event| matches!(event, egui::Event::Key { key: k, modifiers, .. } if *k == key && modifiers.is_none());
        if raw_input.events.iter().any(is_key) {
            raw_input.events.retain(|e| !is_key(e));
            self.note_focus = Some(focus);
        }
    }

    // ---- Dialogs --------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context, round: Option<&RoundView>) {
        let Some(dialog) = self.dialog else { return };
        let (title, text, confirm) = match dialog {
            Dialog::Cancel => (
                "Cancel this round?",
                "The Agent receives the answers you gave so far, marked as cancelled.".to_owned(),
                "Cancel round",
            ),
            Dialog::SubmitUnanswered => (
                "Submit with unanswered questions?",
                format!(
                    "{} question(s) have no answer. The Agent is told which ones.",
                    round.map_or(0, |r| r.unanswered().len())
                ),
                "Submit anyway",
            ),
            Dialog::Close => (
                "Close the Question Desk?",
                "All open rounds are cancelled; their Agents receive the answers given so far."
                    .to_owned(),
                "Cancel rounds and quit",
            ),
        };
        let colors = self.appearance.colors();
        // Cancelling throws away the Agent's wait: hold to confirm. Submitting is harmless.
        let destructive = dialog != Dialog::SubmitUnanswered;
        let mut decision = None;
        let mut open = true;
        let entered = Modal::new("dialog", &mut open)
            .heading(title)
            .max_width(440.0)
            .alert(destructive)
            .footer(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let confirm = if destructive {
                        Button::new(
                            RichText::new(format!("Hold to {}", confirm.to_lowercase()))
                                .strong()
                                .color(colors.error),
                        )
                        .hold(1.0)
                        .hold_fill(colors.error.gamma_multiply(0.45))
                        .hold_burst(HoldBurst::Glow)
                    } else {
                        Button::new(RichText::new(confirm).strong().color(colors.bg))
                            .bg_color(colors.accent)
                    };
                    if ui.add(confirm).clicked() {
                        decision = Some(true);
                    }
                    if ui.add(Button::new("Keep answering")).clicked() {
                        decision = Some(false);
                    }
                });
            })
            .show(ctx, |ui| {
                ui.label(text);
                // Enter confirms only the harmless dialog; cancelling needs a hold.
                !destructive && ui.input(|i| i.key_pressed(Key::Enter))
            });
        if entered == Some(true) {
            decision = Some(true);
        }
        if !open && decision.is_none() {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                self.dialog = None;
                match dialog {
                    Dialog::Cancel => self.desk.cancel(),
                    Dialog::SubmitUnanswered => self.desk.submit(),
                    Dialog::Close => {
                        self.desk.cancel_all();
                        self.closing = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            Some(false) => self.dialog = None,
            None => {}
        }
    }
}

/// Card chrome for Option cards and Review entries: `accent` marks the card (selected or
/// unanswered) with a thicker border in that color.
fn card_visuals(colors: &ThemeColors, accent: Option<Color32>) -> FrameVisuals {
    let border = match accent {
        Some(color) => Stroke::new(2.0, color),
        None => Stroke::new(1.0, colors.border),
    };
    FrameVisuals {
        bg: colors.widget_bg,
        bg_hover: colors.hover,
        bg_click: colors.highlight,
        bg_selected: colors.highlight,
        border,
        border_hover: Stroke::new(border.width, accent.unwrap_or(colors.accent)),
        border_click: Stroke::new(border.width, colors.accent),
        corner_radius: CornerRadius::same(10),
        shadow: Shadow::NONE,
    }
}

fn option_card(
    ui: &mut egui::Ui,
    colors: &ThemeColors,
    option: &OptionView,
    index: usize,
    multi_select: bool,
) -> egui::Response {
    let accent = option.selected.then_some(colors.accent);
    egui_elements::Frame::new()
        .visuals(card_visuals(colors, accent))
        .inner_margin(12)
        .interactive(true)
        .selected(option.selected)
        .sense(Sense::click())
        .fill_width(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mark = match (multi_select, option.selected) {
                    (true, true) => Lucide::SquareCheck,
                    (true, false) => Lucide::Square,
                    (false, true) => Lucide::CircleDot,
                    (false, false) => Lucide::Circle,
                };
                let color = if option.selected {
                    colors.accent
                } else {
                    colors.text_muted
                };
                ui.add(Image::new(mark.size(20.0).color(color).source()));
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        if index < 9 {
                            ui.label(
                                RichText::new((index + 1).to_string())
                                    .monospace()
                                    .color(colors.text_muted),
                            );
                        }
                        ui.label(RichText::new(&option.label).strong());
                        if option.recommended {
                            chip(ui, "Recommended", colors.success);
                        }
                    });
                    if !option.description.is_empty() {
                        ui.label(&option.description);
                    }
                });
            });
        })
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn preview_panel(ui: &mut egui::Ui, preview: Option<&str>) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().extreme_bg_color)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Preview").small().weak());
            ScrollArea::both()
                .id_salt("preview")
                .max_height(420.0)
                .show(ui, |ui| match preview {
                    Some(preview) => {
                        ui.label(RichText::new(preview).font(FontId::monospace(14.0)));
                    }
                    None => {
                        ui.label(RichText::new("Hover an option to see its preview.").weak());
                    }
                });
        });
}

/// A small pill tinted with `color`.
fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(color))
        });
}

/// A Lucide icon sized for buttons and labels.
fn icon_image(icon: Lucide, color: Color32) -> Image<'static> {
    Image::new(icon.size(18.0).color(color).source()).fit_to_exact_size(egui::vec2(18.0, 18.0))
}

fn note_id(ticket: &str, index: usize) -> egui::Id {
    egui::Id::unique(("note", ticket, index))
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
