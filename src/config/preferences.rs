//! Local preference commands only report success after persistence succeeds.

use super::{Config, ConfigResult, DEFAULT_MODEL};
use crate::theme::Theme;

/// Parses the `auto on` / `auto off` toggle command.
pub(crate) fn parse_auto_toggle(input: &str) -> Option<bool> {
    match input.trim().to_lowercase().as_str() {
        "auto on" => Some(true),
        "auto off" => Some(false),
        _ => None,
    }
}

pub(crate) fn auto_mode_description(enabled: bool) -> &'static str {
    if enabled {
        "Auto mode ON — eligible commands Jev confidently assesses as read-only run without confirmation"
    } else {
        "Auto mode OFF — every generated command asks for confirmation"
    }
}

#[derive(Debug)]
pub(crate) enum ModelCommand {
    Show,
    Set(String),
    Reset,
}

/// Parses the `model` / `model <id>` / `model reset` command. Anything with
/// more than one argument is treated as an ordinary prompt, not a command.
pub(crate) fn parse_model_command(input: &str) -> Option<ModelCommand> {
    let mut tokens = input.split_whitespace();
    if !tokens.next()?.eq_ignore_ascii_case("model") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(ModelCommand::Show);
    };
    if tokens.next().is_some() {
        return None;
    }
    match arg.to_lowercase().as_str() {
        "reset" | "default" => Some(ModelCommand::Reset),
        _ => Some(ModelCommand::Set(arg.to_string())),
    }
}

enum AutoPreferenceState {
    Loading,
    Saving(Config),
    Reporting,
    Complete,
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn set_auto_mode(enabled: bool, theme: &Theme) -> ConfigResult<()> {
    let mut state = AutoPreferenceState::Loading;
    loop {
        state = match state {
            AutoPreferenceState::Loading => {
                let mut config = Config::load();
                config.auto = enabled;
                AutoPreferenceState::Saving(config)
            }
            AutoPreferenceState::Saving(config) => match config.save() {
                Ok(()) => AutoPreferenceState::Reporting,
                Err(error) => AutoPreferenceState::Failed(
                    format!("Could not save auto preference: {error}").into(),
                ),
            },
            AutoPreferenceState::Reporting => {
                println!("{}", theme.helper_text(auto_mode_description(enabled)));
                AutoPreferenceState::Complete
            }
            AutoPreferenceState::Complete => return Ok(()),
            AutoPreferenceState::Failed(error) => return Err(error),
        };
    }
}

enum ModelChange {
    Set(String),
    Reset,
}

enum ModelReport {
    Current { source: &'static str },
    Saved(ModelChange),
}

enum ModelPreferenceState {
    Loading,
    Applying(Config),
    Saving { config: Config, change: ModelChange },
    Reporting(ModelReport),
    Complete(String),
    Failed(Box<dyn std::error::Error>),
}

/// Returns the active model only after a successful show/save, so the session
/// can apply the result without changing its settings on a failed save.
pub(crate) fn apply_model_command(
    command: ModelCommand,
    theme: &Theme,
    active_model: &str,
) -> ConfigResult<String> {
    let mut state = ModelPreferenceState::Loading;
    loop {
        state = match state {
            ModelPreferenceState::Loading => ModelPreferenceState::Applying(Config::load()),
            ModelPreferenceState::Applying(mut config) => match &command {
                ModelCommand::Show => {
                    let source = match config.model.as_deref() {
                        Some(saved) if saved == active_model => "saved in ~/.ask/config",
                        None if active_model == DEFAULT_MODEL => "built-in default",
                        _ => "current session override",
                    };
                    ModelPreferenceState::Reporting(ModelReport::Current { source })
                }
                ModelCommand::Set(model) => {
                    if !model.contains('/') {
                        println!(
                            "{}",
                            theme.helper_text(
                                "Note: OpenRouter model IDs usually look like vendor/model-name"
                            )
                        );
                    }
                    config.model = Some(model.clone());
                    ModelPreferenceState::Saving {
                        config,
                        change: ModelChange::Set(model.clone()),
                    }
                }
                ModelCommand::Reset => {
                    config.model = None;
                    ModelPreferenceState::Saving {
                        config,
                        change: ModelChange::Reset,
                    }
                }
            },
            ModelPreferenceState::Saving { config, change } => match config.save() {
                Ok(()) => ModelPreferenceState::Reporting(ModelReport::Saved(change)),
                Err(error) => ModelPreferenceState::Failed(
                    format!("Could not save model preference: {error}").into(),
                ),
            },
            ModelPreferenceState::Reporting(report) => {
                let (message, model) = match report {
                    ModelReport::Current { source } => (
                        format!("Model: {active_model} ({source})"),
                        active_model.to_string(),
                    ),
                    ModelReport::Saved(ModelChange::Set(model)) => (
                        format!("Model set to {model} and saved as the default"),
                        model,
                    ),
                    ModelReport::Saved(ModelChange::Reset) => (
                        format!("Model reset to the built-in default ({DEFAULT_MODEL})"),
                        DEFAULT_MODEL.to_string(),
                    ),
                };
                println!("{}", theme.helper_text(&message));
                ModelPreferenceState::Complete(model)
            }
            ModelPreferenceState::Complete(model) => return Ok(model),
            ModelPreferenceState::Failed(error) => return Err(error),
        };
    }
}
