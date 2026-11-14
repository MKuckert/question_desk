//! Renders the real window offscreen into `docs/`. Not a regression test:
//! `cargo test --release --lib screenshots -- --ignored`.

use eframe::egui;
use egui_kittest::Harness;
use serde_json::{Value, json};

use super::{App, Dialog};
use crate::desk::Desk;
use crate::schema::AskInput;

fn round(ticket: &str, source: &str, body: Value) -> AskInput {
    let mut body = body;
    body["ticket"] = json!(ticket);
    body["source"] = json!(source);
    serde_json::from_value(body).expect("valid round")
}

fn event_store() -> Value {
    json!({
        "title": "Event store design",
        "questions": [
            {
                "question": "Which database should back the event store?",
                "header": "Database",
                "body": "We need **append-only** writes, ordered reads per stream, and ~2k events/s at peak.\n\n| | Postgres | EventStoreDB | DynamoDB |\n|---|---|---|---|\n| Ops effort | low (we run it) | new system | managed |\n| Ordering | per table, via sequence | native | per partition |\n\nWe already run Postgres 16 in production.",
                "options": [
                    { "label": "Postgres", "description": "Events table with a global sequence; we know how to operate it." },
                    { "label": "EventStoreDB", "description": "Purpose-built, projections included, but a new system to run." },
                    { "label": "DynamoDB", "description": "Managed and scalable; ordering only within a partition." }
                ],
                "recommended": "Postgres"
            },
            {
                "question": "Do we need snapshots from day one?",
                "header": "Snapshots",
                "options": [
                    { "label": "No", "description": "Add them when an aggregate gets slow." },
                    { "label": "Yes", "description": "Snapshot every 100 events from the start." }
                ],
                "recommended": "No"
            },
            { "question": "Anything else we should know?", "header": "Notes", "options": [] }
        ]
    })
}

fn offsite() -> Value {
    json!({
        "title": "Team offsite 🏔️",
        "questions": [{
            "question": "Where should we go?",
            "header": "Place",
            "options": [
                { "label": "Mountains ⛰️", "description": "Hiking and a cabin." },
                { "label": "Seaside 🌊", "description": "Beach and boats." }
            ]
        }]
    })
}

fn desk() -> Desk {
    let desk = Desk::default();
    for (input, client) in [
        (
            round("events-7f3", "~/code/shop-backend", event_store()),
            "claude-code",
        ),
        (
            round("offsite-2c1", "~/code/team-offsite", offsite()),
            "claude-code",
        ),
    ] {
        // The Attachment keeps the round connected; leak it for the lifetime of the test.
        std::mem::forget(
            desk.ask(input, Some(client.to_owned()))
                .expect("round opens"),
        );
    }
    desk.select("events-7f3");
    desk.toggle_option(0);
    desk
}

fn shoot(name: &str, theme: &str, prepare: impl Fn(&mut App)) {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .with_pixels_per_point(2.0)
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                let app = app.get_or_insert_with(|| {
                    // eframe adds the system fonts (emoji, symbols); kittest does not.
                    ui.ctx().add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                    let mut app = App::new(ui.ctx(), desk(), String::new(), Some(theme));
                    prepare(&mut app);
                    app
                });
                app.show(ui);
            },
            None,
        );
    harness.run_steps(8);
    let image = harness.render().expect("render");
    let path = format!("{}/docs/{name}.png", env!("CARGO_MANIFEST_DIR"));
    image.save(&path).expect("save");
    println!("wrote {path}");
}

#[test]
#[ignore = "writes screenshots into docs/"]
fn screenshots() {
    shoot("prototype-shadcn-dark", "Tokyo Night", |_| {});
    shoot("prototype-shadcn-light", "Tokyo Night Light", |_| {});
    shoot("prototype-tokyo-review-dark", "Tokyo Night", |app| {
        app.desk.review()
    });
    shoot("prototype-tokyo-cancel-dark", "Tokyo Night", |app| {
        app.dialog = Some(Dialog::Cancel)
    });
    shoot(
        "prototype-tokyo-appearance-light",
        "Tokyo Night Light",
        |app| app.appearance.window_open = true,
    );
}

#[test]
fn switching_themes_and_editing_live_keeps_running() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                let app =
                    app.get_or_insert_with(|| App::new(ui.ctx(), desk(), String::new(), None));
                app.show(ui);
            },
            None,
        );
    harness.run_steps(2);
    for _ in 0..3 {
        let ctx = harness.ctx.clone();
        let app = harness.state_mut().as_mut().unwrap();
        app.appearance.toggle_dark(&ctx);
        harness.run_steps(2);
    }
    let app = harness.state_mut().as_mut().unwrap();
    assert!(
        !app.appearance.theme.dark,
        "three toggles from dark end in light"
    );
    app.appearance.editor.open = true;
    harness.run_steps(3);
}
