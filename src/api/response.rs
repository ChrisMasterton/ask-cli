//! The first choice supplies either command lines or raw saved-tool source.
//! Empty choices and present-but-empty content retain their distinct outcomes.

use super::ApiResult;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ApiResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}

enum ResponseState<'a, P, T> {
    SelectingChoice(P),
    Parsing { content: &'a str, parse: P },
    Complete(T),
    Failed(Box<dyn std::error::Error>),
}

fn parse_response<T, P>(
    response: ApiResponse,
    missing_choice: &'static str,
    parse: P,
) -> ApiResult<T>
where
    P: FnOnce(&str) -> ApiResult<T>,
{
    let mut state = ResponseState::SelectingChoice(parse);
    loop {
        state = match state {
            ResponseState::SelectingChoice(parse) => match response.choices.first() {
                Some(choice) => ResponseState::Parsing {
                    content: choice.message.content.trim(),
                    parse,
                },
                None => ResponseState::Failed(missing_choice.into()),
            },
            ResponseState::Parsing { content, parse } => match parse(content) {
                Ok(parsed) => ResponseState::Complete(parsed),
                Err(error) => ResponseState::Failed(error),
            },
            ResponseState::Complete(parsed) => return Ok(parsed),
            ResponseState::Failed(error) => return Err(error),
        };
    }
}

pub(crate) fn commands_from_api_response(response: ApiResponse) -> ApiResult<Vec<String>> {
    parse_response(response, "No command returned from the model.", |content| {
        let commands = parse_commands(content);
        if commands.is_empty() {
            Err("No response returned from the model.".into())
        } else {
            Ok(commands)
        }
    })
}

pub(crate) fn tool_content_from_api_response(response: ApiResponse) -> ApiResult<String> {
    parse_response(response, "The model returned an empty reply.", |content| {
        // Only the outer whitespace is trimmed. Line indentation and fences
        // belong to the saved-tool parser and must reach it intact.
        Ok(content.to_string())
    })
}

pub(crate) fn parse_commands(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("```") && !line.ends_with("```"))
        // Keep whole shell lines: splitting chains would change quoting,
        // short-circuiting, and state-dependent commands.
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests;
