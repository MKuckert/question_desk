//! The detail pane: the Question page, the Review Page and the empty state, plus the toolbar,
//! the keys and the confirmations that belong to them.

use day::prelude::*;
use day::reactive::{Signal, bind, untrack};

use super::{act, cmd_symbol, desk, markdown, native, state};
use crate::desk::{Desk, OptionView, QuestionState, QuestionView, RoundView, Step};

/// The widest the page content gets, so long lines stay readable.
const PAGE_WIDTH: f64 = 900.0;

/// The detail pane: rebuilt when the round or the step changes, updated in place otherwise.
pub(crate) fn detail() -> impl Piece {
    let s = state();
    switch(
        move || {
            s.round
                .with(|r| r.as_ref().map(|r| (r.ticket.clone(), r.step)))
        },
        |key| match key {
            None => empty_state().any(),
            Some((_, Step::Question(i))) => question_page(*i).any(),
            Some((_, Step::Review)) => review_page().any(),
        },
    )
    .grow(Dimension::Both)
}

/// The window toolbar: a disconnected warning, where the round stands, and Settings. Derived
/// as a whole, so Day re-runs it when the round changes. Day keeps a label item by its id and
/// does not retitle it, so the progress label's id carries its text: a new text is a new item.
pub(crate) fn toolbar_items() -> Vec<ToolbarEntry> {
    let s = state();
    let mut items = Vec::new();
    if let Some(round) = s.round.get() {
        if !round.connected {
            items.push(toolbar_label(
                day::id!(disconnected),
                "⚠ Agent disconnected",
            ));
        }
        let progress = match round.step {
            Step::Question(i) => format!(
                "Question {} of {} · {}",
                i + 1,
                round.questions.len(),
                round.questions[i].header
            ),
            Step::Review => format!("Review · {} questions", round.questions.len()),
        };
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            progress.hash(&mut hasher);
            hasher.finish()
        };
        items.push(toolbar_label(day::id!(progress, key), progress));
    }
    items.push(
        toolbar_button(day::id!(settings), "Settings")
            .icon(Symbol::Settings)
            .action(|| {
                day::open_preferences();
            }),
    );
    items
}

// ---- Empty state ----------------------------------------------------------------------------

fn empty_state() -> impl Piece {
    let s = state();
    let message = column((
        label("No open questions").font(Font::Title),
        label(move || s.status.get())
            .secondary()
            .selectable()
            .align(day_spec::props::TextAlign::Center),
    ))
    .spacing(Space::S)
    .align(HAlign::Center);
    column((spacer(), row((spacer(), message, spacer())), spacer()))
        .padding(Space::L)
        .grow(Dimension::Both)
}

// ---- Question page --------------------------------------------------------------------------

