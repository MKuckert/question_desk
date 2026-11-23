//! The round store: one JSON file per Question Round in `<data_dir>/rounds/` (ADR 0007).
//!
//! Files carry a `version` and are migrated in memory on load. Unparseable files and files
//! of a newer version are skipped and never written, renamed or removed.

use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};

use serde::{Deserialize, Serialize};

use crate::schema::Question;

/// The version this build writes.
pub const VERSION: u64 = 1;

/// One Question Round as stored on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredRound {
    /// The file name without `.json`: the Ticket, plus `@<n>` for later rounds that reuse it.
    pub id: String,
    pub ticket: String,
    /// The Agent Session label.
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub questions: Vec<Question>,
    /// One per Question, in order: drafts while open, the Answers once finished.
    pub answers: Vec<DraftAnswer>,
    /// The Question the user is on; `None` for the Review Page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    pub status: Status,
    pub opened_at: jiff::Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<jiff::Timestamp>,
    /// When the result was first Delivered; `None` while Awaiting Pickup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<jiff::Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftAnswer {
    /// Labels of the selected Options, in Option order.
    #[serde(default)]
    pub selected: Vec<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Answered,
    Cancelled,
}

#[derive(Debug)]
pub enum LoadError {
    /// Written by a newer build; left alone so that build can still read it.
    Newer(u64),
    Unreadable(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Newer(v) => write!(f, "written by a newer version (format {v})"),
            LoadError::Unreadable(e) => write!(f, "unreadable: {e}"),
        }
    }
}

#[derive(Serialize)]
struct Versioned<'a> {
    version: u64,
    #[serde(flatten)]
    round: &'a StoredRound,
}

/// The file contents for `round`; also what the JSON export writes.
pub fn to_json(round: &StoredRound) -> String {
    let mut text = serde_json::to_string_pretty(&Versioned {
        version: VERSION,
        round,
    })
    .expect("rounds serialize");
    text.push('\n');
    text
}

/// Parses a round file, migrating older versions.
pub fn from_json(text: &str) -> Result<StoredRound, LoadError> {
    let mut value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| LoadError::Unreadable(e.to_string()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| LoadError::Unreadable("not a JSON object".into()))?;
    let version = object
        .remove("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| LoadError::Unreadable("no `version`".into()))?;
    if version > VERSION {
        return Err(LoadError::Newer(version));
    }
    let value = migrate(version, value)?;
    serde_json::from_value(value).map_err(|e| LoadError::Unreadable(e.to_string()))
}

/// Lifts a file of `version` to the current format, one version at a time.
fn migrate(version: u64, value: serde_json::Value) -> Result<serde_json::Value, LoadError> {
    match version {
        VERSION => Ok(value),
        // Add `n => migrate(n + 1, upgrade_from_n(value))` arms when the format changes.
        other => Err(LoadError::Unreadable(format!("unknown format {other}"))),
    }
}

/// What a load found.
#[derive(Debug, Default)]
pub struct Loaded {
    pub rounds: Vec<StoredRound>,
    /// Files left alone, with the reason.
    pub skipped: Vec<(PathBuf, String)>,
}

/// The round files under one data dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    data_dir: PathBuf,
}

impl Store {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn rounds_dir(&self) -> PathBuf {
        self.data_dir.join("rounds")
    }

    pub fn path_of(&self, id: &str) -> PathBuf {
        self.rounds_dir().join(format!("{id}.json"))
    }

    /// Whether a file (readable or not) already uses this id.
    pub fn exists(&self, id: &str) -> bool {
        self.path_of(id).exists()
    }

    /// Reads every round file; a missing directory is an empty store.
    pub fn load(&self) -> Loaded {
        let started = Instant::now();
        let mut loaded = Loaded::default();
        let entries = match std::fs::read_dir(self.rounds_dir()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return loaded,
            Err(e) => {
                log::error!("cannot read {}: {e}", self.rounds_dir().display());
                return loaded;
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| LoadError::Unreadable(e.to_string()))
                .and_then(|text| from_json(&text))
                .and_then(|round| {
                    if path.file_stem().is_some_and(|s| *s == *round.id) {
                        Ok(round)
                    } else {
                        Err(LoadError::Unreadable(format!(
                            "its id {:?} does not match the file name",
                            round.id
                        )))
                    }
                });
            match parsed {
                Ok(round) => loaded.rounds.push(round),
                Err(e) => {
                    log::warn!("skipping {}: {e}", path.display());
                    loaded.skipped.push((path, e.to_string()));
                }
            }
        }
        log::info!(
            "loaded {} question round(s) from {} in {:?}",
            loaded.rounds.len(),
            self.rounds_dir().display(),
            started.elapsed()
        );
        loaded
    }

    /// Writes `round` atomically (temp file + rename), readable by the owner only.
    pub fn save(&self, round: &StoredRound) -> io::Result<()> {
        let dir = self.rounds_dir();
        create_private_dir(&dir)?;
        let path = self.path_of(&round.id);
        let temp = dir.join(format!(".{}.json.tmp", round.id));
        write_private(&temp, to_json(round).as_bytes())?;
        std::fs::rename(&temp, &path)
    }

    /// Removes the file of `id`; a missing file is fine.
    pub fn remove(&self, id: &str) -> io::Result<()> {
        match std::fs::remove_file(self.path_of(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
}

/// `$XDG_DATA_HOME/question-desk` or `~/.local/share/question-desk`; on Windows the
/// platform data dir.
pub fn default_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return directories::ProjectDirs::from("", "", "question-desk")
            .map(|d| d.data_dir().to_owned());
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            directories::BaseDirs::new().map(|d| d.home_dir().join(".local").join("share"))
        })?;
    Some(base.join("question-desk"))
}
