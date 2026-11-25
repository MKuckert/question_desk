//! The page area: the step bar plus the scrolling Question page, Review Page or empty
//! state, built from native controls. A page is rebuilt when its structure changes
//! (another round, step or layout) and otherwise updated in place, so the Note keeps its
//! focus, caret and IME state while the user types.

use std::cell::{Cell, RefCell};

use wxdragon::prelude::*;
use wxdragon::widgets::webview::WebView;
use wxdragon::event::WebViewEvents as _;

use super::{markdown, style, with_app};
use crate::desk::{QuestionState, QuestionView, RoundView, Step};

/// Below this column width the preview panel moves under the Options.
const WIDE_LAYOUT: i32 = 720;
/// The widest the page column gets.
const PAGE_WIDTH: i32 = 960;
/// wxRB_SINGLE: a radio button outside any group, so it can be cleared again (GTK
/// otherwise forces one button of a group on, which would pre-select an Option).
const RB_SINGLE: i64 = 0x0008;

/// What decides whether the page must be rebuilt.
#[derive(Debug, Clone, PartialEq)]
pub struct PageKey {
    ticket: Option<String>,
    step: Option<Step>,
    questions: usize,
    zoom: u32,
    wide: bool,
    status: String,
}

impl PageKey {
    pub fn new(round: Option<&RoundView>, status: &str, wide: bool) -> Self {
        Self {
            ticket: round.map(|r| r.ticket.clone()),
            step: round.map(|r| r.step),
            questions: round.map_or(0, |r| r.questions.len()),
            zoom: style::zoom().to_bits(),
            wide,
            status: if round.is_none() { status.to_owned() } else { String::new() },
        }
    }
}

/// A label that is re-wrapped when the column width changes. wx static text does not
/// wrap by itself.
struct Wrapped {
    label: StaticText,
    text: String,
    /// Share of the column width the label lives in (1.0, or 0.5 beside the preview).
    share: f32,
    indent: i32,
}

pub enum OptionControl {
    Radio(RadioButton),
    Check(CheckBox),
}

impl OptionControl {
    fn set_value(&self, value: bool) {
        match self {
            Self::Radio(r) => {
                if r.get_value() != value {
                    r.set_value(value)
                }
            }
            Self::Check(c) => {
                if c.get_value() != value {
                    c.set_value(value)
                }
            }
        }
    }

    fn focus(&self) {
        match self {
            Self::Radio(r) => r.set_focus(),
            Self::Check(c) => c.set_focus(),
        }
    }
}

pub struct QuestionWidgets {
    index: usize,
    options: Vec<OptionControl>,
    pub note: TextCtrl,
    note_hint: StaticText,
    body: Option<WebView>,
    preview: Option<WebView>,
    /// The preview text last sent to the preview view.
    preview_shown: RefCell<Option<Option<String>>>,
    hovered: Cell<Option<usize>>,
}

pub struct Page {
    pub key: PageKey,
    pub panel: Panel,
    scroller: ScrolledWindow,
    column: Panel,
    wrapped: Vec<Wrapped>,
    col_width: Cell<i32>,
    steps: Vec<ToggleButton>,
    pub question: Option<QuestionWidgets>,
    /// Content heights the web views reported, in DIP: (body, preview).
    web_heights: Cell<(i32, i32)>,
}

/// Escapes `&` so wx does not take it for a mnemonic marker.
pub fn esc(text: &str) -> String {
    text.replace('&', "&&")
}

/// The column width for a page area `available` pixels wide.
pub fn column_width(window: &impl WxWidget, available: i32) -> i32 {
    let margin = window.from_dip_int(style::SPACE_L) * 2;
    (available - margin).clamp(window.from_dip_int(280), window.from_dip_int(PAGE_WIDTH))
}

pub fn is_wide(window: &impl WxWidget, column: i32) -> bool {
    column >= window.from_dip_int(WIDE_LAYOUT)
}

