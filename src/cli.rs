//! Argument parsing stops at help, an error, `--`, or a saved-tool argv tail.

use crate::config::{Config, DEFAULT_MODEL};
use crate::theme::ThemeMode;
use crate::tools;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Args {
    pub(crate) prompt: Option<String>,
    pub(crate) model: String,
    pub(crate) theme: ThemeMode,
    pub(crate) auto: bool,
}

#[derive(Debug)]
pub(crate) enum ArgumentOutcome {
    Help,
    Run {
        args: Args,
        theme_preference: Option<Config>,
    },
}

enum OptionName {
    Model,
    Theme,
}

enum ThemeSelection {
    Saved(ThemeMode),
    Override(ThemeMode),
}

impl ThemeSelection {
    fn mode(&self) -> ThemeMode {
        match self {
            Self::Saved(mode) | Self::Override(mode) => *mode,
        }
    }
}

enum ArgumentState {
    Next,
    AwaitingValue(OptionName),
    CollectingTail,
    Finalizing,
    Complete(ArgumentOutcome),
    Failed(&'static str),
}

pub(crate) fn parse_args(
    mut arguments: impl Iterator<Item = String>,
    mut config: Config,
) -> Result<ArgumentOutcome, Box<dyn std::error::Error>> {
    let mut model = config
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let mut theme = ThemeSelection::Saved(config.theme);
    let mut prompt_parts = Vec::new();
    let mut state = ArgumentState::Next;

    loop {
        state = match state {
            ArgumentState::Next => match arguments.next() {
                None => ArgumentState::Finalizing,
                Some(arg) => match arg.as_str() {
                    "-h" | "--help" => ArgumentState::Complete(ArgumentOutcome::Help),
                    "--model" => ArgumentState::AwaitingValue(OptionName::Model),
                    "--theme" => ArgumentState::AwaitingValue(OptionName::Theme),
                    "--" => ArgumentState::CollectingTail,
                    _ => {
                        let is_tool =
                            prompt_parts.is_empty() && tools::parse_tool_invocation(&arg).is_some();
                        prompt_parts.push(arg);
                        if is_tool {
                            ArgumentState::CollectingTail
                        } else {
                            ArgumentState::Next
                        }
                    }
                },
            },
            ArgumentState::AwaitingValue(option) => match (option, arguments.next()) {
                (OptionName::Model, Some(value)) => {
                    model = value;
                    ArgumentState::Next
                }
                (OptionName::Theme, Some(value)) => match ThemeMode::from_str(&value) {
                    Some(mode) => {
                        theme = ThemeSelection::Override(mode);
                        ArgumentState::Next
                    }
                    None => ArgumentState::Failed("Invalid theme. Use 'light' or 'dark'."),
                },
                (OptionName::Model, None) => ArgumentState::Failed("--model requires a value"),
                (OptionName::Theme, None) => ArgumentState::Failed("--theme requires a value"),
            },
            ArgumentState::CollectingTail => {
                prompt_parts.extend(arguments.by_ref());
                ArgumentState::Finalizing
            }
            ArgumentState::Finalizing => {
                if prompt_parts
                    .first()
                    .is_some_and(|first| tools::parse_tool_invocation(first).is_some())
                {
                    // Separate argv entries retain their boundaries. Embedded
                    // shell arguments in the first entry keep their existing form.
                    for arg in prompt_parts.iter_mut().skip(1) {
                        *arg = tools::shell_quote(arg);
                    }
                }
                let prompt = if prompt_parts.is_empty() {
                    None
                } else {
                    Some(prompt_parts.join(" "))
                };
                let args = Args {
                    prompt,
                    model: model.clone(),
                    theme: theme.mode(),
                    auto: config.auto,
                };
                let theme_preference = match theme {
                    ThemeSelection::Saved(_) => None,
                    ThemeSelection::Override(mode) => {
                        config.theme = mode;
                        Some(config.clone())
                    }
                };
                ArgumentState::Complete(ArgumentOutcome::Run {
                    args,
                    theme_preference,
                })
            }
            ArgumentState::Complete(outcome) => return Ok(outcome),
            ArgumentState::Failed(error) => return Err(error.into()),
        };
    }
}

pub(crate) fn print_help() {
    println!(
        "ask - MacOS command assistant

Usage:
  ask [--model MODEL] [--theme light|dark] <prompt>   # Single prompt mode
  ask [--model MODEL] [--theme light|dark]             # Interactive mode
  command | ask \"prompt\"                                # Pipe mode
  command | ask                                         # Pipe mode (auto-summarize)

Modes:
  Single prompt:    Provide a prompt and get commands to execute
  Interactive:      Enter multiple prompts in a session (type 'exit' or 'quit' to end)
  Pipe:             Pipe data from any command for AI analysis and transformation

Options:
  --model MODEL     Override the default LLM model ({DEFAULT_MODEL})
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show this help message

Environment:
  OPENROUTER_ASK_API_KEY must be set with your OpenRouter API key.

Config:
  Preferences are stored in ~/.ask/config
  (theme=light|dark, model=MODEL, auto=on|off).

The tool sends your prompt to OpenRouter, previews the generated commands,
and asks for confirmation before executing each one in your shell.

Model selection:
  ask model             Show the model in use and where it comes from
  ask model MODEL       Save MODEL as the default (persisted in ~/.ask/config)
  ask model reset       Return to the built-in default ({DEFAULT_MODEL})
  All three also work inside interactive mode; --model overrides for one run.

Auto mode:
  ask auto on / ask auto off (also works inside interactive mode)

  Jev independently checks each eligible generated command using the same
  OpenRouter key. Confident read-only assessments run without the [Y/n] prompt.
  Writes, scripts, complex shell syntax, piped-data sessions, uncertain results,
  and unavailable safety checks still ask. The setting is remembered between
  sessions and defaults to OFF.

Tool library:
  ask tool new NAME WHAT IT DOES    Have the LLM write a reusable bash/python
                                    script; you review the source, then approve
  ask 'tool improve NAME ...'       Update an existing tool (review + approve)
  ask tool list                     List saved tools
  ask tool show NAME                Print a tool's source and approval status
  ask tool approve NAME             Re-approve after reviewing a changed script
  ask tool rm NAME                  Delete a tool
  ask '$NAME args'                  Run an approved tool (quote the sigil so
                                    your shell doesn't expand $NAME first)

  Tools live in ~/.ask/tools/NAME/. Each approval stores a checksum of the
  script; if the file changes, ask refuses to run it until re-approved.
  All of these also work inside interactive mode (no quoting needed there).

Pipe mode examples:
  git diff | ask \"write a commit message\"
  cat error.log | ask \"what went wrong?\"
  ps aux | ask \"what's using the most memory?\"
  curl -s api.example.com | ask \"extract all emails\"
  docker logs app | ask \"summarize errors\"
  cat data.csv | ask                                   # auto-summarizes

Command confirmation options:
  Y/yes (or Enter)  Execute the command
  n/no              Cancel execution and exit (in interactive mode, returns to prompt)
  s/skip            Skip this command and continue to the next
                    (offered only when the response has multiple commands)

Interactive mode commands:
  exit / quit       Exit interactive mode
  clear             Clear screen and reset conversation context
  finder            Open Finder window at current directory
  auto on|off       Toggle auto-execution of Jev-assessed read-only commands
  auto              Show whether auto mode is on
  model [MODEL]     Show or change the saved LLM model (also: model reset)
  tool ...          Manage the saved tool library (new, improve, list, show,
                    approve, rm)
  $NAME args        Run an approved saved tool"
    );
}

#[cfg(test)]
mod tests;
