//! Interactive demo client: sends realistic Question Rounds to a running Question Desk and
//! prints each result exactly as the Agent receives it.
//!
//! ```sh
//! cargo run --bin demo-client                 # pick rounds from a menu
//! cargo run --bin demo-client -- grilling     # send one round
//! cargo run --bin demo-client -- all          # send every round at once, from different sources
//! cargo run --bin demo-client -- --pickup db-choice-1a2b   # poll wait_for_answers
//! ```

use std::{
    io::{BufRead, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use rmcp::{
    RoleClient,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProtocolVersion,
    },
    service::{ClientLifecycleMode, ClientServiceExt, RunningService},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

const USAGE: &str = "\
usage: demo-client [--url URL] [--token TOKEN] [ROUND... | all]
       demo-client [--url URL] [--token TOKEN] --pickup TICKET

Without ROUND, shows a menu. `all` sends every round concurrently, each under its own
`source`. The token defaults to $QUESTION_DESK_TOKEN.";

struct Args {
    url: String,
    token: Option<String>,
    rounds: Vec<String>,
    pickup: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args {
        url: "http://127.0.0.1:7457/mcp".into(),
        token: std::env::var("QUESTION_DESK_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
        rounds: Vec::new(),
        pickup: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .unwrap_or_else(|| exit_usage(&format!("{arg} needs a value")))
        };
        match arg.as_str() {
            "--url" => args.url = value(),
            "--token" => args.token = Some(value()),
            "--pickup" => args.pickup = Some(value()),
            "-h" | "--help" => exit_usage(""),
            flag if flag.starts_with('-') => exit_usage(&format!("unknown flag {flag}")),
            round => args.rounds.push(round.to_owned()),
        }
    }
    args
}

fn exit_usage(error: &str) -> ! {
    if !error.is_empty() {
        eprintln!("error: {error}\n");
    }
    eprintln!(
        "{USAGE}\n\nrounds: {}",
        ROUNDS
            .iter()
            .map(|(n, _, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    );
    std::process::exit(if error.is_empty() { 0 } else { 2 })
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    if let Some(ticket) = &args.pickup {
        pickup(&args, ticket).await;
        return;
    }
    let names = if args.rounds.is_empty() {
        menu()
    } else {
        args.rounds.clone()
    };
    let names: Vec<&str> = if names.iter().any(|n| n == "all") {
        ROUNDS.iter().map(|(n, _, _)| *n).collect()
    } else {
        names.iter().map(String::as_str).collect()
    };
    let mut calls = Vec::new();
    for name in names {
        let Some((name, source, round)) = ROUNDS.iter().find(|(n, _, _)| *n == name) else {
            exit_usage(&format!("unknown round {name:?}"));
        };
        let mut args_json = round();
        let ticket = format!("{name}-{}", suffix());
        args_json["ticket"] = json!(ticket);
        args_json["source"] = json!(source);
        let (url, token) = (args.url.clone(), args.token.clone());
        println!("→ sending {name:?} as ticket {ticket} from {source:?}");
        calls.push(tokio::spawn(async move {
            let agent = connect(&url, token).await;
            let result = call(&agent, "ask_user_question", args_json).await;
            print_result(&ticket, &result);
        }));
    }
    for call in calls {
        let _ = call.await;
    }
}

fn menu() -> Vec<String> {
    println!("Rounds:");
    for (i, (name, source, round)) in ROUNDS.iter().enumerate() {
        let questions = round()["questions"].as_array().map_or(0, Vec::len);
        println!(
            "  {}. {name:<10} {questions:>2} question(s), source {source:?}",
            i + 1
        );
    }
    print!("Send which? (numbers or names, space-separated; `all`): ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.split_whitespace()
        .map(|w| match w.parse::<usize>() {
            Ok(i) if (1..=ROUNDS.len()).contains(&i) => ROUNDS[i - 1].0.to_owned(),
            _ => w.to_owned(),
        })
        .collect()
}

async fn pickup(args: &Args, ticket: &str) {
    let agent = connect(&args.url, args.token.clone()).await;
    loop {
        let result = call(&agent, "wait_for_answers", json!({ "ticket": ticket })).await;
        let pending = result
            .structured_content
            .as_ref()
            .is_some_and(|s| s["status"] == "pending");
        print_result(ticket, &result);
        if !pending {
            return;
        }
    }
}

async fn connect(url: &str, token: Option<String>) -> RunningService<RoleClient, ClientConfig> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_owned());
    if let Some(token) = token {
        config = config.auth_header(token);
    }
    let info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("question-desk-demo", env!("CARGO_PKG_VERSION"))
            .with_title("Demo client"),
    );
    let lifecycle = ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    };
    match info
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(config),
            lifecycle,
        )
        .await
    {
        Ok(agent) => agent,
        Err(e) => {
            eprintln!(
                "cannot connect to {url}: {e}\nIs the Question Desk running? Is the token right?"
            );
            std::process::exit(1);
        }
    }
}