impl Page {
    pub fn build(parent: &Panel, key: PageKey, round: Option<&RoundView>, status: &str) -> Page {
        let panel = Panel::builder(parent).build();
        panel.set_font(&style::base_font());
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let dip = |v: i32| parent.from_dip_int(v);

        let mut steps = Vec::new();
        if let Some(round) = round {
            let bar = WrapSizer::builder(Orientation::Horizontal)
                .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
                .build();
            for (i, question) in round.questions.iter().enumerate() {
                let button = ToggleButton::builder(&panel)
                    .with_label(&step_label(i, question.state))
                    .with_style(ToggleButtonStyle::ExactFit)
                    .build();
                button.set_min_size(Size::new(dip(40), -1));
                button.set_value(question.state == QuestionState::Current);
                button.set_tooltip(&question.question);
                button.on_toggle(move |_| with_app(|app| app.go_to(i)));
                bar.add(&button, 0, SizerFlag::Right | SizerFlag::Bottom, dip(style::SPACE_XS));
                steps.push(button);
            }
            let review = ToggleButton::builder(&panel)
                .with_label("Review")
                .with_style(ToggleButtonStyle::ExactFit)
                .build();
            review.set_min_size(Size::new(dip(64), -1));
            review.set_value(round.step == Step::Review);
            review.set_tooltip("Review all answers and submit");
            review.on_toggle(move |_| with_app(|app| app.review()));
            bar.add(&review, 0, SizerFlag::Right | SizerFlag::Bottom, dip(style::SPACE_XS));
            steps.push(review);
            sizer.add_sizer(
                &bar,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Expand,
                dip(style::SPACE_M),
            );
        }

        let scroller = ScrolledWindow::builder(&panel)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroller.set_scroll_rate(0, dip(12));
        let column = Panel::builder(&scroller).build();
        // Centered column of at most PAGE_WIDTH; it fills the height so the empty state
        // can center itself vertically.
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add_stretch_spacer(1);
        row.add(&column, 0, SizerFlag::Expand, 0);
        row.add_stretch_spacer(1);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add_sizer(&row, 1, SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom, dip(style::SPACE_L));
        scroller.set_sizer(outer, true);
        sizer.add(&scroller, 1, SizerFlag::Expand, 0);
        panel.set_sizer(sizer, true);

        let col = column_width(parent, parent.get_client_size().width);
        column.set_min_size(Size::new(col, -1));
        let mut page = Page {
            key,
            panel,
            scroller,
            column,
            wrapped: Vec::new(),
            col_width: Cell::new(col),
            steps,
            question: None,
            web_heights: Cell::new((0, 0)),
        };
        let column_sizer = BoxSizer::builder(Orientation::Vertical).build();
        match round {
            None => page.empty_state(&column_sizer, status),
            Some(round) => match round.step {
                Step::Question(i) => page.question_page(&column_sizer, round, i),
                Step::Review => page.review_page(&column_sizer, round),
            },
        }
        page.column.set_sizer(column_sizer, true);
        page.rewrap(col);
        page
    }

    fn label(&mut self, parent: &impl WxWidget, text: &str, share: f32, indent: i32) -> StaticText {
        let label = StaticText::builder(parent).with_label(&esc(text)).build();
        self.wrapped.push(Wrapped {
            label,
            text: text.to_owned(),
            share,
            indent,
        });
        label
    }

    fn empty_state(&mut self, sizer: &BoxSizer, status: &str) {
        let column = self.column;
        let dip = |v: i32| column.from_dip_int(v);
        sizer.add_stretch_spacer(2);
        let heading = StaticText::builder(&column)
            .with_label("No open questions")
            .with_style(StaticTextStyle::AlignCenterHorizontal)
            .build();
        style::title(&heading);
        sizer.add(&heading, 0, SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom, dip(style::SPACE_S));
        let status = self.label(&column, status, 1.0, 0);
        status.set_foreground_color(style::muted_colour());
        sizer.add(&status, 0, SizerFlag::AlignCenterHorizontal, 0);
        sizer.add_stretch_spacer(3);
    }