fn question_page(index: usize) -> impl Piece {
    let s = state();
    let Some(round) = untrack(|| s.round.get()) else {
        return empty_state().any();
    };
    let question = round.questions[index].clone();

    // The Note is local while the page lives; every edit goes to the desk.
    let note = Signal::new(question.note.clone());
    bind(
        move || note.get(),
        move |text: &String| {
            if desk().current().is_some_and(|r| match r.step {
                Step::Question(i) => r.questions[i].note != *text,
                Step::Review => false,
            }) {
                act(|d| d.set_note(text));
            }
        },
    );
    // The page's keys go to the key catcher until the user clicks into the Note.
    day::task(async move {
        day::sleep(0).await;
        s.keys_focused.set(true);
    });

    let has_options = !question.options.is_empty();
    let hint = move || {
        let selected = s.round.with(|r| {
            r.as_ref()
                .and_then(|r| r.questions.get(index))
                .is_some_and(|q| q.options.iter().any(|o| o.selected))
        });
        if !has_options {
            "Your answer"
        } else if selected {
            "Note (optional): caveats, reasons, “yes, but …”"
        } else {
            "Note: without a selected option, this text is your answer"
        }
    };
    let note_ref = NativeRef::new();
    native::follow_placeholder(&note_ref, hint);

    let content = column((
        label(question.question.clone())
            .font(Font::Title)
            .selectable(),
        question.body.clone().map(|body| markdown::body(&body).card()),
        question
            .recommendation_text
            .clone()
            .map(|text| row((recommended_badge(), label(text).selectable())).spacing(Space::S)),
        options_and_preview(&question),
        column((
            label("Note").font(Font::Headline),
            native::bezeled_text_area(
                text_area(note)
                    .placeholder(untrack(hint))
                    .min_lines(3)
                    .max_lines(12)
                    .focused(s.note_focused)
                    .native_ref(&note_ref),
            ),
        ))
        .spacing(Space::XS)
        .align(HAlign::Leading),
    ))
    .spacing(Space::L)
    .align(HAlign::Leading)
    .max_width(PAGE_WIDTH)
    .padding(Space::L);

    // A canvas behind the page holds the keyboard while the Note does not: digits, Enter and
    // Tab reach it; ⌘ chords still go to the menu.
    let keys = native::quiet_focus(
        canvas(|_, _| {})
            .keyboard()
            .on_key(move |k| question_key(index, k))
            .focused(s.keys_focused)
            .grow(Dimension::Both),
    );

    column((
        step_bar(&round),
        divider(),
        zstack((
            keys,
            scroll(row((spacer(), content, spacer())).align(VAlign::Top)).grow(Dimension::Both),
        ))
        .align(Alignment::TopLeading)
        .grow(Dimension::Both),
        divider(),
        footer(Step::Question(index)),
    ))
    .grow(Dimension::Both)
    .any()
}

fn question_key(index: usize, k: &day::KeyEvent) {
    if !k.is_press() || k.primary() || k.alt() || k.control() {
        return;
    }
    let s = state();
    match k.key {
        day::Key::Tab => s.note_focused.set(true),
        day::Key::Escape => escape(),
        day::Key::Enter => {
            let accepts = s.round.with(|r| {
                r.as_ref()
                    .and_then(|r| r.questions.get(index))
                    .is_some_and(|q| {
                        q.answer.is_none()
                            && (q.recommendation_text.is_some()
                                || q.options.iter().any(|o| o.recommended))
                    })
            });
            if accepts {
                accept_recommendation();
            }
        }
        _ => {
            if let Some(d @ 1..=9) = k.digit() {
                act(|desk| desk.toggle_option(usize::from(d) - 1));
            }
        }
    }
}

fn options_and_preview(question: &QuestionView) -> AnyPiece {
    if question.options.is_empty() {
        return label("Open question: answer in the note below.")
            .secondary()
            .any();
    }
    let options = column(
        question
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| option_row(i, o, question.multi_select).any())
            .collect::<Vec<_>>(),
    )
    .spacing(Space::M)
    .grow(Dimension::Width);
    if question.options.iter().any(|o| o.preview.is_some()) {
        adaptive((options, preview_panel().grow(Dimension::Width)))
            .spacing(Space::L)
            .align(VAlign::Top)
            .any()
    } else {
        options.any()
    }
}

fn option_row(index: usize, option: &OptionView, multi_select: bool) -> impl Piece {
    let s = state();
    let selected = move || {
        s.round.with(|r| {
            r.as_ref()
                .and_then(|r| match r.step {
                    Step::Question(q) => r.questions.get(q),
                    Step::Review => None,
                })
                .and_then(|q| q.options.get(index))
                .is_some_and(|o| o.selected)
        })
    };
    let toggle = move || act(|d| d.toggle_option(index));
    let key = (index < 9).then(|| {
        label((index + 1).to_string())
            .font(Font::Caption)
            .monospace()
            .color(SystemColor::TertiaryText)
    });
    column((
        row((
            native::choice(option.label.clone(), multi_select, selected, toggle),
            option.recommended.then(recommended_badge),
            spacer(),
            key,
        ))
        .spacing(Space::S),
        (!option.description.is_empty()).then(|| {
            label(option.description.clone())
                .secondary()
                .padding(Insets::new(0.0, 0.0, 0.0, native::CHOICE_INDENT))
                .on_tap(toggle)
        }),
    ))
    .spacing(Space::XS)
    .align(HAlign::Leading)
    .on_hover(move |p| {
        if p.is_some() {
            s.hovered.set(Some(index));
        } else if s.hovered.get_untracked() == Some(index) {
            s.hovered.set(None);
        }
    })
}

