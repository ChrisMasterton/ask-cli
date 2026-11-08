//! Generated commands advance through assessment, confirmation, execution, and
//! recording. A stopped batch drains its remaining lines without executing them.

use crate::Theme;
use crate::confirmation::ConfirmationDecision;
use crate::safety;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    CommandFailed,
    Cancelled,
    ConfirmationFailed,
}

impl StopReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::CommandFailed => "earlier command failed",
            Self::Cancelled => "execution cancelled",
            Self::ConfirmationFailed => "confirmation failed",
        }
    }
}

enum BatchState {
    Active,
    Stopped(StopReason),
}

enum ExecutionState {
    Next(BatchState),
    Assessing(String),
    AwaitingConfirmation(String),
    Running(String),
    Complete,
}

// Next(Active) -> Assessing -> AwaitingConfirmation / Running -> Next.
// Failure or cancellation returns to Next(Stopped); comments still display.
pub(crate) fn execute_commands_with<A, C, E>(
    commands: Vec<String>,
    theme: &Theme,
    mut auto_execute: A,
    mut confirm_command: C,
    mut execute_command: E,
) -> ExecutionReport
where
    A: FnMut(&str) -> bool,
    C: FnMut(&str, &Theme, bool) -> io::Result<ConfirmationDecision>,
    E: FnMut(&str) -> Result<String, Box<dyn std::error::Error>>,
{
    let offer_skip = commands.iter().filter(|c| !c.starts_with('#')).count() > 1;
    let mut commands = commands.into_iter();
    let mut report = ExecutionReport::default();
    let mut state = ExecutionState::Next(BatchState::Active);

    loop {
        state = match state {
            ExecutionState::Next(batch) => match commands.next() {
                None => ExecutionState::Complete,
                Some(command) if command.starts_with('#') => {
                    let text = command.trim_start_matches('#').trim();
                    println!("{}\n", theme.helper_text(text));
                    report.outcomes.push(CommandOutcome {
                        command,
                        status: CommandStatus::Comment,
                        output: String::new(),
                    });
                    ExecutionState::Next(batch)
                }
                Some(command) => match batch {
                    BatchState::Active => ExecutionState::Assessing(command),
                    BatchState::Stopped(reason) => {
                        let text = reason.as_str();
                        println!("Not run ({text}): {}", theme.command_text(&command));
                        report.outcomes.push(CommandOutcome {
                            command,
                            status: CommandStatus::NotRun,
                            output: text.to_string(),
                        });
                        ExecutionState::Next(BatchState::Stopped(reason))
                    }
                },
            },
            ExecutionState::Assessing(command) => {
                // Local policy is checked before consulting any model or cache.
                if safety::eligible(&command) && auto_execute(&command) {
                    println!(
                        "{} {} {}",
                        theme.prompt_text("run>"),
                        theme.command_text(&command),
                        theme.helper_text("(auto: Jev read-only)")
                    );
                    ExecutionState::Running(command)
                } else {
                    ExecutionState::AwaitingConfirmation(command)
                }
            }
            ExecutionState::AwaitingConfirmation(command) => {
                match confirm_command(&command, theme, offer_skip) {
                    Ok(ConfirmationDecision::Accept) => ExecutionState::Running(command),
                    Ok(ConfirmationDecision::Decline) => {
                        println!("Command execution cancelled");
                        report.outcomes.push(CommandOutcome {
                            command,
                            status: CommandStatus::Cancelled,
                            output: String::new(),
                        });
                        ExecutionState::Next(BatchState::Stopped(StopReason::Cancelled))
                    }
                    Ok(ConfirmationDecision::Skip) => {
                        println!("Skipping command: {}", theme.command_text(&command));
                        report.outcomes.push(CommandOutcome {
                            command,
                            status: CommandStatus::Skipped,
                            output: String::new(),
                        });
                        ExecutionState::Next(BatchState::Active)
                    }
                    Err(error) => {
                        eprintln!("Confirmation failed: {error}");
                        report.outcomes.push(CommandOutcome {
                            command,
                            status: CommandStatus::ConfirmationFailed,
                            output: error.to_string(),
                        });
                        ExecutionState::Next(BatchState::Stopped(StopReason::ConfirmationFailed))
                    }
                }
            }
            ExecutionState::Running(command) => {
                let outcome = CommandOutcome::from_result(&command, execute_command(&command));
                let batch = if outcome.status == CommandStatus::Failed {
                    outcome.print_failure();
                    BatchState::Stopped(StopReason::CommandFailed)
                } else {
                    BatchState::Active
                };
                report.outcomes.push(outcome);
                ExecutionState::Next(batch)
            }
            ExecutionState::Complete => return report,
        };
    }
}

