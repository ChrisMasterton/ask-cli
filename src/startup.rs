//! Startup commits parsed theme preferences, dispatches local work, and only
//! then acquires the API key and piped context required by an AI mode.

use crate::cli::{self, Args, ArgumentOutcome};
use crate::config::Config;
use crate::config::preferences::{
    ModelCommand, apply_model_command, parse_auto_toggle, parse_model_command, set_auto_mode,
};
use crate::prompt::process_prompt;
use crate::session::run_interactive_mode;
use crate::theme::Theme;
use crate::{MAX_PIPE_BYTES, tools};
use std::env;
use std::io::{self, IsTerminal, Read};

type StartupResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
enum LocalCommand {
    Auto(bool),
    Model(ModelCommand),
    SavedTool { name: String, args_tail: String },
    ManageTool(tools::ToolCommand),
}

#[derive(Debug, PartialEq, Eq)]
enum AiMode {
    Prompt {
        prompt: String,
        piped_data: Option<String>,
    },
    Interactive,
}

enum StartupAction {
    Help,
    Local {
        args: Args,
        command: LocalCommand,
    },
    Ai {
        args: Args,
        api_key: String,
        mode: AiMode,
    },
}

trait StartupIo {
    fn load_config(&mut self) -> Config;
    fn save_config(&mut self, config: &Config) -> StartupResult<()>;
    fn api_key(&mut self) -> StartupResult<String>;
    fn piped_input(&mut self) -> Option<String>;
    fn execute(&mut self, action: StartupAction) -> StartupResult<()>;
}

enum StartupState {
    LoadingConfig,
    Parsing(Config),
    SavingTheme {
        args: Args,
        config: Config,
    },
    Dispatching(Args),
    RequiringKey(Args),
    ReadingPipe {
        args: Args,
        api_key: String,
    },
    SelectingMode {
        args: Args,
        api_key: String,
        piped_data: Option<String>,
    },
    Executing(StartupAction),
    Complete,
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn run() -> StartupResult<()> {
    run_startup_with(env::args().skip(1), &mut SystemStartup)
}

fn run_startup_with(
    mut arguments: impl Iterator<Item = String>,
    io: &mut impl StartupIo,
) -> StartupResult<()> {
    let mut state = StartupState::LoadingConfig;
    loop {
        state = match state {
            StartupState::LoadingConfig => StartupState::Parsing(io.load_config()),
            StartupState::Parsing(config) => match cli::parse_args(&mut arguments, config) {
                Ok(ArgumentOutcome::Help) => StartupState::Executing(StartupAction::Help),
                Ok(ArgumentOutcome::Run {
                    args,
                    theme_preference: Some(config),
                }) => StartupState::SavingTheme { args, config },
                Ok(ArgumentOutcome::Run {
                    args,
                    theme_preference: None,
                }) => StartupState::Dispatching(args),
                Err(error) => StartupState::Failed(error),
            },
            StartupState::SavingTheme { args, config } => {
                if let Err(error) = io.save_config(&config) {
                    eprintln!("Warning: could not save theme preference: {error}");
                }
                StartupState::Dispatching(args)
            }
            StartupState::Dispatching(args) => match args.prompt.as_deref().and_then(local_command)
            {
                Some(command) => StartupState::Executing(StartupAction::Local { args, command }),
                None => StartupState::RequiringKey(args),
            },
            StartupState::RequiringKey(args) => match io.api_key() {
                Ok(api_key) => StartupState::ReadingPipe { args, api_key },
                Err(error) => StartupState::Failed(error),
            },
            StartupState::ReadingPipe { args, api_key } => StartupState::SelectingMode {
                args,
                api_key,
                piped_data: io.piped_input(),
            },
            StartupState::SelectingMode {
                mut args,
                api_key,
                piped_data,
            } => {
                let mode = match (args.prompt.take(), piped_data) {
                    (Some(prompt), piped_data) => AiMode::Prompt { prompt, piped_data },
                    (None, Some(piped_data)) => AiMode::Prompt {
                        prompt: "Summarize and explain this data".to_string(),
                        piped_data: Some(piped_data),
                    },
                    (None, None) => AiMode::Interactive,
                };
                StartupState::Executing(StartupAction::Ai {
                    args,
                    api_key,
                    mode,
                })
            }
            StartupState::Executing(action) => match io.execute(action) {
                Ok(()) => StartupState::Complete,
                Err(error) => StartupState::Failed(error),
            },
            StartupState::Complete => return Ok(()),
            StartupState::Failed(error) => return Err(error),
        };
    }
}

fn local_command(prompt: &str) -> Option<LocalCommand> {
    if let Some(enabled) = parse_auto_toggle(prompt) {
        Some(LocalCommand::Auto(enabled))
    } else if let Some(command) = parse_model_command(prompt) {
        Some(LocalCommand::Model(command))
    } else if let Some((name, args_tail)) = tools::parse_tool_invocation(prompt) {
        Some(LocalCommand::SavedTool { name, args_tail })
    } else {
        tools::parse_tool_command(prompt).map(LocalCommand::ManageTool)
    }
}

struct SystemStartup;

impl StartupIo for SystemStartup {
    fn load_config(&mut self) -> Config {
        Config::load()
    }