fn recommended_badge() -> impl Piece {
    label("Recommended")
        .font(Font::Caption)
        .bold()
        .color(SystemColor::Accent)
        .padding(Insets::new(1.0, 6.0, 1.0, 6.0))
        .background(SystemColor::Accent.with_alpha(0.15))
        .corner_radius(Corner::CAPSULE)
}

/// The preview of the hovered Option, else of the first selected one that has one.
fn preview_text() -> Option<String> {
    let s = state();
    let hovered = s.hovered.get();
    s.round.with(|r| {
        let r = r.as_ref()?;
        let Step::Question(q) = r.step else {
            return None;
        };
        let q = r.questions.get(q)?;
        hovered
            .and_then(|h| q.options.get(h))
            .and_then(|o| o.preview.clone())
            .or_else(|| {
                q.options
                    .iter()
                    .filter(|o| o.selected)
                    .find_map(|o| o.preview.clone())
            })
    })
}

fn preview_panel() -> impl Piece {
    column((
        label("PREVIEW").font(Font::Caption).bold().secondary(),
        label(move || {
            preview_text().unwrap_or_else(|| "Hover an option to see its preview.".to_owned())
        })
        .monospace()
        .selectable(),
    ))
    .spacing(Space::S)
    .align(HAlign::Leading)
    .card()
}

fn step_bar(round: &RoundView) -> impl Piece {
    let s = state();
    let count = round.questions.len();
    let current = match round.step {
        Step::Question(i) => i,
        Step::Review => count,
    };
    let chosen = Signal::new(current);
    bind(
        move || chosen.get(),
        move |&i: &usize| {
            if i != current {
                if i < count {
                    act(|d| d.go_to(i));
                } else {
                    act(Desk::review);
                }
            }
        },
    );
    let segments = move || {
        s.round.with(|r| {
            let mut segments: Vec<(usize, String)> = r
                .iter()
                .flat_map(|r| r.questions.iter().enumerate())
                .map(|(i, q)| {
                    let mark = match q.state {
                        QuestionState::Answered => " ✓",
                        QuestionState::Current | QuestionState::Unanswered => "",
                    };
                    (i, format!("{}{mark}", i + 1))
                })
                .collect();
            segments.push((count, "Review".to_owned()));
            segments
        })
    };
    row((spacer(), picker(chosen, segments).segmented(), spacer()))
        .padding(Insets::new(10.0, 16.0, 10.0, 16.0))
}

fn footer(step: Step) -> impl Piece {
    let primary = match step {
        Step::Question(_) => "Next",
        Step::Review => "Submit",
    };
    let hints = match step {
        Step::Question(_) => format!(
            "1–9 select · Tab note · ↩ accept recommendation · {cmd}↩ next · {cmd}⇧↩ back · \
             Esc leave note / cancel",
            cmd = cmd_symbol()
        ),
        Step::Review => format!(
            "↩ or {cmd}↩ submit · {cmd}⇧↩ back · Esc cancel",
            cmd = cmd_symbol()
        ),
    };
    column((
        row((
            button("Cancel Round…").action(confirm_cancel),
            spacer(),
            button("Back")
                .enabled(step != Step::Question(0))
                .action(|| act(Desk::back)),
            button(primary).prominent().action(next_or_submit),
        ))
        .spacing(Space::S),
        label(hints).font(Font::Caption).secondary(),
    ))
    .spacing(Space::S)
    .align(HAlign::Leading)
    .padding(Insets::new(10.0, 16.0, 12.0, 16.0))
}

// ---- Review page ----------------------------------------------------------------------------