async fn call(
    agent: &RunningService<RoleClient, ClientConfig>,
    tool: &str,
    args: Value,
) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_owned())
        .with_arguments(args.as_object().expect("arguments are an object").clone());
    match agent.call_tool(params).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{tool} failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Prints the whole result at once, so concurrent rounds do not interleave.
fn print_result(ticket: &str, result: &CallToolResult) {
    let error = if result.is_error == Some(true) {
        " (isError)"
    } else {
        ""
    };
    let mut out = format!("\n===== result for {ticket}{error} =====\n");
    for content in &result.content {
        match content.as_text() {
            Some(text) => out += &format!("{}\n", text.text),
            None => out += &format!("{content:?}\n"),
        }
    }
    if let Some(structured) = &result.structured_content {
        out += "----- structuredContent -----\n";
        out += &format!(
            "{}\n",
            serde_json::to_string_pretty(structured).unwrap_or_default()
        );
    }
    out += "=====";
    println!("{out}");
}

fn suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!("{:04x}", nanos & 0xffff)
}

// ---------------------------------------------------------------------------------------
// Sample rounds: (name, source, arguments without ticket/source)

type Round = fn() -> Value;

const ROUNDS: &[(&str, &str, Round)] = &[
    ("grilling", "~/code/shop-backend", grilling),
    ("features", "~/code/shop-frontend", features),
    ("previews", "~/code/design-system", previews),
    ("emoji", "~/code/team-offsite", emoji),
    ("open", "~/code/billing-service", open_questions),
];

fn grilling() -> Value {
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
                "question": "How should we version event schemas?",
                "header": "Versioning",
                "body": "Events are stored forever. Old events must stay readable after schema changes.\n\n- **Upcasting** converts old events on read.\n- **Versioned types** keep `OrderPlacedV1`, `OrderPlacedV2` side by side.\n- **Weak schema** tolerates missing fields everywhere.",
                "options": [
                    { "label": "Upcasting", "description": "One current type per event; upcasters migrate on read." },
                    { "label": "Versioned types", "description": "Every version is its own type; handlers deal with all of them." },
                    { "label": "Weak schema", "description": "Optional fields and defaults; no explicit versions." }
                ],
                "recommended": "Upcasting"
            },
            {
                "question": "Do we need snapshots from day one?",
                "header": "Snapshots",
                "body": "Our largest aggregate (a customer account) has ~400 events after two years. Replaying 400 events takes ~3 ms.",
                "options": [
                    { "label": "No", "description": "Add them when an aggregate gets slow." },
                    { "label": "Yes", "description": "Snapshot every 100 events from the start." }
                ],
                "recommended": "No"
            },
            {
                "question": "Where should projections run?",
                "header": "Projections",
                "options": [
                    { "label": "In-process", "description": "Same service, same deploy; simplest." },
                    { "label": "Separate worker", "description": "Own deploy; can be rebuilt without touching writes." }
                ],
                "recommended": "Separate worker, but share the code with the service so we can move later"
            },
            {
                "question": "How strict should optimistic concurrency be?",
                "header": "Concurrency",
                "body": "Two commands on the same stream at once: the second one fails on a version conflict.",
                "options": [
                    { "label": "Fail and retry", "description": "Reload the aggregate and retry up to 3 times." },
                    { "label": "Fail to caller", "description": "Return 409 and let the client decide." }
                ],
                "recommended": "Fail and retry"
            },
            {
                "question": "Which delivery guarantee do subscribers get?",
                "header": "Delivery",
                "options": [
                    { "label": "At least once", "description": "Subscribers must be idempotent." },
                    { "label": "Exactly once", "description": "Transactional outbox plus dedup table; more moving parts." }
                ],
                "recommended": "At least once"
            },
            {
                "question": "Anything we have not covered that worries you?",
                "header": "Other",
                "body": "Free text. Leave empty if nothing comes to mind."
            }
        ]
    })
}

