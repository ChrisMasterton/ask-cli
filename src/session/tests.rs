use super::*;
use crate::ThemeMode;
use crate::execution::CommandStatus;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

struct FakeInput {
    lines: VecDeque<Result<String, ReadlineError>>,
    remembered: Rc<RefCell<Vec<String>>>,
}

impl SessionInput for FakeInput {
    fn readline(&mut self, _: &str) -> Result<String, ReadlineError> {
        self.lines.pop_front().unwrap_or(Err(ReadlineError::Eof))
    }

    fn remember(&mut self, input: &str) {
        self.remembered.borrow_mut().push(input.into());
    }
}

fn input(lines: &[&str], remembered: Rc<RefCell<Vec<String>>>) -> FakeInput {
    FakeInput {
        lines: lines.iter().map(|line| Ok((*line).into())).collect(),
        remembered,
    }
}

fn answer() -> ExecutionReport {
    ExecutionReport {
        outcomes: vec![CommandOutcome {
            command: "# answer".into(),
            status: CommandStatus::Comment,
            output: String::new(),
        }],
    }
}

#[test]
fn interrupted_and_blank_input_never_reach_input_history_or_action_dispatch() {
    let remembered = Rc::new(RefCell::new(Vec::new()));
    let mut editor = input(&[" \t ", " question "], remembered.clone());
    editor.lines.push_front(Err(ReadlineError::Interrupted));
    let mut requests = Vec::new();
    let theme = Theme::from_mode(ThemeMode::Dark);
    let session = run_session_with(
        "vendor/model",
        false,
        &theme,
        || Ok(editor),
        |action, settings, history| {
            handle_action(
                action,
                settings,
                history,
                &theme,
                |prompt, _, _, history| {
                    assert!(history.is_empty());
                    requests.push(prompt.to_string());
                    Ok(answer())
                },
            )
        },
    )
    .unwrap();
    assert_eq!(*remembered.borrow(), ["question"]);
    assert_eq!(requests, ["question"]);
    assert_eq!(session.history.as_slice().len(), 1);
    assert_eq!(session.history.as_slice()[0].prompt, "question");
}

#[test]
fn prompt_errors_recover_without_recording_failed_requests_or_losing_prior_outcomes() {
    let remembered = Rc::new(RefCell::new(Vec::new()));
    let editor = input(
        &["first request", "failed request", "follow up"],
        remembered.clone(),
    );
    let mut observed = Vec::new();
    let theme = Theme::from_mode(ThemeMode::Dark);
    let session = run_session_with(
        "vendor/model",
        false,
        &theme,
        || Ok(editor),
        |action, settings, history| {
            handle_action(
                action,
                settings,
                history,
                &theme,
                |prompt, model, auto, history| {
                    assert_eq!(model, "vendor/model");
                    assert!(!auto);
                    observed.push((
                        prompt.to_string(),
                        history
                            .iter()
                            .map(|turn| turn.prompt.clone())
                            .collect::<Vec<_>>(),
                    ));
                    match prompt {
                        "first request" => Ok(ExecutionReport {
                            outcomes: vec![
                                CommandOutcome::succeeded("first", "done".into()),
                                CommandOutcome {
                                    command: "second".into(),
                                    status: CommandStatus::Failed,
                                    output: "partial effects".into(),
                                },
                                CommandOutcome {
                                    command: "third".into(),
                                    status: CommandStatus::NotRun,
                                    output: "earlier command failed".into(),
                                },
                            ],
                        }),
                        "failed request" => Err("synthetic provider error".into()),
                        "follow up" => {
                            assert_eq!(
                                history[0]
                                    .outcomes
                                    .iter()
                                    .map(|outcome| outcome.status)
                                    .collect::<Vec<_>>(),
                                [
                                    CommandStatus::Succeeded,
                                    CommandStatus::Failed,
                                    CommandStatus::NotRun
                                ]
                            );
                            Ok(answer())
                        }
                        _ => panic!("unexpected request"),
                    }
                },
            )
        },
    )
    .unwrap();
    assert_eq!(
        *remembered.borrow(),
        ["first request", "failed request", "follow up"]
    );
    assert_eq!(
        observed,
        [
            ("first request".into(), vec![]),
            ("failed request".into(), vec!["first request".into()]),
            ("follow up".into(), vec!["first request".into()]),
        ]
    );
    assert_eq!(session.history.as_slice().len(), 2);
    assert_eq!(session.history.as_slice()[1].prompt, "follow up");
}

