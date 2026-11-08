use super::*;
use crate::api::response::commands_from_api_response;
use crate::confirmation::ConfirmationDecision;
use crate::execution::{CommandStatus, execute_commands_with};
use crate::theme::ThemeMode;
use std::io;

#[test]
fn request_and_parsing_errors_never_reach_execution_or_become_reports() {
    for message in ["Network error: fixture", "API error 429: quota"] {
        let error = process_with(
            || Err(message.into()),
            |_| panic!("a failed query must not execute"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), message);
    }
    let error = process_with(
        || commands_from_api_response(serde_json::from_str(r#"{"choices":[]}"#).unwrap()),
        |_| panic!("a missing proposal must not execute"),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "No command returned from the model.");
}

#[test]
fn interactive_reports_and_one_shot_errors_preserve_execution_outcomes() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    for (scenario, expected, expected_error) in [
        ("success", CommandStatus::Succeeded, None),
        ("cancel", CommandStatus::Cancelled, None),
        (
            "fail",
            CommandStatus::Failed,
            Some("fixture execution error"),
        ),
        (
            "confirmation",
            CommandStatus::ConfirmationFailed,
            Some("fixture input error"),
        ),
    ] {
        let mut executed = Vec::new();
        let report = process_with(
            || commands_from_api_response(serde_json::from_str(r##"{"choices":[{"message":{"content":"# before\necho fixture\necho later\n# after"}}]}"##).unwrap()),
            |commands| execute_commands_with(
                commands,
                &theme,
                |_| false,
                |_, _, _| match scenario {
                    "cancel" => Ok(ConfirmationDecision::Decline),
                    "confirmation" => Err(io::Error::other("fixture input error")),
                    _ => Ok(ConfirmationDecision::Accept),
                },
                |command| {
                    executed.push(command.to_string());
                    if scenario == "fail" { Err("fixture execution error".into()) } else { Ok("fixture output".to_string()) }
                },
            ),
        ).expect("completed execution returns a report even if a command fails");
        assert_eq!(report.outcomes[1].status, expected);
        assert_eq!(report.outcomes[0].status, CommandStatus::Comment);
        assert_eq!(report.outcomes[3].status, CommandStatus::Comment);
        assert_eq!(report.error(), expected_error);
        if scenario == "success" {
            assert_eq!(executed, ["echo fixture", "echo later"]);
        } else {
            assert_eq!(report.outcomes[2].status, CommandStatus::NotRun);
            assert_eq!(executed.len(), usize::from(scenario == "fail"));
        }
        let result = process_once_with(&theme, || Ok(report));
        assert_eq!(
            result.err().map(|error| error.to_string()).as_deref(),
            expected_error
        );
    }
}

#[test]
fn one_shot_request_errors_are_returned_unchanged() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let result = process_once_with(
        &theme,
        || Err("No response returned from the model.".into()),
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "No response returned from the model."
    );
}
