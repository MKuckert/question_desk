//! The Settings window (App menu ▸ Settings…, ⌘, / the toolbar's gear): host, port and access
//! token of the MCP server. Saving restarts the server.

use day::prelude::*;
use day::reactive::Signal;

use super::{apply_config, config, home_relative, is_loopback, native};
use crate::{
    config::{self as cfg, Config},
    server::{DEFAULT_HOST, DEFAULT_PORT},
};

pub(crate) fn settings_page() -> impl Piece {
    let current = config();
    let host = Signal::new(current.host.clone());
    let port = Signal::new(current.port.to_string());
    let token = Signal::new(current.bearer_token.clone().unwrap_or_default());
    let show_token = Signal::new(false);
    let error = Signal::new(None::<String>);
    let window = day::current_window();

    let close = {
        let window = window.clone();
        move || {
            if let Some(window) = &window {
                window.close();
            }
        }
    };
    let save = move || {
        let form = Form {
            host: host.get_untracked(),
            port: port.get_untracked(),
            token: token.get_untracked(),
        };
        match form.config().and_then(apply_config) {
            Ok(()) => {
                if let Some(window) = &window {
                    window.close();
                }
            }
            Err(e) => error.set(Some(e)),
        }
    };
    let warnings = move || {
        let mut lines = Vec::new();
        if !is_loopback(&host.get()) {
            let mut warning = "Other machines can reach this address.".to_owned();
            if token.get().trim().is_empty() {
                warning.push_str(" Set an access token.");
            }
            lines.push(format!("⚠ {warning}"));
        }
        if let Some(e) = error.get() {
            lines.push(format!("⚠ {e}"));
        }
        lines.join("\n")
    };

    column((
        label(
            "Where Agents reach the MCP server. Saving restarts the server; Agents already \
             waiting for answers stay connected.",
        )
        .secondary(),
        form((section((
            labeled(
                "Host",
                text_field(host).placeholder(DEFAULT_HOST).grow(Dimension::Width),
            ),
            labeled(
                "Port",
                text_field(port)
                    .placeholder(DEFAULT_PORT.to_string())
                    .width(96.0),
            ),
            labeled(
                "Access token",
                switch(
                    move || show_token.get(),
                    move |&shown| {
                        let hint = "None: every local client may connect";
                        if shown {
                            text_field(token).placeholder(hint).grow(Dimension::Width).any()
                        } else {
                            secure_field(token).placeholder(hint).grow(Dimension::Width).any()
                        }
                    },
                )
                .grow(Dimension::Width),
            ),
            labeled(
                "",
                native::choice(
                    "Show token".to_owned(),
                    true,
                    move || show_token.get(),
                    move || show_token.update(|s| *s = !*s),
                ),
            ),
        )),)),
        when(move || !warnings().is_empty(), move || {
            label(warnings).color(SystemColor::Warning)
        }),
        row((
            spacer(),
            button("Cancel").action(close),
            button("Save").prominent().action(save),
        ))
        .spacing(Space::S),
        cfg::path().map(|path| {
            label(format!("Saved in {}", home_relative(&path)))
                .font(Font::Caption)
                .secondary()
                .selectable()
        }),
    ))
    .spacing(Space::M)
    .padding(Space::L)
}

/// The settings window's fields, as typed.
struct Form {
    host: String,
    port: String,
    token: String,
}

impl Form {
    /// The config the fields describe, or what is wrong with them.
    fn config(&self) -> Result<Config, String> {
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|&p| p != 0)
            .ok_or("The port must be a number between 1 and 65535.")?;
        let token = self.token.trim();
        let config = Config {
            host: self.host.trim().to_owned(),
            port,
            bearer_token: (!token.is_empty()).then(|| token.to_owned()),
        };
        cfg::validate(&config)?;
        Ok(config)
    }
}
