use super::*;

#[test]
fn api_response_requires_a_choice_and_nonempty_content() {
    let no_choice = commands_from_api_response(ApiResponse { choices: vec![] });
    assert!(no_choice.is_err());

    let empty_content = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "  \n".to_string(),
            },
        }],
    });
    assert!(empty_content.is_err());
}

#[test]
fn api_response_is_parsed_into_comments_and_intact_shell_lines() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "# Set up the repo\nmkdir app && cd app && git init".to_string(),
            },
        }],
    })
    .expect("valid model response");

    assert_eq!(
        reply,
        vec!["# Set up the repo", "mkdir app && cd app && git init"]
    );
}

#[test]
fn conversational_reply_keeps_comment_lines() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "# hello there!".to_string(),
            },
        }],
    })
    .expect("valid response");
    assert_eq!(reply, vec!["# hello there!"]);
}

fn reply(contents: &[&str]) -> ApiResponse {
    serde_json::from_value(serde_json::json!({
        "choices": contents.iter().map(|content| serde_json::json!({
            "message": {"content": content}
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[test]
fn missing_and_empty_first_choices_keep_their_distinct_errors() {
    assert_eq!(
        commands_from_api_response(reply(&[]))
            .unwrap_err()
            .to_string(),
        "No command returned from the model."
    );
    assert_eq!(
        tool_content_from_api_response(reply(&[]))
            .unwrap_err()
            .to_string(),
        "The model returned an empty reply."
    );
    for first in ["", "  \n", "```bash\n```", "echo ignored```"] {
        assert_eq!(
            commands_from_api_response(reply(&[first, "echo second"]))
                .unwrap_err()
                .to_string(),
            "No response returned from the model."
        );
    }
    assert_eq!(
        tool_content_from_api_response(reply(&[" \n", "second"])).unwrap(),
        ""
    );
}

#[test]
fn tool_source_and_command_lines_keep_separate_whitespace_and_fence_policies() {
    let source = " \nLANG: python\n```python\nif True:\n    print('kept')\n```\n ";
    assert_eq!(
        tool_content_from_api_response(reply(&[source])).unwrap(),
        source.trim()
    );
    assert_eq!(
        parse_commands(source),
        ["LANG: python", "if True:", "print('kept')"]
    );
    assert_eq!(
        parse_commands("# note\r\n echo 'a && b' && pwd\r\n"),
        ["# note", "echo 'a && b' && pwd"]
    );
}
