use super::*;
use crate::execution::{CommandOutcome, CommandStatus};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[test]
fn normal_prompt_requests_state_dependent_commands_as_one_chain() {
    let prompt = build_prompt("create and enter a directory", None, "");
    assert!(
        prompt.contains("Keep state-dependent steps such as `cd` or `export` in one `&&` chain")
    );
    assert!(prompt.contains("**User request:** create and enter a directory"));
}

#[test]
fn normal_prompt_embeds_the_tool_catalog_and_leaves_no_placeholder() {
    let catalog = "**User tool library**\n- $backup — back up things\n";
    let prompt = build_prompt("list files", None, catalog);
    assert!(prompt.contains("- $backup — back up things"));
    assert!(!prompt.contains("{tool_catalog}"));

    let without = build_prompt("list files", None, "");
    assert!(!without.contains("{tool_catalog}"));
    assert!(!without.contains("tool library"));
}

#[test]
fn piped_prompt_never_contains_the_tool_catalog() {
    let catalog = "**User tool library**\n- $backup — back up things\n";
    let prompt = build_prompt("summarize", Some("some data"), catalog);
    assert!(!prompt.contains("$backup"));
    assert!(!prompt.contains("{tool_catalog}"));
}

#[test]
fn piped_prompt_includes_request_and_unicode_data_without_panicking() {
    let data = "🚀".repeat((MAX_PIPE_BYTES / 4) + 1);
    let prompt = build_prompt("summarize", Some(&data), "");
    assert!(prompt.contains("**User request:** summarize"));
    assert!(prompt.contains("truncated"));
    assert!(prompt.contains("---BEGIN PIPED DATA---"));
}

#[test]
fn piped_prompt_preserves_literal_placeholders_in_user_content() {
    let request = "Explain {piped_data}, {tool_catalog}, and {query}";
    let data = r#"{"query":"{query}","template":"{piped_data}"}"#;
    let piped_prompt = build_prompt(request, Some(data), "");
    assert!(
        piped_prompt.contains(&format!(
            "---BEGIN PIPED DATA---\n{data}\n---END PIPED DATA---"
        )),
        "piped data must reach the model unchanged: {piped_prompt}"
    );
    assert!(piped_prompt.ends_with(&format!("**User request:** {request}\n")));
}

#[test]
fn normal_prompt_preserves_literal_placeholders_in_user_content() {
    let request = "Explain {piped_data}, {tool_catalog}, and {query}";
    let catalog = "- $render — Fill in the {query} placeholder";
    let normal_prompt = build_prompt(request, None, catalog);
    assert!(
        normal_prompt.contains(catalog),
        "tool descriptions must stay intact"
    );
    assert!(normal_prompt.ends_with(&format!("**User request:** {request}\n")));
}

#[derive(Deserialize)]
struct Fixture {
    name: String,
    prompt: String,
    model: String,
    catalog: String,
    piped_data: Option<String>,
    piped_repeat: Option<RepeatedData>,
    #[serde(default)]
    history: Vec<FixtureTurn>,
    sha256: String,
}

#[derive(Deserialize)]
struct RepeatedData {
    text: String,
    count: usize,
}

#[derive(Deserialize)]
struct FixtureTurn {
    prompt: String,
    comment: String,
}

#[test]
fn request_payloads_match_pre_refactor_cli_and_interactive_fixtures() {
    // Digests were captured at the provider boundary of the old CLI, including
    // an actual interactive follow-up. No credentials or host paths are stored.
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("../../../tests/fixtures/prompt-requests.json")).unwrap();
    for fixture in fixtures {
        let piped_data = fixture
            .piped_repeat
            .map(|data| data.text.repeat(data.count))
            .or(fixture.piped_data);
        let history: Vec<_> = fixture
            .history
            .into_iter()
            .map(|turn| ConversationContext {
                prompt: turn.prompt,
                outcomes: vec![CommandOutcome {
                    command: turn.comment,
                    status: CommandStatus::Comment,
                    output: String::new(),
                }],
            })
            .collect();
        let mut catalog_loads = 0;
        let mut sends = 0;
        let commands = query_api_with(
            &fixture.prompt,
            &history,
            piped_data.as_deref(),
            || {
                catalog_loads += 1;
                fixture.catalog.clone()
            },
            |messages| {
                sends += 1;
                let body = json!({"model":fixture.model,"messages":messages});
                assert_eq!(
                    format!("{:x}", Sha256::digest(serde_json::to_vec(&body).unwrap())),
                    fixture.sha256,
                    "{}",
                    fixture.name
                );
                Ok(serde_json::from_value(
                    json!({"choices":[{"message":{"content":"# fixture answer"}}]}),
                )
                .unwrap())
            },
        )
        .unwrap();
        assert_eq!(commands, ["# fixture answer"]);
        assert_eq!(sends, 1);
        assert_eq!(catalog_loads, usize::from(piped_data.is_none()));
    }
}

#[test]
fn each_query_refreshes_its_catalog_without_retrying_a_failed_request() {
    let mut revision = 0;
    let mut sends = 0;
    for should_fail in [true, false] {
        let result = query_api_with(
            "request",
            &[],
            None,
            || {
                revision += 1;
                format!("catalog revision {revision}")
            },
            |messages| {
                sends += 1;
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0]["role"], "user");
                assert!(
                    messages[0]["content"]
                        .as_str()
                        .unwrap()
                        .contains(&format!("catalog revision {sends}"))
                );
                if should_fail {
                    Err("Network error: fixture".into())
                } else {
                    Ok(serde_json::from_value(
                        json!({"choices":[{"message":{"content":"# fixture answer"}}]}),
                    )
                    .unwrap())
                }
            },
        );
        if should_fail {
            assert_eq!(result.unwrap_err().to_string(), "Network error: fixture");
        } else {
            assert_eq!(result.unwrap(), ["# fixture answer"]);
        }
    }
    assert_eq!((revision, sends), (2, 2));
}

#[test]
fn even_an_empty_pipe_bypasses_the_catalog_and_uses_the_pipe_template() {
    query_api_with(
        "request",
        &[],
        Some(""),
        || panic!("pipe queries never read tools"),
        |messages| {
            assert!(
                messages[0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("---BEGIN PIPED DATA---\n\n---END PIPED DATA---")
            );
            Ok(serde_json::from_value(
                json!({"choices":[{"message":{"content":"# fixture answer"}}]}),
            )
            .unwrap())
        },
    )
    .unwrap();
}