    fn save_config(&mut self, config: &Config) -> StartupResult<()> {
        config.save()
    }

    fn api_key(&mut self) -> StartupResult<String> {
        env::var("OPENROUTER_ASK_API_KEY")
            .map_err(|_| "Please set the OPENROUTER_ASK_API_KEY environment variable.".into())
    }

    fn piped_input(&mut self) -> Option<String> {
        read_piped_input(io::stdin().is_terminal(), io::stdin().lock())
    }

    fn execute(&mut self, action: StartupAction) -> StartupResult<()> {
        match action {
            StartupAction::Help => cli::print_help(),
            StartupAction::Local { args, command } => {
                let theme = Theme::from_mode(args.theme);
                match command {
                    LocalCommand::Auto(enabled) => set_auto_mode(enabled, &theme)?,
                    LocalCommand::Model(command) => {
                        apply_model_command(command, &theme, &args.model)?;
                    }
                    LocalCommand::SavedTool { name, args_tail } => {
                        let execution = tools::invocation::invoke_tool(
                            &name,
                            &args_tail,
                            tools::invocation::InvocationOrigin::User,
                            |command| {
                                println!(
                                    "{} {}",
                                    theme.prompt_text("run>"),
                                    theme.command_text(command)
                                )
                            },
                        )?;
                        execution.result?;
                    }
                    LocalCommand::ManageTool(command) => {
                        tools::handle_tool_command(command, &args.model, &theme)?;
                    }
                }
            }
            StartupAction::Ai {
                args,
                api_key,
                mode,
            } => {
                let theme = Theme::from_mode(args.theme);
                match mode {
                    AiMode::Prompt { prompt, piped_data } => {
                        process_prompt(
                            &prompt,
                            &args.model,
                            &api_key,
                            &theme,
                            piped_data.as_deref(),
                            args.auto,
                        )?;
                    }
                    AiMode::Interactive => {
                        run_interactive_mode(&args.model, &api_key, &theme, args.auto)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Keep one sentinel byte beyond the prompt limit. Read errors retain partial
/// data, and invalid UTF-8 is replaced just as it is for other captured output.
fn read_piped_input(is_terminal: bool, reader: impl Read) -> Option<String> {
    if is_terminal {
        return None;
    }
    let mut buffer = Vec::with_capacity(8192);
    let _ = reader
        .take(MAX_PIPE_BYTES as u64 + 1)
        .read_to_end(&mut buffer);
    if buffer.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&buffer).to_string())
    }
}

#[cfg(test)]
mod tests;
