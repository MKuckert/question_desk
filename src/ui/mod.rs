//! The Question Desk window: renders the `desk` read model and dispatches user intents.

mod grapheme;
#[cfg(target_os = "macos")]
mod macos_text_input;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use eframe::egui::{
    self, FontId, Key, KeyboardShortcut, Modifiers, RichText, ScrollArea, Sense, Stroke, TextEdit,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use shadcn_egui::{
    Palette, Theme,
    components::{
        alert::Alert,
        badge::{Badge, BadgeVariant},
        button::{Button, ButtonSize, ButtonVariant},
        card::{Card, CardSize, card_content},
        dialog::{self, Dialog as ShadcnDialog},
        icon::IconView,
        item::{Item, ItemSize, ItemVariant},
        kbd::Kbd,
        label::Label,
        separator::Separator,
        tooltip::tooltip,
    },
    icons::lucide,
};

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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Theme::vega().apply(&cc.egui_ctx);
            // Dark unless the user switched to light last time.
            let light = cc
                .storage
                .and_then(|s| s.get_string(THEME_KEY))
                .is_some_and(|t| t == "light");
            cc.egui_ctx.set_theme(if light {
                egui::Theme::Light
            } else {
                egui::Theme::Dark
            });

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
                dark: !light,
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
    /// Last seen light/dark mode, saved like the zoom.
    dark: bool,
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
        storage.set_string(
            THEME_KEY,
            if self.dark { "dark" } else { "light" }.to_owned(),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl App {
    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.zoom = Some(ctx.zoom_factor());
        self.dark = ui.visuals().dark_mode;
        self.request_attention(&ctx);
        self.handle_close_request(&ctx);

        let sessions = self.desk.sessions();
        let round = self.desk.current();

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui, round.as_ref()));
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
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let toggle = if self.sidebar_open {
                lucide::PANEL_LEFT_CLOSE
            } else {
                lucide::PANEL_LEFT_OPEN
            };
            let button = Button::new("Toggle sidebar")
                .icon(toggle)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::IconSm);
            if tooltip(ui.add(button), "Show or hide open rounds").clicked() {
                self.sidebar_open = !self.sidebar_open;
            }
            match round {
                None => {
                    ui.add(IconView::new(&lucide::MESSAGE_CIRCLE_QUESTION));
                    ui.label(RichText::new("Question Desk").strong());
                }
                Some(round) => {
                    ui.add(
                        Badge::new(&round.session)
                            .icon(lucide::FOLDER)
                            .variant(BadgeVariant::Outline),
                    );
                    if let Some(title) = &round.title {
                        ui.label(RichText::new(title).strong());
                    }
                    if !round.connected {
                        let badge = Badge::new("Agent disconnected")
                            .icon(lucide::UNPLUG)
                            .variant(BadgeVariant::Destructive);
                        tooltip(
                            ui.add(badge),
                            "No Agent call is waiting for this round. Your answers are kept; \
                             the Agent can pick them up with its ticket.",
                        );
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme_toggle(ui);
                let Some(round) = round else { return };
                match round.step {
                    Step::Question(i) => {
                        ui.label(
                            RichText::new(format!("{} / {}", i + 1, round.questions.len()))
                                .color(palette(ui).muted_foreground),
                        );
                        ui.add(
                            Badge::new(&round.questions[i].header).variant(BadgeVariant::Secondary),
                        );
                    }
                    Step::Review => {
                        ui.add(
                            Badge::new("Review")
                                .icon(lucide::LIST_CHECKS)
                                .variant(BadgeVariant::Secondary),
                        );
                    }
                }
            });
        });
        ui.add_space(6.0);
    }

    fn sidebar(
        &mut self,
        ui: &mut egui::Ui,
        sessions: &[SessionView],
        current: Option<&RoundView>,
    ) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add(IconView::new(&lucide::INBOX));
            ui.label(RichText::new("Open rounds").strong());
            let count: usize = sessions.iter().map(|s| s.rounds.len()).sum();
            if count > 0 {
                ui.add(Badge::new(count.to_string()).variant(BadgeVariant::Secondary));
            }
        });
        ui.add(Separator::new());
        ScrollArea::vertical().show(ui, |ui| {
            if sessions.is_empty() {
                ui.label(RichText::new("None").color(palette(ui).muted_foreground));
            }
            for session in sessions {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let muted = palette(ui).muted_foreground;
                    ui.add(IconView::new(&lucide::FOLDER).size(14.0).color(muted));
                    ui.label(RichText::new(&session.label).small().color(muted));
                });
                for round in &session.rounds {
                    let selected = current.is_some_and(|c| c.ticket == round.ticket);
                    let name = round.title.as_deref().unwrap_or(&round.ticket);
                    let detail = format!(
                        "{}/{} answered · {}",
                        round.answered,
                        round.total,
                        age(round.opened_at)
                    );
                    let item = Item::new()
                        .id_salt(("round", &round.ticket))
                        .size(ItemSize::Sm)
                        .variant(if selected {
                            ItemVariant::Muted
                        } else {
                            ItemVariant::Default
                        })
                        .selected(selected)
                        .icon(lucide::MESSAGE_CIRCLE_QUESTION)
                        .title(name)
                        .description(detail)
                        .sense(Sense::click())
                        .show(ui, |ui| {
                            if !round.connected {
                                let icon = IconView::new(&lucide::UNPLUG)
                                    .size(14.0)
                                    .color(palette(ui).destructive);
                                tooltip(ui.add(icon), "Agent disconnected");
                            }
                            if round.new {
                                ui.add(Badge::new("New"));
                            }
                        });
                    if item.response.clicked() {
                        self.desk.select(&round.ticket);
                    }
                }
            }
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 4.0);
            ui.set_max_width(440.0);
            Card::new().show(ui, |ui| {
                card_content(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(
                            IconView::new(&lucide::INBOX)
                                .size(32.0)
                                .color(palette(ui).muted_foreground),
                        );
                        ui.add_space(4.0);
                        ui.heading("No open questions");
                        ui.label(RichText::new(&self.status).color(palette(ui).muted_foreground));
                    });
                });
            });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let cancel = Button::new("Cancel round")
                .icon(lucide::X)
                .variant(ButtonVariant::Ghost);
            if ui.add(cancel).clicked() {
                self.dialog = Some(Dialog::Cancel);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match round.step {
                    Step::Question(_) => {
                        if ui
                            .add(Button::new("Next").icon_end(lucide::ARROW_RIGHT))
                            .clicked()
                        {
                            self.desk.next();
                        }
                    }
                    Step::Review => {
                        if ui.add(Button::new("Submit").icon(lucide::SEND)).clicked() {
                            self.submit(round);
                        }
                    }
                }
                let at_start = round.step == Step::Question(0);
                let back = Button::new("Back")
                    .icon(lucide::ARROW_LEFT)
                    .variant(ButtonVariant::Outline);
                if ui.add_enabled(!at_start, back).clicked() {
                    self.desk.back();
                }
            });
        });
        ui.add_space(4.0);
        let cmd = cmd_symbol();
        let hints: &[(&str, &str)] = match round.step {
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
        ui.horizontal_wrapped(|ui| {
            let muted = palette(ui).muted_foreground;
            for (keys, action) in hints {
                ui.add(Kbd::new(keys.replace('⌘', cmd)));
                ui.label(RichText::new(*action).small().color(muted));
                ui.add_space(6.0);
            }
        });
        ui.add_space(6.0);
    }

    fn step_bar(&mut self, ui: &mut egui::Ui, round: &RoundView) {
        ui.horizontal_wrapped(|ui| {
            for (i, question) in round.questions.iter().enumerate() {
                let label = (i + 1).to_string();
                let button = match question.state {
                    QuestionState::Current => Button::new(label),
                    QuestionState::Answered => Button::new(label)
                        .icon(lucide::CHECK)
                        .variant(ButtonVariant::Secondary),
                    QuestionState::Unanswered => Button::new(label).variant(ButtonVariant::Outline),
                };
                if tooltip(ui.add(button.size(ButtonSize::Sm)), &question.question).clicked() {
                    self.desk.go_to(i);
                }
            }
            let review = Button::new("Review")
                .icon(lucide::LIST_CHECKS)
                .size(ButtonSize::Sm)
                .variant(if round.step == Step::Review {
                    ButtonVariant::Default
                } else {
                    ButtonVariant::Ghost
                });
            if ui.add(review).clicked() {
                self.desk.review();
            }
        });
    }

    // ---- Question page --------------------------------------------------------------------

    fn question_page(&mut self, ui: &mut egui::Ui, round: &RoundView, index: usize) {
        let question = &round.questions[index];
        ui.add_space(8.0);
        self.step_bar(ui, round);
        ui.add_space(12.0);
        ScrollArea::vertical()
            .id_salt(("page", &round.ticket, index))
            .show(ui, |ui| {
                Card::new().show(ui, |ui| {
                    card_content(ui, |ui| {
                        ui.label(RichText::new(&question.question).heading().strong());
                        if let Some(body) = &question.body {
                            ui.add(Separator::new());
                            ScrollArea::vertical()
                                .id_salt(("body", &round.ticket, index))
                                .max_height((ui.ctx().content_rect().height() * 0.35).max(120.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    CommonMarkViewer::new().show(ui, &mut self.markdown, body);
                                });
                        }
                    });
                });
                ui.add_space(12.0);
                if let Some(recommendation) = &question.recommendation_text {
                    ui.add(
                        Alert::new()
                            .icon(lucide::SPARKLES)
                            .title("Recommended")
                            .description(recommendation.as_str()),
                    );
                    ui.add_space(12.0);
                }
                self.options_and_preview(ui, round, question, index);
                ui.add_space(12.0);
                self.note_field(ui, round, question, index);
                ui.add_space(8.0);
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
            ui.label(RichText::new("Open question: answer in the note below.").weak());
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
            let response = option_card(ui, option, i, question.multi_select);
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
        ui.add(Label::new("Note"));
        ui.add_space(2.0);
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
        ui.add_space(8.0);
        self.step_bar(ui, round);
        ui.add_space(12.0);
        ui.heading("Review your answers");
        ui.add_space(6.0);
        let unanswered = round.unanswered().len();
        if unanswered > 0 {
            ui.add(
                Alert::new()
                    .icon(lucide::TRIANGLE_ALERT)
                    .title(format!("{unanswered} question(s) unanswered"))
                    .description("Click a question to answer it, or submit anyway."),
            );
        }
        ui.add_space(8.0);
        ScrollArea::vertical()
            .id_salt(("review", &round.ticket))
            .show(ui, |ui| {
                for (i, question) in round.questions.iter().enumerate() {
                    let card = ui.scope(|ui| {
                        Card::new().size(CardSize::Sm).show(ui, |ui| {
                            card_content(ui, |ui| review_card(ui, i, question));
                        });
                    });
                    let response = ui
                        .interact(
                            card.response.rect,
                            egui::Id::unique(("review", &round.ticket, i)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if question.answer.is_none() || response.hovered() {
                        let p = palette(ui);
                        let color = if question.answer.is_none() {
                            p.destructive
                        } else {
                            p.ring
                        };
                        ui.painter().rect_stroke(
                            card.response.rect,
                            ui.visuals().widgets.noninteractive.corner_radius,
                            Stroke::new(1.5, color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if tooltip(response, "Edit this answer").clicked() {
                        self.desk.go_to(i);
                    }
                    ui.add_space(8.0);
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
        let mut open = true;
        let destructive = dialog != Dialog::SubmitUnanswered;
        ShadcnDialog::new("dialog", title)
            .description(&text)
            .show(ctx, &mut open, |ui| {
                dialog::footer(ui, |ui| {
                    let keep = Button::new("Keep answering").variant(ButtonVariant::Outline);
                    if ui.add(keep).clicked() {
                        decision = Some(false);
                    }
                    let confirm = Button::new(confirm).variant(if destructive {
                        ButtonVariant::Destructive
                    } else {
                        ButtonVariant::Default
                    });
                    if ui.add(confirm).clicked() {
                        decision = Some(true);
                    }
                });
                // Enter confirms only the harmless dialog; cancelling needs a click.
                if dialog == Dialog::SubmitUnanswered && ui.input(|i| i.key_pressed(Key::Enter)) {
                    decision = Some(true);
                }
            });
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

fn review_card(ui: &mut egui::Ui, index: usize, question: &QuestionView) {
    let p = palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{}.", index + 1)).color(p.muted_foreground));
        ui.add(Badge::new(&question.header).variant(BadgeVariant::Secondary));
        ui.label(RichText::new(&question.question).strong());
    });
    match &question.answer {
        Some(answer) => {
            ui.horizontal_wrapped(|ui| {
                ui.add(IconView::new(&lucide::CIRCLE_CHECK).color(p.primary));
                ui.label(answer);
            });
        }
        None => {
            ui.add(
                Badge::new("Unanswered")
                    .icon(lucide::CIRCLE_X)
                    .variant(BadgeVariant::Destructive),
            );
        }
    }
    if let Some(note) = &question.answer_note {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                IconView::new(&lucide::STICKY_NOTE)
                    .size(14.0)
                    .color(p.muted_foreground),
            );
            ui.label(RichText::new(note).color(p.muted_foreground));
        });
    }
}

/// shadcn has no selectable card, so the Option card is drawn from the theme's tokens:
/// card fill and border, accent on hover, a primary ring when selected.
fn option_card(
    ui: &mut egui::Ui,
    option: &OptionView,
    index: usize,
    multi_select: bool,
) -> egui::Response {
    let p = palette(ui);
    let radius = ui.visuals().widgets.inactive.corner_radius;
    let mut prepared = egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(radius)
        .inner_margin(14.0)
        .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let mark = match (multi_select, option.selected) {
                (true, true) => lucide::SQUARE_CHECK,
                (true, false) => lucide::SQUARE,
                (false, true) => lucide::CIRCLE_DOT,
                (false, false) => lucide::CIRCLE,
            };
            let color = if option.selected {
                p.primary
            } else {
                p.muted_foreground
            };
            ui.add(IconView::new(&mark).size(20.0).color(color));
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    if index < 9 {
                        ui.add(Kbd::new((index + 1).to_string()));
                    }
                    ui.label(RichText::new(&option.label).strong());
                    if option.recommended {
                        ui.add(Badge::new("Recommended").icon(lucide::SPARKLES));
                    }
                });
                if !option.description.is_empty() {
                    ui.label(RichText::new(&option.description).color(p.muted_foreground));
                }
            });
        });
    }
    let response = prepared
        .allocate_space(ui)
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if option.selected {
        prepared.frame.fill = p.primary.gamma_multiply(0.08);
        prepared.frame.stroke = Stroke::new(2.0, p.primary);
    } else if response.hovered() {
        prepared.frame.fill = p.accent;
        prepared.frame.stroke = Stroke::new(1.0, p.ring);
    }
    prepared.paint(ui);
    response
}