#[test]
fn clear_resets_conversation_without_clearing_input_history_or_settings() {
    let remembered = Rc::new(RefCell::new(Vec::new()));
    let editor = input(&["first request", "clear", "follow up"], remembered.clone());
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut requests = 0;
    let session = run_session_with(
        "vendor/current",
        true,
        &theme,
        || Ok(editor),
        |action, settings, history| {
            // The real CLI comparison exercises the utility; here the reset effect
            // is supplied without starting an external clear process.
            if matches!(action, SessionAction::Clear) {
                assert_eq!(history.as_slice().len(), 1);
                return Ok(SessionEffect::Clear);
            }
            handle_action(
                action,
                settings,
                history,
                &theme,
                |_, model, auto, history| {
                    requests += 1;
                    assert_eq!(model, "vendor/current");
                    assert!(auto);
                    assert!(history.is_empty());
                    Ok(answer())
                },
            )
        },
    )
    .unwrap();
    assert_eq!(requests, 2);
    assert_eq!(
        *remembered.borrow(),
        ["first request", "clear", "follow up"]
    );
    assert_eq!(session.settings.model, "vendor/current");
    assert!(session.settings.auto);
    assert_eq!(session.history.as_slice().len(), 1);
    assert_eq!(session.history.as_slice()[0].prompt, "follow up");
}

#[test]
fn editor_and_fatal_action_errors_stop_before_later_input_is_processed() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let result = run_session_with(
        "vendor/model",
        false,
        &theme,
        || -> SessionResult<FakeInput> { Err("editor unavailable".into()) },
        |_, _, _| panic!("failed editor initialization must not dispatch"),
    );
    assert_eq!(result.err().unwrap().to_string(), "editor unavailable");

    let remembered = Rc::new(RefCell::new(Vec::new()));
    let editor = input(&["clear", "unread request"], remembered.clone());
    let result = run_session_with(
        "vendor/model",
        false,
        &theme,
        || Ok(editor),
        |action, _, _| {
            assert!(matches!(action, SessionAction::Clear));
            Err("clear failed".into())
        },
    );
    assert_eq!(result.err().unwrap().to_string(), "clear failed");
    assert_eq!(*remembered.borrow(), ["clear"]);

    let mut editor = input(&["unread request"], remembered.clone());
    editor
        .lines
        .push_front(Err(ReadlineError::Io(io::Error::other("reader stopped"))));
    let result = run_session_with(
        "vendor/model",
        false,
        &theme,
        || Ok(editor),
        |_, _, _| panic!("failed input must not dispatch"),
    );
    assert_eq!(result.err().unwrap().to_string(), "reader stopped");
    assert_eq!(*remembered.borrow(), ["clear"]);
}

#[test]
fn routing_preserves_shortcuts_tool_precedence_and_direct_command_rewrites() {
    assert!(matches!(route_input("q".into()), SessionAction::Quit));
    assert!(matches!(
        route_input(".".into()),
        SessionAction::CurrentDirectory
    ));
    assert!(matches!(
        route_input("..".into()),
        SessionAction::ParentDirectory
    ));
    assert!(matches!(route_input("clear".into()), SessionAction::Clear));
    assert!(matches!(
        route_input("auto".into()),
        SessionAction::ShowAuto
    ));
    assert!(matches!(
        route_input("AUTO ON".into()),
        SessionAction::ToggleAuto(true)
    ));
    assert!(matches!(
        route_input("model reset".into()),
        SessionAction::Model(ModelCommand::Reset)
    ));
    assert!(matches!(
        route_input("tool list".into()),
        SessionAction::ManageTool(tools::ToolCommand::List)
    ));
    assert!(
        matches!(route_input("$backup docs".into()), SessionAction::SavedTool { name, args_tail, .. } if name == "backup" && args_tail == "docs")
    );
    assert!(
        matches!(route_input("ls".into()), SessionAction::Direct { input, command } if input == "ls" && command == "ls -l")
    );
    assert!(
        matches!(route_input("deploy.sh".into()), SessionAction::Direct { command, .. } if command == "bash deploy.sh")
    );
    for prompt in [
        "AUTO",
        "CLEAR",
        "model reset and then list files",
        "tool show me the weather",
        "$HOME/x",
    ] {
        assert!(
            matches!(route_input(prompt.into()), SessionAction::Prompt(input) if input == prompt)
        );
    }
}
