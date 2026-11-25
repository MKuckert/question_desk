//! The Settings dialog: host, port and access token of the MCP server. A native modal
//! dialog with the platform's button order; saving restarts the server.

use std::{cell::RefCell, net::IpAddr, rc::Rc};

use wxdragon::prelude::*;

use super::style;
use crate::{
    config::{self, Config},
    server::{DEFAULT_HOST, DEFAULT_PORT},
};

/// The settings dialog's fields, as typed.
pub struct SettingsForm {
    pub host: String,
    pub port: String,
    pub token: String,
}

impl SettingsForm {
    /// The config the fields describe, or what is wrong with them.
    pub fn config(&self) -> Result<Config, String> {
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
        config::validate(&config)?;
        Ok(config)
    }

    /// Warnings shown under the fields while typing.
    fn warning(&self) -> Option<String> {
        if is_loopback(&self.host) || self.host.trim().is_empty() {
            return None;
        }
        let mut warning = "Other machines can reach this address.".to_owned();
        if self.token.trim().is_empty() {
            warning.push_str(" Set an access token.");
        }
        Some(warning)
    }
}

/// Shows the dialog modally. `save` applies a validated config and says why it failed, if
/// it did; the dialog stays open until a save succeeds or the user cancels.
pub fn show(parent: &Frame, config: &Config, save: impl Fn(&Config) -> Result<(), String> + 'static) {
    let dialog = Dialog::builder(parent, "Settings").build();
    let gap = dialog.from_dip_int(8);
    let root = BoxSizer::builder(Orientation::Vertical).build();

    let intro = StaticText::builder(&dialog)
        .with_label(
            "Where Agents reach the MCP server. Saving restarts the server; \
             Agents already waiting for answers stay connected.",
        )
        .build();
    intro.wrap(dialog.from_dip_int(440));
    root.add(&intro, 0, SizerFlag::All | SizerFlag::Expand, gap * 2);

    let grid = FlexGridSizer::builder(0, 2).with_vgap(gap).with_hgap(gap * 2).build();
    grid.add_growable_col(1, 1);
    let label = |text: &str| {
        StaticText::builder(&dialog)
            .with_label(text)
            .build()
    };
    let host = TextCtrl::builder(&dialog).with_value(&config.host).build();
    let port = TextCtrl::builder(&dialog)
        .with_value(&config.port.to_string())
        .with_size(dialog.from_dip(Size::new(96, -1)))
        .build();
    let token = TextCtrl::builder(&dialog)
        .with_value(config.bearer_token.as_deref().unwrap_or_default())
        .with_style(TextCtrlStyle::Password)
        .build();
    host.set_tooltip(&format!("Default: {DEFAULT_HOST}"));
    port.set_tooltip(&format!("Default: {DEFAULT_PORT}"));
    token.set_tooltip("Empty: every local client may connect");
    let show_token = CheckBox::builder(&dialog).with_label("Show token").build();
    let align = SizerFlag::AlignCenterVertical;
    grid.add(&label("Host:"), 0, align | SizerFlag::AlignRight, 0);
    grid.add(&host, 1, SizerFlag::Expand, 0);
    grid.add(&label("Port:"), 0, align | SizerFlag::AlignRight, 0);
    grid.add(&port, 0, SizerFlag::AlignLeft, 0);
    grid.add(&label("Access token:"), 0, align | SizerFlag::AlignRight, 0);
    grid.add(&token, 1, SizerFlag::Expand, 0);
    grid.add(&StaticText::builder(&dialog).build(), 0, align, 0);
    grid.add(&show_token, 0, SizerFlag::AlignLeft, 0);
    root.add_sizer(&grid, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, gap * 2);

    let warning = StaticText::builder(&dialog).build();
    warning.set_foreground_color(style::warning_colour());
    warning.show(false);
    root.add(&warning, 0, SizerFlag::All | SizerFlag::Expand, gap * 2);

    if let Some(path) = config::path() {
        let saved = StaticText::builder(&dialog)
            .with_label(&format!("Saved in {}", home_relative(&path)))
            .build();
        style::small(&saved);
        saved.wrap(dialog.from_dip_int(440));
        saved.set_foreground_color(style::muted_colour());
        root.add(&saved, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand, gap * 2);
    }

    let buttons = StdDialogButtonSizerBuilder::new().build();
    let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Save").build();
    let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
    ok.set_default();
    buttons.add_button(&ok);
    buttons.add_button(&cancel);
    buttons.realize();
    root.add_sizer(&buttons, 0, SizerFlag::All | SizerFlag::Expand, gap * 2);

    let error: Rc<RefCell<Option<String>>> = Rc::default();
    let form = move || SettingsForm {
        host: host.get_value(),
        port: port.get_value(),
        token: token.get_value(),
    };
    let update_warning = {
        let error = error.clone();
        move || {
            let mut lines: Vec<String> = form().warning().into_iter().collect();
            lines.extend(error.borrow().clone());
            let text = lines
                .iter()
                .map(|l| format!("⚠ {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            if warning.get_label() != text {
                warning.set_label(&text);
                warning.wrap(dialog.from_dip_int(440));
                warning.show(!text.is_empty());
                dialog.fit();
                dialog.layout();
            }
        }
    };
    let update_warning = Rc::new(update_warning);
    for field in [host, port, token] {
        let update = update_warning.clone();
        let error = error.clone();
        field.on_text_changed(move |_| {
            error.borrow_mut().take();
            update();
        });
    }
    show_token.on_toggled(move |e| token.set_password_mode(!e.is_checked()));
    {
        let update = update_warning.clone();
        let error = error.clone();
        ok.on_click(move |_| {
            let result = form().config().and_then(|config| save(&config));
            match result {
                Ok(()) => dialog.end_modal(ID_OK),
                Err(e) => {
                    *error.borrow_mut() = Some(e);
                    update();
                }
            }
        });
    }

    dialog.set_sizer_and_fit(root, true);
    update_warning();
    dialog.centre();
    host.set_focus();
    dialog.show_modal();
    dialog.destroy();
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