fn review_page() -> impl Piece {
    let s = state();
    let Some(round) = untrack(|| s.round.get()) else {
        return empty_state().any();
    };
    let unanswered = round.unanswered().len();
    let cards = round
        .questions
        .iter()
        .enumerate()
        .map(|(i, q)| review_card(i, q).any())
        .collect::<Vec<_>>();
    let content = column((
        label("Review your answers").font(Font::Title),
        (unanswered > 0).then(|| {
            label(format!("⚠ {unanswered} question(s) unanswered"))
                .color(SystemColor::Warning)
        }),
        column(cards).spacing(Space::M),
    ))
    .spacing(Space::M)
    .align(HAlign::Leading)
    .max_width(PAGE_WIDTH)
    .padding(Space::L);
    column((
        step_bar(&round),
        divider(),
        scroll(row((spacer(), content, spacer())).align(VAlign::Top)).grow(Dimension::Both),
        divider(),
        footer(Step::Review),
    ))
    .grow(Dimension::Both)
    .any()
}

fn review_card(index: usize, question: &QuestionView) -> impl Piece {
    let answered = question.answer.is_some();
    let (mark, color) = if answered {
        ("✓", SystemColor::Success)
    } else {
        ("⚠", SystemColor::Warning)
    };
    column((
        row((
            label(mark).color(color),
            label(format!("{}.", index + 1)).secondary(),
            label(question.header.clone()).bold(),
            label(question.question.clone()).grow(Dimension::Width),
            button("Edit").action(move || act(|d| d.go_to(index))),
        ))
        .spacing(Space::S),
        match &question.answer {
            Some(answer) => label(answer.clone()).selectable().any(),
            None => label("Unanswered")
                .italic()
                .color(SystemColor::Warning)
                .any(),
        },
        question
            .answer_note
            .clone()
            .map(|note| label(format!("Note: {note}")).secondary().selectable()),
    ))
    .spacing(Space::XS)
    .align(HAlign::Leading)
    .card()
}

// ---- Intents with confirmation ---------------------------------------------------------------

pub(crate) fn next_or_submit() {
    let Some(round) = desk().current() else {
        return;
    };
    match round.step {
        Step::Question(_) => act(Desk::next),
        Step::Review => submit(&round),
    }
}

pub(crate) fn accept_recommendation() {
    act(Desk::accept_recommendation);
}

fn submit(round: &RoundView) {
    let unanswered = round.unanswered().len();
    if unanswered == 0 {
        act(Desk::submit);
        return;
    }
    let ticket = round.ticket.clone();
    day::task(async move {
        let choice = Alert::new("Submit with unanswered questions?")
            .message(format!(
                "{unanswered} question(s) have no answer. The Agent is told which ones."
            ))
            .button("Submit Anyway", true)
            .cancel("Keep Answering")
            .present()
            .await;
        if choice == Some(true) && desk().current().is_some_and(|r| r.ticket == ticket) {
            act(Desk::submit);
        }
    });
}

/// Escape: leaves the Note if it has the keyboard, closes the Settings window if that is in
/// front, else asks to cancel the round.
pub(crate) fn escape() {
    let s = state();
    if s.note_focused.get_untracked() {
        s.keys_focused.set(true);
        return;
    }
    if let Some(focused) = day::focused_window()
        && day::initial_window().is_none_or(|main| {
            day_core::windows::window_node_id(&focused) != day_core::windows::window_node_id(&main)
        })
    {
        focused.close();
        return;
    }
    confirm_cancel();
}

fn confirm_cancel() {
    let Some(ticket) = desk().current().map(|r| r.ticket) else {
        return;
    };
    native::mark_destructive("Cancel Round");
    day::task(async move {
        let choice = Alert::new("Cancel this round?")
            .message("The Agent receives the answers you gave so far, marked as cancelled.")
            // The first button answers Return; cancelling needs a click.
            .button("Keep Answering", false)
            .destructive("Cancel Round", true)
            .present()
            .await;
        if choice == Some(true) && desk().current().is_some_and(|r| r.ticket == ticket) {
            act(Desk::cancel);
        }
    });
}
