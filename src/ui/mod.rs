//! The Question Desk window: renders the `desk` read model and dispatches user intents.

mod grapheme;
mod icons;
#[cfg(target_os = "macos")]
mod macos_text_input;
mod theme;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use eframe::egui::{
    self, Color32, FontId, Frame, Key, KeyboardShortcut, Margin, Modifiers, RichText, ScrollArea,
    Sense, Stroke, TextEdit, class::HasClasses as _,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::desk::{
    Desk, DeskEvent, OptionView, QuestionState, QuestionView, RoundSummary, RoundView, SessionView,
    Step,
};
use grapheme::GraphemeText;
use icons::{button_icon, icon};
use theme::{SPACE_L, SPACE_M, SPACE_S, SPACE_XS};

const ZOOM_KEY: &str = "zoom_factor";
/// Below this width of the page the preview panel moves under the Options.
const WIDE_LAYOUT: f32 = 720.0;
/// The widest the page content gets.
const PAGE_WIDTH: f32 = 960.0;

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
            theme::install(&cc.egui_ctx);

            let attention = Arc::new(AtomicBool::new(false));
            let ctx = cc.egui_ctx.clone();
            let flag = attention.clone();
            desk.on_change(move |event| {
                if event == DeskEvent::RoundOpened {
                    flag.store(true, Ordering::Relaxed);
                }
                ctx.request_repaint();
            });
            Ok(Box::new(App {
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
            }))
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
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl App {
    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.zoom = Some(ctx.zoom_factor());
        self.request_attention(&ctx);
        self.handle_close_request(&ctx);

        let sessions = self.desk.sessions();
        let round = self.desk.current();

        let p = theme::of(ui);
        let bar = Frame::new()
            .fill(p.panel)
            .inner_margin(Margin::symmetric(SPACE_L as i8, SPACE_S as i8));
        egui::Panel::top("top")
            .frame(bar)
            .show(ui, |ui| self.top_bar(ui, round.as_ref()));
        if let Some(round) = &round {
            egui::Panel::bottom("footer")
                .frame(bar.inner_margin(Margin::symmetric(SPACE_L as i8, SPACE_M as i8)))
                .show(ui, |ui| self.footer(ui, round));
        }
        let mut sidebar_open = self.sidebar_open;
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(260.0)
            .min_size(200.0)
            .frame(Frame::new().fill(p.panel).inner_margin(SPACE_M))
            .show_collapsible(ui, &mut sidebar_open, |ui| {
                self.sidebar(ui, &sessions, round.as_ref())
            });
        self.sidebar_open = sidebar_open;
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(0, SPACE_L as i8)),
            )
            .show(ui, |ui| {
                page_column(ui, |ui| match &round {
                    None => self.empty_state(ui),
                    Some(round) => match round.step {
                        Step::Question(i) => self.question_page(ui, round, i),
                        Step::Review => self.review_page(ui, round),
                    },
                })
            });

        if let Some(round) = &round {
            self.shortcuts(&ctx, round);
        }
        self.dialogs(&ctx, round.as_ref());
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

    fn top_bar(&mut self, ui: &mut egui::Ui, round: Option<&RoundView>) {
        let p = theme::of(ui);
        ui.horizontal(|ui| {
            let (toggle, tip) = if self.sidebar_open {
                (icons::PANEL_CLOSE, "Hide open rounds")
            } else {
                (icons::PANEL_OPEN, "Show open rounds")
            };
            if ui
                .add(
                    egui::Button::image(button_icon(toggle))
                        .image_tint_follows_text_color(true)
                        .with_class(theme::GHOST),
                )
                .on_hover_text(tip)
                .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }
            ui.add(icon(icons::LOGO, 20.0, p.accent));
            ui.label(
                RichText::new("Question Desk")
                    .font(theme::semibold(theme::TEXT_BODY))
                    .color(p.text_strong),
            );
            if let Some(round) = round {
                ui.label(RichText::new("/").color(p.border_strong));
                ui.label(RichText::new(&round.session).color(p.muted));
                if let Some(title) = &round.title {
                    ui.label(
                        RichText::new(title)
                            .font(theme::semibold(theme::TEXT_BODY))
                            .color(p.text),
                    );
                }
                if !round.connected {
                    ui.add(icon(icons::WARN, 16.0, p.warn));
                    ui.label(RichText::new("Agent disconnected").color(p.warn))
                        .on_hover_text(
                            "No Agent call is waiting for this round. Your answers are kept; \
                             the Agent can pick them up with its ticket.",
                        );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::global_theme_preference_buttons(ui);
                let Some(round) = round else { return };
                ui.add_space(SPACE_S);
                match round.step {
                    Step::Question(i) => {
                        ui.label(
                            RichText::new(format!("{} / {}", i + 1, round.questions.len()))
                                .font(theme::semibold(theme::TEXT_BUTTON))
                                .color(p.muted),
                        );
                        theme::neutral_chip(ui, &round.questions[i].header);
                    }
                    Step::Review => theme::neutral_chip(ui, "Review"),
                }
            });
        });
    }

    fn sidebar(
        &mut self,
        ui: &mut egui::Ui,
        sessions: &[SessionView],
        current: Option<&RoundView>,
    ) {
        let p = theme::of(ui);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("OPEN ROUNDS")
                    .font(theme::semibold(theme::TEXT_SMALL))
                    .color(p.muted),
            );
            let open: usize = sessions.iter().map(|s| s.rounds.len()).sum();
            if open > 0 {
                theme::neutral_chip(ui, &open.to_string());
            }
        });
        ui.add_space(SPACE_XS);
        ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = SPACE_XS;
            if sessions.is_empty() {
                ui.label(RichText::new("Nothing waiting").color(p.muted));
            }
            for session in sessions {
                ui.add_space(SPACE_S);
                ui.label(
                    RichText::new(&session.label)
                        .size(theme::TEXT_SMALL)
                        .color(p.muted),
                );
                for round in &session.rounds {
                    let selected = current.is_some_and(|c| c.ticket == round.ticket);
                    let text = round_entry(ui, round, selected);
                    let mut button = egui::Button::new(text);
                    if round.new {
                        button = button.right_text(new_badge(ui));
                    }
                    let button = button
                        .selected(selected)
                        .with_class(theme::NAV)
                        .min_size(egui::vec2(ui.available_width(), 0.0));
                    if ui.add(button).clicked() {
                        self.desk.select(&round.ticket);
                    }
                }
            }
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let p = theme::of(ui);
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.add(icon(icons::INBOX, 40.0, p.muted));
            ui.add_space(SPACE_M);
            ui.heading("No open questions");
            ui.add_space(SPACE_S);
            ui.label(RichText::new(&self.status).color(p.muted));
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        ui.horizontal(|ui| {
            let cancel = egui::Button::image_and_text(button_icon(icons::CLOSE), "Cancel round")
                .image_tint_follows_text_color(true)
                .with_class(theme::GHOST);
            if ui.add(cancel).clicked() {
                self.dialog = Some(Dialog::Cancel);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let primary = match round.step {
                    Step::Question(_) => ("Next", icons::NEXT),
                    Step::Review => ("Submit", icons::SEND),
                };
                let button = egui::Button::new((primary.0, button_icon(primary.1)))
                    .image_tint_follows_text_color(true)
                    .with_class(theme::PRIMARY)
                    .min_size(egui::vec2(112.0, 0.0));
                if ui.add(button).clicked() {
                    match round.step {
                        Step::Question(_) => self.desk.next(),
                        Step::Review => self.submit(round),
                    }
                }
                let at_start = round.step == Step::Question(0);
                let back = egui::Button::image_and_text(button_icon(icons::BACK), "Back")
                    .image_tint_follows_text_color(true);
                if ui.add_enabled(!at_start, back).clicked() {
                    self.desk.back();
                }
            });
        });
        ui.add_space(SPACE_XS);
        shortcut_hints(ui, round.step);
    }

    fn step_bar(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_XS + 2.0;
            for (i, question) in round.questions.iter().enumerate() {
                let class = match question.state {
                    QuestionState::Current => Some(theme::STEP_CURRENT),
                    QuestionState::Answered => Some(theme::STEP_DONE),
                    QuestionState::Unanswered => None,
                };
                let button = egui::Button::new((i + 1).to_string())
                    .with_class(theme::STEP)
                    .with_class_if(class.unwrap_or_default(), class.is_some())
                    .min_size(egui::vec2(32.0, 28.0));
                if ui.add(button).on_hover_text(&question.question).clicked() {
                    self.desk.go_to(i);
                }
            }
            let review = round.step == Step::Review;
            let button = egui::Button::new("Review")
                .with_class(theme::STEP)
                .with_class_if(theme::STEP_CURRENT, review)
                .min_size(egui::vec2(32.0, 28.0));
            if ui.add(button).clicked() {
                self.desk.review();
            }
        });
    }

    // ---- Question page --------------------------------------------------------------------

    fn question_page(&mut self, ui: &mut egui::Ui, round: &RoundView, index: usize) {
        let p = theme::of(ui);
        let question = &round.questions[index];
        self.step_bar(ui, round);
        ui.add_space(SPACE_L);
        ScrollArea::vertical()
            .id_salt(("page", &round.ticket, index))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(&question.question)
                        .font(theme::semibold(theme::TEXT_TITLE))
                        .color(p.text_strong),
                );
                ui.add_space(SPACE_S);
                if let Some(body) = &question.body {
                    card_frame(p.surface, Stroke::new(1.0, p.border)).show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt(("body", &round.ticket, index))
                            .max_height((ui.ctx().content_rect().height() * 0.35).max(120.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                CommonMarkViewer::new().show(ui, &mut self.markdown, body);
                            });
                    });
                    ui.add_space(SPACE_M);
                }
                if let Some(recommendation) = &question.recommendation_text {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(icon(icons::SPARKLES, 16.0, p.accent));
                        theme::accent_chip(ui, "Recommended");
                        ui.label(recommendation);
                    });
                    ui.add_space(SPACE_S);
                }
                self.options_and_preview(ui, round, question, index);
                ui.add_space(SPACE_L);
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
                    .color(theme::of(ui).muted),
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
                ui.add_space(SPACE_S);
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
            let response = option_card(ui, option, i, question.multi_select);
            if response.hovered() {
                hovered = Some(i);
            }
            if response.clicked() {
                self.desk.toggle_option(i);
            }
            ui.add_space(SPACE_XS);
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
        ui.label(
            RichText::new("Note")
                .font(theme::semibold(theme::TEXT_BUTTON))
                .color(theme::of(ui).text_strong),
        );
        let mut note = question.note.clone();
        let mut buffer = GraphemeText(&mut note);
        let edit = TextEdit::multiline(&mut buffer)
            .id(note_id(&round.ticket, index))
            .hint_text(hint)
            .desired_rows(3)
            .margin(Margin::same(SPACE_M as i8))
            .desired_width(f32::INFINITY);
        let response = grapheme::show(ui, edit);
        if response.changed() {
            self.desk.set_note(&note);
        }
    }

    // ---- Review page ----------------------------------------------------------------------

    fn review_page(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        let p = theme::of(ui);
        self.step_bar(ui, round);
        ui.add_space(SPACE_L);
        ui.heading("Review your answers");
        let unanswered = round.unanswered().len();
        if unanswered > 0 {
            ui.horizontal(|ui| {
                ui.add(icon(icons::WARN, 16.0, p.warn));
                ui.label(
                    RichText::new(format!("{unanswered} question(s) unanswered")).color(p.warn),
                );
            });
        }
        ui.add_space(SPACE_S);
        ScrollArea::vertical()
            .id_salt(("review", &round.ticket))
            .show(ui, |ui| {
                for (i, question) in round.questions.iter().enumerate() {
                    let answered = question.answer.is_some();
                    let response = clickable_card(
                        ui,
                        |hovered| {
                            let border = match (answered, hovered) {
                                (false, _) => p.warn,
                                (true, true) => p.border_strong,
                                (true, false) => p.border,
                            };
                            let fill = if hovered { p.hover } else { p.surface };
                            (fill, Stroke::new(1.0, border))
                        },
                        |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let (mark, color) = if answered {
                                    (icons::DONE, p.success)
                                } else {
                                    (icons::WARN, p.warn)
                                };
                                ui.add(icon(mark, 16.0, color));
                                ui.label(RichText::new(format!("{}.", i + 1)).color(p.muted));
                                theme::neutral_chip(ui, &question.header);
                                ui.label(
                                    RichText::new(&question.question)
                                        .font(theme::semibold(theme::TEXT_BODY))
                                        .color(p.text_strong),
                                );
                            });
                            ui.add_space(SPACE_XS);
                            match &question.answer {
                                Some(answer) => {
                                    ui.label(answer);
                                }
                                None => {
                                    ui.label(RichText::new("Unanswered").italics().color(p.warn));
                                }
                            }
                            if let Some(note) = &question.answer_note {
                                ui.label(RichText::new(format!("Note: {note}")).color(p.muted));
                            }
                        },
                    )
                    .on_hover_text("Edit this answer");
                    if response.clicked() {
                        self.desk.go_to(i);
                    }
                    ui.add_space(SPACE_XS);
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
        let mut decision = None;
        let confirm_class = match dialog {
            Dialog::SubmitUnanswered => theme::PRIMARY,
            Dialog::Cancel | Dialog::Close => theme::DANGER,
        };
        let style = ctx.global_style();
        let frame = Frame::window(&style).inner_margin(theme::SPACE_L + theme::SPACE_S);
        let modal = egui::Modal::new(egui::Id::unique("dialog"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading(title);
                ui.add_space(SPACE_XS);
                ui.label(RichText::new(text).color(theme::of(ui).muted));
                ui.add_space(SPACE_L);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(confirm).with_class(confirm_class))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Keep answering").clicked() {
                        decision = Some(false);
                    }
                });
                // Enter confirms only the harmless dialog; cancelling needs a click.
                if dialog == Dialog::SubmitUnanswered && ui.input(|i| i.key_pressed(Key::Enter)) {
                    decision = Some(true);
                }
            });
        if modal.should_close() && decision.is_none() {
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

/// The page content: centered and at most `PAGE_WIDTH` wide, so long lines stay readable.
fn page_column(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let side = ((ui.available_width() - PAGE_WIDTH) / 2.0).max(SPACE_L * 2.0);
    let rect = ui
        .available_rect_before_wrap()
        .shrink2(egui::vec2(side, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add_contents);
}

fn card_frame(fill: Color32, stroke: Stroke) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(theme::RADIUS)
        .inner_margin(SPACE_M)
}

/// A full-width card that reacts to the mouse; `style` maps "hovered" to fill and border.
fn clickable_card(
    ui: &mut egui::Ui,
    style: impl Fn(bool) -> (Color32, Stroke),
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let (fill, stroke) = style(false);
    let mut prepared = card_frame(fill, stroke).begin(ui);
    prepared
        .content_ui
        .set_width(prepared.content_ui.available_width());
    add_contents(&mut prepared.content_ui);
    let response = prepared.allocate_space(ui).interact(Sense::click());
    (prepared.frame.fill, prepared.frame.stroke) = style(response.hovered());
    prepared.paint(ui);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn option_card(
    ui: &mut egui::Ui,
    option: &OptionView,
    index: usize,
    multi_select: bool,
) -> egui::Response {
    let p = theme::of(ui);
    let selected = option.selected;
    clickable_card(
        ui,
        |hovered| match (selected, hovered) {
            (true, _) => (theme::tint(p.accent, 0.12), Stroke::new(1.5, p.accent)),
            (false, true) => (p.hover, Stroke::new(1.0, p.border_strong)),
            (false, false) => (p.surface, Stroke::new(1.0, p.border)),
        },
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_M;
                selection_mark(ui, multi_select, selected);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = SPACE_XS;
                    ui.horizontal_wrapped(|ui| {
                        if index < 9 {
                            key_badge(ui, &(index + 1).to_string());
                        }
                        ui.label(
                            RichText::new(&option.label)
                                .font(theme::semibold(theme::TEXT_BODY))
                                .color(p.text_strong),
                        );
                        if option.recommended {
                            theme::accent_chip(ui, "Recommended");
                        }
                    });
                    if !option.description.is_empty() {
                        ui.label(RichText::new(&option.description).color(p.text));
                    }
                });
            });
        },
    )
}