    fn question_page(&mut self, sizer: &BoxSizer, round: &RoundView, index: usize) {
        let column = self.column;
        let dip = |v: i32| column.from_dip_int(v);
        let question = &round.questions[index];

        let title = self.label(&column, &question.question, 1.0, 0);
        style::title(&title);
        sizer.add(&title, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));

        let body = question.body.as_ref().map(|body| {
            let view = web_view(&column, &markdown::body_page(body, style::base_point_size()), WebKind::Body);
            sizer.add(&view, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));
            view
        });

        if let Some(recommendation) = &question.recommendation_text {
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            let badge = StaticText::builder(&column).with_label("★ Recommended:").build();
            style::bold(&badge);
            badge.set_foreground_color(style::accent_colour());
            row.add(&badge, 0, SizerFlag::Right, dip(style::SPACE_S));
            let text = self.label(&column, recommendation, 1.0, dip(130));
            row.add(&text, 1, SizerFlag::Expand, 0);
            sizer.add_sizer(&row, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));
        }

        let mut options = Vec::new();
        let mut preview = None;
        if question.options.is_empty() {
            let open = StaticText::builder(&column)
                .with_label("Open question: answer in the note below.")
                .build();
            open.set_foreground_color(style::muted_colour());
            sizer.add(&open, 0, SizerFlag::Bottom, dip(style::SPACE_M));
        } else {
            let has_previews = question.options.iter().any(|o| o.preview.is_some());
            let wide = self.key.wide;
            let share = if has_previews && wide { 0.5 } else { 1.0 };
            let list = BoxSizer::builder(Orientation::Vertical).build();
            for (i, option) in question.options.iter().enumerate() {
                options.push(self.option_row(&list, question, i, option, share));
            }
            if has_previews {
                let selected = initial_preview(question);
                let view = web_view(
                    &column,
                    &markdown::preview_page(selected.as_deref(), style::base_point_size()),
                    WebKind::Preview,
                );
                view.set_min_size(Size::new(-1, dip(120)));
                let caption = StaticText::builder(&column).with_label("PREVIEW").build();
                caption.set_font(&style::scaled(0.8, true));
                caption.set_foreground_color(style::muted_colour());
                let boxed = BoxSizer::builder(Orientation::Vertical).build();
                boxed.add(&caption, 0, SizerFlag::Bottom, dip(style::SPACE_XS));
                boxed.add(&view, 1, SizerFlag::Expand, 0);
                if wide {
                    let row = BoxSizer::builder(Orientation::Horizontal).build();
                    row.add_sizer(&list, 1, SizerFlag::Right | SizerFlag::Expand, dip(style::SPACE_L));
                    row.add_sizer(&boxed, 1, SizerFlag::Expand | SizerFlag::Top, dip(style::SPACE_S));
                    sizer.add_sizer(&row, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));
                } else {
                    sizer.add_sizer(&list, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_S));
                    sizer.add_sizer(&boxed, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));
                }
                preview = Some((view, selected));
            } else {
                sizer.add_sizer(&list, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_M));
            }
        }

        let heading = StaticText::builder(&column).with_label("Note").build();
        style::bold(&heading);
        sizer.add(&heading, 0, SizerFlag::Top, dip(style::SPACE_S));
        let note_hint = StaticText::builder(&column).with_label(note_hint_text(question)).build();
        style::small(&note_hint);
        note_hint.set_foreground_color(style::muted_colour());
        sizer.add(&note_hint, 0, SizerFlag::Bottom | SizerFlag::Top, dip(2));
        let note = TextCtrl::builder(&column)
            .with_value(&question.note)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::WordWrap)
            .with_size(Size::new(-1, dip(84)))
            .build();
        note.set_accessibility_label("Note");
        note.on_text_changed(move |_| with_app(|app| app.note_changed()));
        sizer.add(&note, 0, SizerFlag::Expand, 0);

        let (preview, shown) = match preview {
            Some((view, shown)) => (Some(view), Some(shown)),
            None => (None, None),
        };
        self.question = Some(QuestionWidgets {
            index,
            options,
            note,
            note_hint,
            body,
            preview,
            preview_shown: RefCell::new(shown),
            hovered: Cell::new(None),
        });
    }

    fn option_row(
        &mut self,
        list: &BoxSizer,
        question: &QuestionView,
        i: usize,
        option: &crate::desk::OptionView,
        share: f32,
    ) -> OptionControl {
        let column = self.column;
        let dip = |v: i32| column.from_dip_int(v);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let control = if question.multi_select {
            let check = CheckBox::builder(&column).with_label(&esc(&option.label)).build();
            check.set_value(option.selected);
            check.on_toggled(move |_| with_app(|app| app.toggle_option(i)));
            style::bold(&check);
            row.add(&check, 0, SizerFlag::AlignCenterVertical, 0);
            check.on_mouse_enter(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(Some(i)))
            });
            check.on_mouse_leave(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(None))
            });
            OptionControl::Check(check)
        } else {
            let radio = RadioButton::builder(&column)
                .with_label(&esc(&option.label))
                .with_style(RadioButtonStyle::from_bits_retain(RB_SINGLE))
                .build();
            radio.set_value(option.selected);
            radio.on_selected(move |_| with_app(|app| app.select_option(i)));
            style::bold(&radio);
            row.add(&radio, 0, SizerFlag::AlignCenterVertical, 0);
            radio.on_mouse_enter(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(Some(i)))
            });
            radio.on_mouse_leave(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(None))
            });
            OptionControl::Radio(radio)
        };
        if option.recommended {
            let badge = StaticText::builder(&column).with_label("★ Recommended").build();
            badge.set_font(&style::scaled(0.85, true));
            badge.set_foreground_color(style::accent_colour());
            row.add(&badge, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, dip(style::SPACE_S));
        }
        row.add_stretch_spacer(1);
        if i < 9 {
            let key = StaticText::builder(&column).with_label(&(i + 1).to_string()).build();
            style::small(&key);
            key.set_foreground_color(style::muted_colour());
            key.set_tooltip(&format!("Press {} to select", i + 1));
            row.add(&key, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, dip(style::SPACE_S));
        }
        list.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::Top, dip(style::SPACE_S));
        if !option.description.is_empty() {
            // Indented to line up with the control's label, like a macOS radio group's
            // explanatory text.
            let indent = dip(22);
            let description = self.label(&column, &option.description, share, indent);
            description.set_foreground_color(style::muted_colour());
            description.on_mouse_left_down(move |_| with_app(|app| app.click_description(i)));
            description.on_mouse_enter(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(Some(i)))
            });
            description.on_mouse_leave(move |e| {
                e.skip(true);
                with_app(|app| app.hover_option(None))
            });
            let line = BoxSizer::builder(Orientation::Horizontal).build();
            line.add_spacer(indent);
            line.add(&description, 0, 0.into_flag(), 0);
            list.add_sizer(&line, 0, SizerFlag::Top, dip(2));
        }
        control
    }

    fn review_page(&mut self, sizer: &BoxSizer, round: &RoundView) {
        let column = self.column;
        let dip = |v: i32| column.from_dip_int(v);
        let heading = StaticText::builder(&column).with_label("Review your answers").build();
        style::title(&heading);
        sizer.add(&heading, 0, SizerFlag::Bottom, dip(style::SPACE_S));
        let unanswered = round.unanswered().len();
        if unanswered > 0 {
            let warn = StaticText::builder(&column)
                .with_label(&format!("⚠ {unanswered} question(s) unanswered"))
                .build();
            warn.set_foreground_color(style::warning_colour());
            sizer.add(&warn, 0, SizerFlag::Bottom, dip(style::SPACE_S));
        }
        for (i, question) in round.questions.iter().enumerate() {
            let answered = question.answer.is_some();
            let mark = if answered { "✓" } else { "⚠" };
            let caption = format!("{mark} {}. {}", i + 1, question.header);
            let boxed = StaticBoxSizerBuilder::new_with_label(Orientation::Vertical, &column, &esc(&caption)).build();
            let Some(frame) = boxed.get_static_box() else { continue };
            let inner = BoxSizer::builder(Orientation::Vertical).build();
            let text = self.label(&frame, &question.question, 1.0, dip(40));
            style::bold(&text);
            inner.add(&text, 0, SizerFlag::Bottom | SizerFlag::Expand, dip(style::SPACE_XS));
            match &question.answer {
                Some(answer) => {
                    let answer = self.label(&frame, answer, 1.0, dip(40));
                    inner.add(&answer, 0, SizerFlag::Expand, 0);
                }
                None => {
                    let missing = StaticText::builder(&frame).with_label("Unanswered").build();
                    style::bold(&missing);
                    missing.set_foreground_color(style::warning_colour());
                    inner.add(&missing, 0, 0.into_flag(), 0);
                }
            }
            if let Some(note) = &question.answer_note {
                let note = self.label(&frame, &format!("Note: {note}"), 1.0, dip(40));
                note.set_foreground_color(style::muted_colour());
                inner.add(&note, 0, SizerFlag::Top | SizerFlag::Expand, dip(style::SPACE_XS));
            }
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            row.add_sizer(&inner, 1, SizerFlag::Expand, 0);
            let edit = Button::builder(&frame).with_label("Edit").build();
            edit.set_tooltip("Edit this answer");
            edit.on_click(move |_| with_app(|app| app.go_to(i)));
            row.add(&edit, 0, SizerFlag::AlignCenterVertical | SizerFlag::Left, dip(style::SPACE_S));
            boxed.add_sizer(&row, 1, SizerFlag::All | SizerFlag::Expand, dip(style::SPACE_S));
            sizer.add_sizer(&boxed, 0, SizerFlag::Top | SizerFlag::Expand, dip(style::SPACE_S));
        }
    }

    /// Re-wraps the labels for a column `width` pixels wide and refits the scroll area.
    pub fn rewrap(&self, width: i32) {
        self.col_width.set(width);
        self.column.set_min_size(Size::new(width, -1));
        let gap = self.column.from_dip_int(style::SPACE_L);
        for w in &self.wrapped {
            let share = if w.share < 1.0 {
                ((width - gap) as f32 * w.share) as i32
            } else {
                width
            };
            w.label.set_label(&esc(&w.text));
            w.label.wrap((share - w.indent).max(80));
        }
        self.apply_web_heights();
        self.panel.layout();
        self.column.layout();
        self.scroller.layout();
        self.panel.layout();
    }

    pub fn col_width(&self) -> i32 {
        self.col_width.get()
    }

    /// A web view reported its content height (in CSS px, which are DIP).
    pub fn web_height(&self, kind: WebKind, height: i32) {
        let (body, preview) = self.web_heights.get();
        let heights = match kind {
            WebKind::Body => (height, preview),
            WebKind::Preview => (body, height),
        };
        if heights != self.web_heights.get() {
            self.web_heights.set(heights);
            self.apply_web_heights();
            self.column.layout();
            self.scroller.layout();
        }
    }

    fn apply_web_heights(&self) {
        let Some(q) = &self.question else { return };
        let (body, preview) = self.web_heights.get();
        let dip = |v: i32| self.column.from_dip_int(v);
        if let Some(view) = q.body {
            // Short bodies fit; long ones scroll inside a view of about a third of the
            // window, like the egui card.
            let window = self.panel.get_parent().map_or(800, |p| p.get_size().height);
            let max = (window as f32 * 0.35).max(dip(120) as f32) as i32;
            let height = if body > 0 { dip(body + 2).min(max) } else { dip(120) };
            view.set_min_size(Size::new(-1, height));
        }
        if let Some(view) = q.preview {
            let height = if preview > 0 { dip(preview + 2).clamp(dip(60), dip(420)) } else { dip(120) };
            view.set_min_size(Size::new(-1, height));
        }
    }

    /// Updates the page in place for a round whose structure did not change.
    pub fn update(&self, round: &RoundView) {
        for (i, (button, question)) in self.steps.iter().zip(&round.questions).enumerate() {
            let label = step_label(i, question.state);
            if button.get_label() != label {
                button.set_label(&label);
            }
            button.set_value(question.state == QuestionState::Current);
        }
        if let Some(review) = self.steps.last() {
            review.set_value(round.step == Step::Review);
        }
        let Some(q) = &self.question else { return };
        let Some(question) = round.questions.get(q.index) else { return };
        for (control, option) in q.options.iter().zip(&question.options) {
            control.set_value(option.selected);
        }
        let hint = note_hint_text(question);
        if q.note_hint.get_label() != hint {
            q.note_hint.set_label(hint);
        }
        if q.note.get_value() != question.note {
            q.note.change_value(&question.note);
        }
        self.update_preview(question);
    }

    pub fn hover(&self, option: Option<usize>) {
        if let Some(q) = &self.question {
            q.hovered.set(option);
        }
    }

    /// Shows the preview of the hovered Option, else of the first selected one.
    pub fn update_preview(&self, question: &QuestionView) {
        let Some(q) = &self.question else { return };
        let Some(view) = q.preview else { return };
        let hovered = q
            .hovered
            .get()
            .and_then(|i| question.options.get(i))
            .and_then(|o| o.preview.clone());
        let preview = hovered.or_else(|| initial_preview(question));
        let mut shown = q.preview_shown.borrow_mut();
        if shown.as_ref() != Some(&preview) {
            view.set_page(
                &markdown::preview_page(preview.as_deref(), style::base_point_size()),
                "about:blank",
            );
            *shown = Some(preview);
        }
    }

    pub fn note_focused(&self) -> bool {
        self.question.as_ref().is_some_and(|q| q.note.has_focus())
    }

    /// Initial focus: the first Option, so digits and Return reach the page rather than
    /// a text field.
    pub fn focus_default(&self) {
        match &self.question {
            Some(q) => match q.options.first() {
                Some(option) => option.focus(),
                None => self.column.set_focus(),
            },
            None => self.column.set_focus(),
        }
    }

    pub fn note_value(&self) -> Option<String> {
        self.question.as_ref().map(|q| q.note.get_value())
    }
}

