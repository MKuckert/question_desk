//! Window size, sidebar width and zoom, remembered between runs in `window.toml` next to
//! `config.toml` (eframe kept these in its own storage; wx leaves it to the app).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub sidebar_width: i32,
    pub sidebar_open: bool,
    pub zoom: f32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 800,
            maximized: false,
            sidebar_width: 260,
            sidebar_open: true,
            zoom: 1.0,
        }
    }
}

pub const MIN_ZOOM: f32 = 0.7;
pub const MAX_ZOOM: f32 = 2.0;

fn path() -> Option<PathBuf> {
    Some(config::path()?.parent()?.join("window.toml"))
}

pub fn load() -> WindowState {
    let state = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| toml::from_str::<WindowState>(&text).ok())
        .unwrap_or_default();
    WindowState {
        width: state.width.max(520),
        height: state.height.max(400),
        sidebar_width: state.sidebar_width.clamp(160, 600),
        zoom: state.zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        ..state
    }
}

pub fn save(state: &WindowState) {
    let Some(path) = path() else { return };
    let result = toml::to_string(state)
        .map_err(|e| e.to_string())
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, text).map_err(|e| e.to_string())
        });
    if let Err(e) = result {
        log::warn!("could not save the window state to {}: {e}", path.display());
    }
}