/// A radio dot or checkbox, painted in the accent when selected.
fn selection_mark(ui: &mut egui::Ui, multi_select: bool, selected: bool) {
    let p = theme::of(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let painter = ui.painter();
    let (fill, stroke) = if selected {
        (p.accent, Stroke::new(1.5, p.accent))
    } else {
        (p.surface, Stroke::new(1.5, p.border_strong))
    };
    if multi_select {
        painter.rect(rect, 5.0, fill, stroke, egui::StrokeKind::Inside);
        if selected {
            icon(icons::CHECK, 14.0, p.on_accent).paint_at(ui, rect.shrink(3.0));
        }
    } else {
        painter.circle(rect.center(), 9.25, fill, stroke);
        if selected {
            painter.circle_filled(rect.center(), 4.0, p.on_accent);
        }
    }
}

/// A key, e.g. the digit that selects an Option, styled as a key cap.
fn key_badge(ui: &mut egui::Ui, key: &str) {
    let p = theme::of(ui);
    Frame::new()
        .fill(p.sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(6, 0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(key)
                    .font(FontId::proportional(theme::TEXT_SMALL))
                    .color(p.muted),
            )
        });
}

/// The keyboard shortcuts of the current step: key caps with a short label, wrapping in narrow
/// windows.
fn shortcut_hints(ui: &mut egui::Ui, step: Step) {
    let cmd = cmd_symbol();
    let hints: &[(&str, &str)] = match step {
        Step::Question(_) => &[
            ("1–9", "select"),
            ("Tab", "note"),
            ("↩", "accept recommendation"),
            ("⌘↩", "next"),
            ("⌘⇧↩", "back"),
            ("Esc", "leave note / cancel"),
        ],
        Step::Review => &[("⌘↩", "submit"), ("⌘⇧↩", "back"), ("Esc", "cancel")],
    };
    let p = theme::of(ui);
    ui.horizontal_wrapped(|ui| {
        for (key, label) in hints {
            ui.spacing_mut().item_spacing.x = SPACE_XS;
            key_badge(ui, &key.replace('⌘', cmd));
            ui.label(RichText::new(*label).size(theme::TEXT_SMALL).color(p.muted));
            ui.add_space(SPACE_M - SPACE_XS);
        }
    });
}