trait IntoFlag {
    fn into_flag(self) -> SizerFlag;
}

impl IntoFlag for i32 {
    fn into_flag(self) -> SizerFlag {
        SizerFlag::from_bits_retain(self as i64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebKind {
    Body,
    Preview,
}

fn web_view(parent: &Panel, html: &str, kind: WebKind) -> WebView {
    let view = WebView::builder(parent).build();
    view.add_script_message_handler(markdown::HEIGHT_HANDLER);
    view.enable_context_menu(false);
    view.on_script_message_received(move |e| {
        let Some(message) = e.get_string() else { return };
        log::debug!("{kind:?} web view reports {message}");
        let height = message
            .split(',')
            .next()
            .and_then(|h| h.trim().parse::<i32>().ok());
        if let Some(height) = height {
            with_app(|app| app.web_height(kind, height));
        }
    });
    view.on_navigating(move |e| {
        let url = e.get_url().unwrap_or_default();
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            e.event.event.veto();
            launch_default_browser(&url, BrowserLaunchFlags::Default);
        }
    });
    view.set_page(html, "about:blank");
    view.set_min_size(Size::new(-1, parent.from_dip_int(120)));
    view
}

fn initial_preview(question: &QuestionView) -> Option<String> {
    question
        .options
        .iter()
        .filter(|o| o.selected)
        .find_map(|o| o.preview.clone())
}

fn note_hint_text(question: &QuestionView) -> &'static str {
    if question.options.is_empty() {
        "Your answer."
    } else if question.options.iter().any(|o| o.selected) {
        "Optional: caveats, reasons, “yes, but …”."
    } else {
        "Without a selected option, this text is your answer."
    }
}

fn step_label(i: usize, state: QuestionState) -> String {
    match state {
        QuestionState::Answered => format!("{} ✓", i + 1),
        _ => (i + 1).to_string(),
    }
}