fn preview_panel(ui: &mut egui::Ui, preview: Option<&str>) {
    Card::new().size(CardSize::Sm).show(ui, |ui| {
        card_content(ui, |ui| {
            let muted = palette(ui).muted_foreground;
            ui.horizontal(|ui| {
                ui.add(IconView::new(&lucide::EYE).size(14.0).color(muted));
                ui.label(RichText::new("Preview").small().color(muted));
            });
            ScrollArea::both()
                .id_salt("preview")
                .max_height(420.0)
                .show(ui, |ui| match preview {
                    Some(preview) => {
                        ui.label(RichText::new(preview).font(FontId::monospace(13.0)));
                    }
                    None => {
                        ui.label(RichText::new("Hover an option to see its preview.").color(muted));
                    }
                });
        });
    });
}

/// Sun/moon button that flips between the light and the dark palette of the theme.
fn theme_toggle(ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    let (icon, next, hint) = if dark {
        (lucide::SUN, egui::Theme::Light, "Switch to light mode")
    } else {
        (lucide::MOON, egui::Theme::Dark, "Switch to dark mode")
    };
    let button = Button::new(hint)
        .icon(icon)
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::IconSm);
    if tooltip(ui.add(button), hint).clicked() {
        ui.ctx().set_theme(next);
    }
}

fn palette(ui: &egui::Ui) -> Palette {
    *Theme::current(ui.ctx()).palette()
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
