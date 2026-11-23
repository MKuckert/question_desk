//! The user's settings: `config.toml` in `~/.config/question-desk/` (Windows: `%APPDATA%`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    desk::StorageSettings,
    server::{DEFAULT_HOST, DEFAULT_PORT, ServerSettings},
    store,
};

/// Finished rounds are pruned this many days after they finished, unless configured.
pub const DEFAULT_RETENTION_DAYS: u32 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address or name the MCP server listens on.
    pub host: String,
    /// Port the MCP server listens on.
    pub port: u16,
    /// If set, clients must send `Authorization: Bearer <token>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    /// Days after which finished rounds are deleted; `0` keeps them forever.
    pub retention_days: u32,
    /// Where rounds are stored; the XDG data dir if unset. May start with `~/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    /// Whether rounds are written to `data_dir` at all.
    pub persistence: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_PORT,
            bearer_token: None,
            retention_days: DEFAULT_RETENTION_DAYS,
            data_dir: None,
            persistence: true,
        }
    }
}

impl Config {
    /// The directory rounds are stored in: `data_dir` with `~` expanded, else the default.
    pub fn data_dir(&self) -> Option<PathBuf> {
        let Some(dir) = &self.data_dir else {
            return store::default_data_dir();
        };
        match dir.strip_prefix("~") {
            Ok(rest) => directories::BaseDirs::new().map(|d| d.home_dir().join(rest)),
            Err(_) => Some(dir.clone()),
        }
    }

    pub fn storage(&self) -> StorageSettings {
        StorageSettings {
            persistence: self.persistence,
            data_dir: self.data_dir(),
        }
    }

    pub fn server_settings(&self) -> ServerSettings {
        ServerSettings {
            host: self.host.clone(),
            port: self.port,
            bearer_token: self.bearer_token.clone(),
            ..ServerSettings::default()
        }
    }
}

/// `~/.config/question-desk/config.toml` on macOS and Linux (under `$XDG_CONFIG_HOME` if
/// set), `%APPDATA%\question-desk\config\config.toml` on Windows.
pub fn path() -> Option<PathBuf> {
    if cfg!(windows) {
        return directories::ProjectDirs::from("", "", "question-desk")
            .map(|d| d.config_dir().join("config.toml"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))?;
    Some(base.join("question-desk").join("config.toml"))
}

/// Loads the config file; defaults when it does not exist.
pub fn load() -> Result<Config, String> {
    match path() {
        Some(path) => load_from(&path),
        None => Ok(Config::default()),
    }
}

/// Loads the config file at `path`; defaults when it does not exist.
pub fn load_from(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Writes `config` to [`path`]; only the owner may read it, as it may hold the token.
pub fn save(config: &Config) -> Result<PathBuf, String> {
    let path = path().ok_or("no home directory to save the settings in")?;
    write(&path, config).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

fn write(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(config).expect("config serializes");
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temp, path)
}

pub fn parse(text: &str) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    validate(&config)?;
    Ok(config)
}

pub fn validate(config: &Config) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("`host` must not be empty".into());
    }
    if config.port == 0 {
        return Err("`port` must be between 1 and 65535".into());
    }
    if let Some(dir) = &config.data_dir
        && !(dir.is_absolute() || dir.starts_with("~"))
    {
        return Err(
            "`data_dir` must be an absolute path or start with `~/`; remove it for the default"
                .into(),
        );
    }
    if let Some(token) = &config.bearer_token {
        if token.trim().is_empty() {
            return Err("`bearer_token` must not be empty; remove it to disable the token".into());
        }
        if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("`bearer_token` must not contain spaces or control characters".into());
        }
    }
    Ok(())
}
