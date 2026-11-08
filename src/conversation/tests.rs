use super::*;

fn turn(prompt: &str) -> ConversationContext {
    ConversationContext {
        prompt: prompt.into(),
        outcomes: vec![],
    }
}

#[test]
fn context_rendering_matches_pre_refactor_fixtures() {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/conversation-context.json"
    ))
    .unwrap();
    let mixed = vec![ConversationContext {
        prompt: "inspect café".into(),
        outcomes: vec![
            CommandOutcome {
                command: "# details".into(),
                status: CommandStatus::Comment,
                output: String::new(),
            },
            CommandOutcome::succeeded("ls", "one\ntwo\n".into()),
            CommandOutcome {
                command: "false".into(),
                status: CommandStatus::Failed,
                output: "error\n".into(),
            },
            CommandOutcome {
                command: "touch skip".into(),
                status: CommandStatus::Skipped,
                output: String::new(),
            },
            CommandOutcome {
                command: "touch cancel".into(),
                status: CommandStatus::Cancelled,
                output: String::new(),
            },
            CommandOutcome {
                command: "later".into(),
                status: CommandStatus::NotRun,
                output: "execution cancelled".into(),
            },
            CommandOutcome {
                command: "confirm".into(),
                status: CommandStatus::ConfirmationFailed,
                output: "tty closed".into(),
            },
        ],
    }];
    let huge = || turn(&"x".repeat(12000));
    let unicode = vec![ConversationContext {
        prompt: "logs".into(),
        outcomes: vec![CommandOutcome::succeeded(
            "cat log",
            "start".to_string() + &"x".repeat(4100) + "🚀 final diagnostic",
        )],
    }];
    for (name, history) in [
        ("empty", vec![]),
        ("mixed", mixed),
        ("oversized_newest", vec![turn("short"), huge()]),
        ("overflow_older", vec![turn("short"), huge(), turn("short")]),
        ("unicode_output", unicode),
    ] {
        assert_eq!(
            render_context(&history),
            expected[name].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn context_rendering_retains_omitted_turns_until_explicit_clear() {
    let mut history = ConversationHistory::default();
    history.record(turn("older"), NoticePolicy::Silent);
    assert_eq!(
        history.record(turn(&"x".repeat(12000)), NoticePolicy::Silent),
        HistoryNotice::None
    );
    assert!(render_context(history.as_slice()).contains("Showing recent 0 of 2"));
    assert_eq!(history.as_slice().len(), 2);
    assert_eq!(
        history.record(turn("newest"), NoticePolicy::CheckBudget),
        HistoryNotice::ContextLimitExceeded
    );
    assert!(render_context(history.as_slice()).contains("Showing recent 1 of 3"));
    assert_eq!(history.as_slice().len(), 3);
    assert_eq!(history.as_slice()[0].prompt, "older");

    history.clear();
    assert!(history.as_slice().is_empty());
    assert_eq!(
        history.record(turn("fresh"), NoticePolicy::CheckBudget),
        HistoryNotice::None
    );
    assert_eq!(history.as_slice()[0].prompt, "fresh");
}

#[test]
fn warning_threshold_and_context_selection_keep_their_distinct_budgets() {
    let mut history = ConversationHistory::default();
    // A rendered turn includes "User: ", a newline, and a final blank line.
    let prompt = "x".repeat(MAX_CONTEXT_TOKENS * 4 - 8);
    assert_eq!(
        history.record(turn(&prompt), NoticePolicy::CheckBudget),
        HistoryNotice::None
    );
    assert_eq!(
        estimated_history_tokens(history.as_slice()),
        MAX_CONTEXT_TOKENS
    );
    // The context header consumes selection budget, but is not in the warning's estimate.
    assert!(render_context(history.as_slice()).contains("Showing recent 0 of 1"));
    assert_eq!(
        history.record(turn("next"), NoticePolicy::CheckBudget),
        HistoryNotice::ContextLimitExceeded
    );
    assert_eq!(
        history.record(turn("again"), NoticePolicy::CheckBudget),
        HistoryNotice::ContextLimitExceeded
    );
}

#[test]
fn output_allowance_is_shared_across_the_turn_and_keeps_final_diagnostics() {
    let context = ConversationContext {
        prompt: "two logs".into(),
        outcomes: vec![
            CommandOutcome::succeeded(
                "first",
                "old first ".to_string() + &"x".repeat(2500) + "first end",
            ),
            CommandOutcome::succeeded(
                "second",
                "old second ".to_string() + &"y".repeat(2500) + "second end",
            ),
        ],
    }
    .render();
    assert!(!context.contains("old first"));
    assert!(!context.contains("old second"));
    assert!(context.contains("first end"));
    assert!(context.contains("second end"));
    assert_eq!(context.matches("earlier output truncated").count(), 2);
}
