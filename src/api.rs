//! One chat-completion request, shared by command proposals and saved-tool
//! authoring. Safety assessments retain their separate provider contract.

pub(crate) mod response;

use response::ApiResponse;
use serde_json::{Value, json};
use std::time::Duration;

const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
type ApiResult<T> = Result<T, Box<dyn std::error::Error>>;

enum RequestState {
    Building(Vec<Value>),
    Sending(Value),
    Decoding(ureq::Response),
    ReadingError { code: u16, response: ureq::Response },
    Complete(ApiResponse),
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn call_llm(messages: Vec<Value>, model: &str, api_key: &str) -> ApiResult<ApiResponse> {
    call_llm_at(messages, model, api_key, API_URL)
}

fn call_llm_at(
    messages: Vec<Value>,
    model: &str,
    api_key: &str,
    endpoint: &str,
) -> ApiResult<ApiResponse> {
    let mut state = RequestState::Building(messages);
    loop {
        state = match state {
            RequestState::Building(messages) => RequestState::Sending(json!({
                "model": model,
                "messages": messages,
            })),
            RequestState::Sending(body) => {
                let response = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(10))
                    .timeout(Duration::from_secs(120))
                    .build()
                    .post(endpoint)
                    .set("Authorization", &format!("Bearer {api_key}"))
                    .set("Content-Type", "application/json")
                    .send_json(body);
                match response {
                    Ok(response) => RequestState::Decoding(response),
                    Err(ureq::Error::Status(code, response)) => {
                        RequestState::ReadingError { code, response }
                    }
                    Err(error) => RequestState::Failed(format!("Network error: {error}").into()),
                }
            }
            // Use the provider library's existing JSON/body readers, preserving
            // its decoding, size limits, and error representations.
            RequestState::Decoding(response) => match response.into_json::<ApiResponse>() {
                Ok(response) => RequestState::Complete(response),
                Err(error) => RequestState::Failed(error.into()),
            },
            RequestState::ReadingError { code, response } => {
                let text = response.into_string().unwrap_or_else(|_| String::new());
                RequestState::Failed(format!("API error {code}: {text}").into())
            }
            RequestState::Complete(response) => return Ok(response),
            RequestState::Failed(error) => return Err(error),
        };
    }
}

#[cfg(test)]
mod tests;