/// An assessment outage disables automatic execution until the next AI request.
/// An uncertain assessment only requires confirmation for that command.
pub(crate) enum AutoExecution {
    Disabled,
    Available,
    Unavailable,
}

impl AutoExecution {
    pub(crate) fn new(enabled: bool, has_piped_data: bool) -> Self {
        if enabled && !has_piped_data {
            Self::Available
        } else {
            Self::Disabled
        }
    }

    pub(crate) fn permits_with<F>(&mut self, command: &str, assess: F) -> bool
    where
        F: FnOnce(&str) -> Result<bool, &'static str>,
    {
        match self {
            Self::Disabled | Self::Unavailable => false,
            Self::Available => match assess(command) {
                Ok(allowed) => allowed,
                Err(reason) => {
                    eprintln!("{reason}; asking for confirmation for the remaining commands.");
                    *self = Self::Unavailable;
                    false
                }
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStatus {
    Comment,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    NotRun,
    ConfirmationFailed,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandOutcome {
    pub(crate) command: String,
    pub(crate) status: CommandStatus,
    pub(crate) output: String,
}

impl CommandOutcome {
    pub(crate) fn succeeded(command: &str, output: String) -> Self {
        Self {
            command: command.to_string(),
            status: CommandStatus::Succeeded,
            output,
        }
    }

    pub(crate) fn from_result(
        command: &str,
        result: Result<String, Box<dyn std::error::Error>>,
    ) -> Self {
        match result {
            Ok(output) => Self::succeeded(command, output),
            Err(error) => Self {
                command: command.to_string(),
                status: CommandStatus::Failed,
                output: error.to_string(),
            },
        }
    }

    pub(crate) fn print_failure(&self) {
        if self.status == CommandStatus::Failed {
            eprintln!("Command failed: {}", self.output);
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionReport {
    pub(crate) outcomes: Vec<CommandOutcome>,
}

impl ExecutionReport {
    pub(crate) fn error(&self) -> Option<&str> {
        self.outcomes
            .iter()
            .find(|o| {
                matches!(
                    o.status,
                    CommandStatus::Failed | CommandStatus::ConfirmationFailed
                )
            })
            .map(|o| o.output.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThemeMode;

    #[test]
    fn disabled_auto_and_piped_requests_never_assess_commands() {
        for (enabled, piped) in [(false, false), (false, true), (true, true)] {
            let mut auto = AutoExecution::new(enabled, piped);
            assert!(!auto.permits_with("ls", |_| panic!("must use manual confirmation")));
        }
    }

    #[test]
    fn safety_outage_falls_back_for_the_batch_and_retries_on_the_next_request() {
        let mut auto = AutoExecution::new(true, false);
        let mut assessments = Vec::new();
        let mut confirmations = Vec::new();
        let mut executed = Vec::new();
        let report = execute_commands_with(
            vec!["ls".into(), "du".into(), "pwd".into()],
            &Theme::from_mode(ThemeMode::Dark),
            |command| {
                auto.permits_with(command, |command| {
                    assessments.push(command.to_string());
                    if command == "ls" {
                        Ok(true)
                    } else {
                        Err("safety service unavailable")
                    }
                })
            },
            |command, _, _| {
                confirmations.push(command.to_string());
                Ok(ConfirmationDecision::Accept)
            },
            |command| {
                executed.push(command.to_string());
                Ok(String::new())
            },
        );

        assert_eq!(assessments, ["ls", "du"]);
        assert_eq!(confirmations, ["du", "pwd"]);
        assert_eq!(executed, ["ls", "du", "pwd"]);
        assert!(
            report
                .outcomes
                .iter()
                .all(|o| o.status == CommandStatus::Succeeded)
        );

        let mut next_request = AutoExecution::new(true, false);
        let mut retried = false;
        assert!(next_request.permits_with("pwd", |_| {
            retried = true;
            Ok(true)
        }));
        assert!(retried);
    }

    #[test]
    fn uncertain_command_does_not_disable_later_assessments() {
        let mut auto = AutoExecution::new(true, false);
        assert!(!auto.permits_with("du", |_| Ok(false)));
        assert!(auto.permits_with("pwd", |_| Ok(true)));
    }
}
