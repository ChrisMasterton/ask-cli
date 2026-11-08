//! Command confirmation and saved-tool approval share an input state machine,
//! with separate policies for their intentionally different defaults.

use crate::Theme;
use std::io::{self, Write};

mod input;
use input::read_confirmation_line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmationDecision {
    Accept,
    Decline,
    Skip,
}

#[derive(Clone, Copy)]
enum ConfirmationPolicy {
    Command,
    ToolApproval,
}

impl ConfirmationPolicy {
    fn decide(self, input: &str) -> Option<ConfirmationDecision> {
        let input = normalize_confirmation_input(input);
        match self {
            Self::Command => match input.as_str() {
                "" | "y" | "yes" => Some(ConfirmationDecision::Accept),
                "n" | "no" => Some(ConfirmationDecision::Decline),
                // Skip remains valid even when the single-command prompt omits it.
                "s" | "skip" => Some(ConfirmationDecision::Skip),
                _ => None,
            },
            Self::ToolApproval => Some(if matches!(input.as_str(), "y" | "yes") {
                ConfirmationDecision::Accept
            } else {
                ConfirmationDecision::Decline
            }),
        }
    }
}

enum ConfirmationState {
    Prompting,
    AwaitingInput,
    InvalidInput,
    Decided(ConfirmationDecision),
    Failed(io::Error),
}

// Prompting -> AwaitingInput -> Decided / InvalidInput / Failed.
// Invalid command input returns to Prompting; tool approval never retries.
fn confirm_with<P, R, I>(
    policy: ConfirmationPolicy,
    mut prompt: P,
    mut read_input: R,
    mut invalid_input: I,
) -> io::Result<ConfirmationDecision>
where
    P: FnMut() -> io::Result<()>,
    R: FnMut() -> io::Result<String>,
    I: FnMut(),
{
    let mut state = ConfirmationState::Prompting;
    loop {
        state = match state {
            ConfirmationState::Prompting => match prompt() {
                Ok(()) => ConfirmationState::AwaitingInput,
                Err(error) => ConfirmationState::Failed(error),
            },
            ConfirmationState::AwaitingInput => match read_input() {
                Ok(input) => match policy.decide(&input) {
                    Some(decision) => ConfirmationState::Decided(decision),
                    None => ConfirmationState::InvalidInput,
                },
                Err(error) => ConfirmationState::Failed(error),
            },
            ConfirmationState::InvalidInput => {
                invalid_input();
                ConfirmationState::Prompting
            }
            ConfirmationState::Decided(decision) => return Ok(decision),
            ConfirmationState::Failed(error) => return Err(error),
        };
    }
}

pub(crate) fn confirm_command(
    command: &str,
    theme: &Theme,
    offer_skip: bool,
) -> io::Result<ConfirmationDecision> {
    let (options, invalid_hint) = if offer_skip {
        ("[Y/n/s]", "Please use Y(es), n(o), or s(kip).")
    } else {
        ("[Y/n]", "Please use Y(es) or n(o).")
    };
    confirm_with(
        ConfirmationPolicy::Command,
        || {
            print!(
                "{} {}?  {options}  ",
                theme.prompt_text("run>"),
                theme.command_text(command)
            );
            io::stdout().flush()
        },
        read_confirmation_line,
        || println!("Invalid response. {invalid_hint}"),
    )
}

pub(crate) fn confirm_tool_approval(question: &str, theme: &Theme) -> io::Result<bool> {
    confirm_with(
        ConfirmationPolicy::ToolApproval,
        || {
            print!("{} ", theme.prompt_text(question));
            io::stdout().flush()
        },
        read_confirmation_line,
        || unreachable!("tool approval declines input other than explicit yes"),
    )
    .map(|decision| decision == ConfirmationDecision::Accept)
}

fn normalize_confirmation_input(input: &str) -> String {
    let mut cleaned = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            // Strip ANSI escape sequences that can leak into terminal input.
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for seq_char in chars.by_ref() {
                    if ('@'..='~').contains(&seq_char) {
                        break;
                    }
                }
            }
            continue;
        }

        if !ch.is_control() {
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[test]
    fn command_choices_preserve_defaults_skip_and_normalization() {
        for (input, expected) in [
            ("", Some(ConfirmationDecision::Accept)),
            (" \r\n", Some(ConfirmationDecision::Accept)),
            ("YES", Some(ConfirmationDecision::Accept)),
            ("  No  ", Some(ConfirmationDecision::Decline)),
            ("s", Some(ConfirmationDecision::Skip)),
            ("skip", Some(ConfirmationDecision::Skip)),
            ("maybe", None),
            (
                "\u{1b}[?2004lyes\u{1b}[?2004h\n",
                Some(ConfirmationDecision::Accept),
            ),
        ] {
            assert_eq!(
                ConfirmationPolicy::Command.decide(input),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn invalid_command_input_reprompts_before_returning_one_decision() {
        let events = RefCell::new(Vec::new());
        let mut inputs = VecDeque::from(["maybe", "YES", "n"]);
        let decision = confirm_with(
            ConfirmationPolicy::Command,
            || {
                events.borrow_mut().push("prompt");
                Ok(())
            },
            || {
                events.borrow_mut().push("read");
                Ok(inputs.pop_front().unwrap().to_string())
            },
            || events.borrow_mut().push("invalid"),
        )
        .unwrap();

        assert_eq!(decision, ConfirmationDecision::Accept);
        assert_eq!(
            *events.borrow(),
            ["prompt", "read", "invalid", "prompt", "read"]
        );
        assert_eq!(inputs, ["n"]);
    }

    #[test]
    fn tool_approval_requires_explicit_yes_and_never_reprompts() {
        for (input, expected) in [
            ("y", ConfirmationDecision::Accept),
            (" YES ", ConfirmationDecision::Accept),
            ("\u{1b}[?2004ly\u{1b}[?2004h", ConfirmationDecision::Accept),
            ("", ConfirmationDecision::Decline),
            ("n", ConfirmationDecision::Decline),
            ("skip", ConfirmationDecision::Decline),
            ("maybe", ConfirmationDecision::Decline),
        ] {
            let mut prompts = 0;
            let mut reads = 0;
            let decision = confirm_with(
                ConfirmationPolicy::ToolApproval,
                || {
                    prompts += 1;
                    Ok(())
                },
                || {
                    reads += 1;
                    Ok(input.to_string())
                },
                || panic!("tool approval must not reprompt"),
            )
            .unwrap();
            assert_eq!(decision, expected, "{input:?}");
            assert_eq!((prompts, reads), (1, 1));
        }
    }

    #[test]
    fn closed_or_failed_input_never_becomes_a_confirmation() {
        for policy in [
            ConfirmationPolicy::Command,
            ConfirmationPolicy::ToolApproval,
        ] {
            for kind in [io::ErrorKind::UnexpectedEof, io::ErrorKind::Interrupted] {
                let error = confirm_with(
                    policy,
                    || Ok(()),
                    || Err(io::Error::new(kind, "input unavailable")),
                    || panic!("input failures must not reprompt"),
                )
                .unwrap_err();
                assert_eq!(error.kind(), kind);
            }
        }
    }

    #[test]
    fn prompt_failure_does_not_read_or_accept_input() {
        let error = confirm_with(
            ConfirmationPolicy::Command,
            || Err(io::Error::new(io::ErrorKind::BrokenPipe, "output closed")),
            || panic!("a failed prompt must not read confirmation"),
            || panic!("a failed prompt must not reprompt"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
