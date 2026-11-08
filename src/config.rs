//! Saved preferences have one load/save owner. Loads fall back to defaults;
//! callers decide whether a save failure is fatal or only a warning.

use crate::theme::ThemeMode;
use std::env;
use std::fs;
use std::path::PathBuf;

pub(crate) mod preferences;

pub(crate) const DEFAULT_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";
type ConfigResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) theme: ThemeMode,
    pub(crate) model: Option<String>,
    pub(crate) auto: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            model: None,
            auto: false,
        }
    }
}

enum LoadState {
    ResolvingPath,
    Reading(PathBuf),
    Parsing(String),
    Complete(Config),
}

enum SaveState {
    ResolvingPath,
    CreatingDirectory(PathBuf),
    Serializing(PathBuf),
    Writing { path: PathBuf, contents: String },
    Complete,
    Failed(Box<dyn std::error::Error>),
}

impl Config {
    pub(crate) fn load() -> Self {
        Self::load_with(config_path)
    }

    fn load_with(resolve_path: impl Fn() -> Option<PathBuf>) -> Self {
        let mut state = LoadState::ResolvingPath;
        loop {
            state = match state {
                LoadState::ResolvingPath => match resolve_path() {
                    Some(path) => LoadState::Reading(path),
                    None => LoadState::Complete(Self::default()),
                },
                LoadState::Reading(path) => match fs::read_to_string(path) {
                    Ok(contents) => LoadState::Parsing(contents),
                    Err(_) => LoadState::Complete(Self::default()),
                },
                LoadState::Parsing(contents) => {
                    let mut config = Self::default();
                    for line in contents.lines() {
                        if let Some(value) = line.strip_prefix("theme=") {
                            if let Some(theme) = ThemeMode::from_str(value.trim()) {
                                config.theme = theme;
                            }
                        } else if let Some(value) = line.strip_prefix("model=") {
                            let value = value.trim();
                            if !value.is_empty() {
                                config.model = Some(value.to_string());
                            }
                        } else if let Some(value) = line.strip_prefix("auto=")
                            && let Some(auto) = parse_on_off(value.trim())
                        {
                            config.auto = auto;
                        }
                    }
                    LoadState::Complete(config)
                }
                LoadState::Complete(config) => return config,
            };
        }
    }

    pub(crate) fn save(&self) -> ConfigResult<()> {
        self.save_with(config_path)
    }

    fn save_with(&self, resolve_path: impl Fn() -> Option<PathBuf>) -> ConfigResult<()> {
        let mut state = SaveState::ResolvingPath;
        loop {
            state = match state {
                SaveState::ResolvingPath => match resolve_path() {
                    Some(path) => SaveState::CreatingDirectory(path),
                    None => SaveState::Failed("HOME is not set; cannot save ~/.ask/config.".into()),
                },
                SaveState::CreatingDirectory(path) => {
                    let result = path.parent().map_or(Ok(()), fs::create_dir_all);
                    match result {
                        Ok(()) => SaveState::Serializing(path),
                        Err(error) => SaveState::Failed(error.into()),
                    }
                }
                SaveState::Serializing(path) => {
                    let mut contents = format!("theme={}\n", self.theme.as_str());
                    if let Some(model) = &self.model {
                        contents.push_str(&format!("model={model}\n"));
                    }
                    contents.push_str(&format!("auto={}\n", if self.auto { "on" } else { "off" }));
                    SaveState::Writing { path, contents }
                }
                SaveState::Writing { path, contents } => match fs::write(path, contents) {
                    Ok(()) => SaveState::Complete,
                    Err(error) => SaveState::Failed(error.into()),
                },
                SaveState::Complete => return Ok(()),
                SaveState::Failed(error) => return Err(error),
            };
        }
    }
}

pub(crate) fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn config_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".ask").join("config"))
}

#[cfg(test)]
mod tests;
