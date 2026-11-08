//! Conversation updates and prompt-only compaction have separate owners.
//! Rendering a limited context never prunes the retained session history.

use crate::execution::{CommandOutcome, CommandStatus};
use crate::terminal_io::output_tail;
use std::iter::Rev;
use std::slice::Iter;

pub(crate) const MAX_CONTEXT_TOKENS: usize = 3000;
const TOKEN_ESTIMATE_RATIO: usize = 4;
pub(crate) const MAX_HISTORY_OUTPUT_BYTES: usize = 4000;
const CONTEXT_HEADER: &str = "Previous commands and outputs in this session:\n\n";

#[derive(Clone)]
pub(crate) struct ConversationContext {
    pub(crate) prompt: String,
    pub(crate) outcomes: Vec<CommandOutcome>,
}

impl ConversationContext {
    pub(crate) fn render(&self) -> String {
        let mut context = format!("User: {}\n", self.prompt);
        // Share this turn's allowance across all nonempty outputs.
        let output_count = self
            .outcomes
            .iter()
            .filter(|o| !o.output.is_empty())
            .count()
            .max(1);
        let output_budget = MAX_HISTORY_OUTPUT_BYTES / output_count;
        for outcome in &self.outcomes {
            if outcome.status == CommandStatus::Comment {
                context.push_str(&format!(
                    "Assistant: {}\n",
                    outcome.command.trim_start_matches('#').trim()
                ));
                continue;
            }
            let status = match outcome.status {
                CommandStatus::Succeeded => "succeeded (exit status 0)",
                CommandStatus::Failed => "failed (may have partial effects; not rolled back)",
                CommandStatus::Skipped => "skipped by user; not executed",
                CommandStatus::Cancelled => "cancelled by user; not executed",
                CommandStatus::NotRun => "not executed",
                CommandStatus::ConfirmationFailed => "confirmation failed; not executed",
                CommandStatus::Comment => unreachable!(),
            };
            context.push_str(&format!("Command: {}\nStatus: {status}\n", outcome.command));
            if !outcome.output.is_empty() {
                context.push_str(&format!(
                    "Output: {}\n",
                    output_tail(&outcome.output, output_budget)
                ));
            }
        }
        context.push('\n');
        context
    }
}

#[derive(Clone, Copy)]
pub(crate) enum NoticePolicy {
    Silent,
    CheckBudget,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HistoryNotice {
    None,
    ContextLimitExceeded,
}

enum HistoryState {
    Recording {
        context: ConversationContext,
        notice: NoticePolicy,
    },
    CheckingBudget,
    Clearing,
    Complete(HistoryNotice),
}

#[derive(Default)]
pub(crate) struct ConversationHistory {
    turns: Vec<ConversationContext>,
}

impl ConversationHistory {
    pub(crate) fn as_slice(&self) -> &[ConversationContext] {
        &self.turns
    }

    pub(crate) fn record(
        &mut self,
        context: ConversationContext,
        notice: NoticePolicy,
    ) -> HistoryNotice {
        self.update(HistoryState::Recording { context, notice })
    }

    pub(crate) fn clear(&mut self) {
        self.update(HistoryState::Clearing);
    }

    fn update(&mut self, mut state: HistoryState) -> HistoryNotice {
        loop {
            state = match state {
                HistoryState::Recording { context, notice } => {
                    self.turns.push(context);
                    match notice {
                        NoticePolicy::Silent => HistoryState::Complete(HistoryNotice::None),
                        NoticePolicy::CheckBudget => HistoryState::CheckingBudget,
                    }
                }
                HistoryState::CheckingBudget => HistoryState::Complete(
                    if estimated_history_tokens(&self.turns) > MAX_CONTEXT_TOKENS {
                        HistoryNotice::ContextLimitExceeded
                    } else {
                        HistoryNotice::None
                    },
                ),
                HistoryState::Clearing => {
                    self.turns.clear();
                    HistoryState::Complete(HistoryNotice::None)
                }
                HistoryState::Complete(notice) => return notice,
            };
        }
    }
}

pub(crate) fn estimate_tokens(text: &str) -> usize {
    text.len() / TOKEN_ESTIMATE_RATIO
}

pub(crate) fn estimated_history_tokens(history: &[ConversationContext]) -> usize {
    history
        .iter()
        .map(|context| estimate_tokens(&context.render()))
        .sum()
}

enum ContextState<'a> {
    Selecting {
        remaining: Rev<Iter<'a, ConversationContext>>,
        selected: Vec<String>,
        tokens: usize,
    },
    Rendering(Vec<String>),
    Appending {
        context: String,
        remaining: std::vec::IntoIter<String>,
    },
    Complete(String),
}

pub(crate) fn render_context(history: &[ConversationContext]) -> String {
    let mut state = ContextState::Selecting {
        remaining: history.iter().rev(),
        selected: Vec::new(),
        tokens: estimate_tokens(CONTEXT_HEADER),
    };
    loop {
        state = match state {
            ContextState::Selecting {
                mut remaining,
                mut selected,
                tokens,
            } => match remaining.next() {
                Some(turn) => {
                    let rendered = turn.render();
                    let cost = estimate_tokens(&rendered);
                    if tokens + cost > MAX_CONTEXT_TOKENS {
                        // The first overflow stops selection, even if older turns fit.
                        ContextState::Rendering(selected)
                    } else {
                        selected.push(rendered);
                        ContextState::Selecting {
                            remaining,
                            selected,
                            tokens: tokens + cost,
                        }
                    }
                }
                None => ContextState::Rendering(selected),
            },
            ContextState::Rendering(mut selected) => {
                selected.reverse();
                let mut context = CONTEXT_HEADER.to_string();
                if selected.len() < history.len() {
                    // Preserve the existing note, whose overhead is outside the selection budget.
                    context.push_str(&format!(
                        "(Note: Showing recent {} of {} total interactions due to length)\n\n",
                        selected.len(),
                        history.len()
                    ));
                }
                ContextState::Appending {
                    context,
                    remaining: selected.into_iter(),
                }
            }
            ContextState::Appending {
                mut context,
                mut remaining,
            } => match remaining.next() {
                Some(turn) => {
                    context.push_str(&turn);
                    ContextState::Appending { context, remaining }
                }
                None => ContextState::Complete(context),
            },
            ContextState::Complete(context) => return context,
        };
    }
}

#[cfg(test)]
mod tests;
