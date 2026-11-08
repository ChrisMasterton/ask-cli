//! Prompt requests finish with proposals, an execution report, or an error.
//! One-shot mode interprets report failures before observing directory changes;
//! interactive mode receives the full report for its history owner to record.

mod request;

pub(crate) use request::query_api;

use crate::confirmation::confirm_command;
use crate::conversation::ConversationContext;
use crate::execution::{AutoExecution, ExecutionReport, execute_commands_with};
use crate::theme::Theme;
use crate::working_directory::{DirectoryChange, DirectorySnapshot};
use crate::{run_line, safety};

type PromptResult<T> = Result<T, Box<dyn std::error::Error>>;

enum PromptState<Q, E> {
    Querying { query: Q, execute: E },
    Executing { commands: Vec<String>, execute: E },
    Complete(ExecutionReport),
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn process_prompt_with_context(
    prompt: &str,
    model: &str,
    api_key: &str,
    theme: &Theme,
    history: &[ConversationContext],
    piped_data: Option<&str>,
    auto_mode: bool,
) -> PromptResult<ExecutionReport> {
    process_with(
        || query_api(prompt, model, api_key, history, piped_data),
        |commands| {
            let mut auto = AutoExecution::new(auto_mode, piped_data.is_some());
            execute_commands_with(
                commands,
                theme,
                |command| {
                    auto.permits_with(command, |command| {
                        safety::assess_cached(command, api_key)
                            .map(|assessment| assessment.permits_auto())
                    })
                },
                confirm_command,
                run_line,
            )
        },
    )
}

fn process_with<Q, E>(query: Q, execute: E) -> PromptResult<ExecutionReport>
where
    Q: FnOnce() -> PromptResult<Vec<String>>,
    E: FnOnce(Vec<String>) -> ExecutionReport,
{
    let mut state = PromptState::Querying { query, execute };
    loop {
        state = match state {
            PromptState::Querying { query, execute } => match query() {
                Ok(commands) => PromptState::Executing { commands, execute },
                Err(error) => PromptState::Failed(error),
            },
            PromptState::Executing { commands, execute } => {
                PromptState::Complete(execute(commands))
            }
            PromptState::Complete(report) => return Ok(report),
            PromptState::Failed(error) => return Err(error),
        };
    }
}

enum OneShotState<P> {
    CapturingDirectory(P),
    Processing {
        directory: DirectorySnapshot,
        process: P,
    },
    CheckingReport {
        directory: DirectorySnapshot,
        report: ExecutionReport,
    },
    ObservingDirectory(DirectorySnapshot),
    Notifying(DirectoryChange),
    Complete,
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn process_prompt(
    prompt: &str,
    model: &str,
    api_key: &str,
    theme: &Theme,
    piped_data: Option<&str>,
    auto_mode: bool,
) -> PromptResult<()> {
    process_once_with(theme, || {
        process_prompt_with_context(prompt, model, api_key, theme, &[], piped_data, auto_mode)
    })
}

fn process_once_with(
    theme: &Theme,
    process: impl FnOnce() -> PromptResult<ExecutionReport>,
) -> PromptResult<()> {
    let mut state = OneShotState::CapturingDirectory(process);
    loop {
        state = match state {
            OneShotState::CapturingDirectory(process) => OneShotState::Processing {
                directory: DirectorySnapshot::capture(),
                process,
            },
            OneShotState::Processing { directory, process } => match process() {
                Ok(report) => OneShotState::CheckingReport { directory, report },
                Err(error) => OneShotState::Failed(error),
            },
            OneShotState::CheckingReport { directory, report } => match report.error() {
                Some(error) => OneShotState::Failed(error.into()),
                None => OneShotState::ObservingDirectory(directory),
            },
            OneShotState::ObservingDirectory(directory) => {
                OneShotState::Notifying(directory.finish())
            }
            OneShotState::Notifying(change) => {
                if let DirectoryChange::Changed {
                    starting,
                    final_dir,
                } = change
                {
                    println!(
                        "{}",
                        theme.helper_text(&format!(
                            "Note: the command used {}, but your calling shell remains in {}. Run ask interactively to keep directory changes between prompts.",
                            final_dir.display(), starting.display()
                        ))
                    );
                }
                OneShotState::Complete
            }
            OneShotState::Complete => return Ok(()),
            OneShotState::Failed(error) => return Err(error),
        };
    }
}

#[cfg(test)]
mod tests;
