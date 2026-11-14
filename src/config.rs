//! The user's settings: `config.toml` in the platform's config directory.

use std::path::PathBuf;

use serde::Deserialize;

use crate::server::DEFAULT_PORT;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Port on `127.0.0.1` the MCP server listens on.
    pub port: u16,
    /// If set, clients must send `Authorization: Bearer <token>`.
    pub bearer_token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bearer_token: None,
        }
    }
}

/// `…/question-desk/config.toml` in the platform's config directory, e.g.
/// `~/Library/Application Support/question-desk/config.toml` on macOS.
pub fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "question-desk")
        .map(|d| d.config_dir().join("config.toml"))
}

/// Loads the config file; defaults when it does not exist.
pub fn load() -> Result<Config, String> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn parse(text: &str) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    if config
        .bearer_token
        .as_deref()
        .is_some_and(|t| t.trim().is_empty())
    {
        return Err("`bearer_token` must not be empty; remove it to disable the token".into());
    }
    Ok(config)
}