fn preview_panel(ui: &mut egui::Ui, preview: Option<&str>) {
    let p = theme::of(ui);
    card_frame(p.sunken, Stroke::new(1.0, p.border)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add(icon(icons::EYE, 14.0, p.muted));
            ui.label(
                RichText::new("PREVIEW")
                    .font(theme::semibold(theme::TEXT_SMALL))
                    .color(p.muted),
            );
        });
        ScrollArea::both()
            .id_salt("preview")
            .max_height(420.0)
            .show(ui, |ui| match preview {
                Some(preview) => {
                    ui.label(
                        RichText::new(preview)
                            .font(FontId::monospace(theme::TEXT_MONO))
                            .color(p.text),
                    );
                }
                None => {
                    ui.label(RichText::new("Hover an option to see its preview.").color(p.muted));
                }
            });
    });
}

/// A sidebar entry: title, progress and age, flag for disconnected rounds.
fn round_entry(ui: &egui::Ui, round: &RoundSummary, selected: bool) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let p = theme::of(ui);
    let mut job = LayoutJob::default();
    let title_color = if selected { p.text_strong } else { p.text };
    let name = round.title.as_deref().unwrap_or(&round.ticket);
    job.append(
        name,
        0.0,
        TextFormat::simple(theme::semibold(theme::TEXT_BUTTON), title_color),
    );
    let mut meta = format!(
        "\n{}/{} answered · {}",
        round.answered,
        round.total,
        age(round.opened_at)
    );
    if !round.connected {
        meta.push_str(" · ⚠ disconnected");
    }
    let meta_color = if round.connected { p.muted } else { p.warn };
    job.append(
        &meta,
        0.0,
        TextFormat::simple(FontId::proportional(theme::TEXT_SMALL), meta_color),
    );
    job
}

/// The "New" badge of a round the user has not looked at, laid out inside its sidebar entry.
fn new_badge(ui: &egui::Ui) -> egui::Atom<'static> {
    let p = theme::of(ui);
    let galley = ui.painter().layout_no_wrap(
        "New".to_owned(),
        theme::semibold(theme::TEXT_SMALL),
        p.on_accent,
    );
    let padding = egui::vec2(SPACE_S, 2.0);
    egui::Atom::paint(galley.size() + 2.0 * padding, move |ui, args| {
        let painter = ui.painter();
        painter.rect_filled(args.rect, theme::RADIUS, p.accent);
        painter.galley(args.rect.min + padding, galley.clone(), p.on_accent);
    })
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