fn features() -> Value {
    json!({
        "title": "Release 2.4 scope",
        "questions": [
            {
                "question": "Which features go into 2.4?",
                "header": "Scope",
                "multiSelect": true,
                "body": "We have **three weeks**. Each feature is roughly one week for one person; we are two.",
                "options": [
                    { "label": "Wishlist", "description": "Save products for later; most requested by customers." },
                    { "label": "Gift cards", "description": "Needs billing changes; marketing wants it for December." },
                    { "label": "Dark mode", "description": "Mostly CSS variables; design is done." },
                    { "label": "Saved searches", "description": "Email alerts for new matches; needs a scheduler." },
                    { "label": "Order export", "description": "CSV export for B2B customers." },
                    { "label": "Faster checkout", "description": "One-page checkout; touches payment flows." }
                ],
                "recommended": "Wishlist, Gift cards and Dark mode"
            },
            {
                "question": "Who should review the payment changes?",
                "header": "Review",
                "options": [
                    { "label": "Anna", "description": "Wrote the payment module." },
                    { "label": "Ben", "description": "Knows the PSP integration." }
                ],
                "recommended": "Anna"
            }
        ]
    })
}

fn previews() -> Value {
    json!({
        "title": "Button API",
        "questions": [
            {
                "question": "Which button API should the design system expose?",
                "header": "API",
                "body": "Hover an option to see its usage. The preview panel moves below the options in a narrow window.",
                "options": [
                    {
                        "label": "Variant prop",
                        "description": "One component, a `variant` string.",
                        "preview": "<Button variant=\"primary\" size=\"lg\" onClick={save}>\n  Save\n</Button>\n\n<Button variant=\"danger\">Delete</Button>"
                    },
                    {
                        "label": "Separate components",
                        "description": "One component per variant.",
                        "preview": "<PrimaryButton size=\"lg\" onClick={save}>\n  Save\n</PrimaryButton>\n\n<DangerButton>Delete</DangerButton>"
                    },
                    {
                        "label": "Builder",
                        "description": "Chained configuration.",
                        "preview": "button()\n  .primary()\n  .large()\n  .onClick(save)\n  .label(\"Save\")"
                    }
                ],
                "recommended": "Variant prop"
            },
            {
                "question": "How should the layout look on mobile?",
                "header": "Layout",
                "options": [
                    {
                        "label": "Stacked",
                        "description": "Full-width buttons, primary on top.",
                        "preview": "┌────────────────────┐\n│ [      Save      ] │\n│ [     Cancel     ] │\n└────────────────────┘"
                    },
                    {
                        "label": "Side by side",
                        "description": "Like desktop, smaller.",
                        "preview": "┌────────────────────┐\n│ [Cancel]   [Save]  │\n└────────────────────┘"
                    }
                ]
            }
        ]
    })
}

fn emoji() -> Value {
    json!({
        "title": "Team-Offsite 🎉",
        "questions": [
            {
                "question": "Wohin geht das Offsite? 🗺️",
                "header": "Ort",
                "body": "Zur Auswahl stehen drei Orte. Bitte antworte gern mit Emoji 😄 (Ctrl+Cmd+Space) und Umlauten: ä ö ü ß.",
                "options": [
                    { "label": "Ostsee 🌊", "description": "Strand, Fischbrötchen, 3 h mit dem Zug." },
                    { "label": "Alpen 🏔️", "description": "Wandern, Hütte, 6 h mit dem Zug." },
                    { "label": "Berlin 🇩🇪", "description": "Kein Reiseaufwand, Büro um die Ecke." }
                ],
                "recommended": "Ostsee 🌊"
            },
            {
                "question": "Welche Aktivitäten? 🧗",
                "header": "Programm",
                "multiSelect": true,
                "options": [
                    { "label": "Kochkurs 👩‍🍳", "description": "Gemeinsam kochen." },
                    { "label": "Escape Room 🔐", "description": "Teamwork unter Zeitdruck." },
                    { "label": "Wanderung 🥾", "description": "Halber Tag draußen." },
                    { "label": "Hackathon 💻", "description": "Ein Tag an eigenen Ideen." }
                ]
            },
            {
                "question": "Gibt es Essenswünsche? 🥗",
                "header": "Essen"
            }
        ]
    })
}

fn open_questions() -> Value {
    json!({
        "title": "Billing migration",
        "questions": [
            {
                "question": "What should happen to invoices that are still open during the migration?",
                "header": "Open invoices",
                "body": "About 1,200 invoices are open at any time. The old system keeps running read-only for 90 days."
            },
            {
                "question": "Who needs to sign off before we switch?",
                "header": "Sign-off",
                "recommended": "Finance lead and the on-call engineer"
            },
            {
                "question": "Is there a deadline we must not miss?",
                "header": "Deadline"
            }
        ]
    })
}
