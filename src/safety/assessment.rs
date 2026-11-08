//! One bounded provider request. Parsed assessments remain evidence; callers
//! apply confidence thresholds and local execution policy separately.

use super::{Assessment, MAX_COMMAND_BYTES, request};
use serde::Deserialize;
use std::io::Read;
use std::time::Duration;

pub(super) const API_URL: &str = "https://openrouter.ai/api/alpha/decisions";
pub(super) const MAX_RESPONSE_BYTES: u64 = 32 * 1024;

#[derive(Deserialize)]
struct Response {
    answers: Answers,
}

#[derive(Deserialize)]
struct Answers {
    safety: Assessment,
}

enum AssessmentState {
    CheckingSize,
    Sending,
    Reading(ureq::Response),
    Decoding(Vec<u8>),
    Complete(Assessment),
    Failed(&'static str),
}

pub(super) fn assess_at(
    command: &str,
    api_key: &str,
    endpoint: &str,
) -> Result<Assessment, &'static str> {
    let mut state = AssessmentState::CheckingSize;
    loop {
        state = match state {
            AssessmentState::CheckingSize => {
                if command.len() > MAX_COMMAND_BYTES {
                    AssessmentState::Failed("command too long to assess")
                } else {
                    AssessmentState::Sending
                }
            }
            AssessmentState::Sending => {
                match ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(2))
                    .timeout(Duration::from_secs(5))
                    .build()
                    .post(endpoint)
                    .set("Authorization", &format!("Bearer {api_key}"))
                    .set("Content-Type", "application/json")
                    .send_json(request(command))
                {
                    Ok(response) => AssessmentState::Reading(response),
                    Err(_) => AssessmentState::Failed("Jev safety check unavailable"),
                }
            }
            AssessmentState::Reading(response) => {
                // Provider bodies never appear in errors, and allocation is bounded.
                let mut reader = response.into_reader().take(MAX_RESPONSE_BYTES + 1);
                let mut bytes = Vec::new();
                match reader.read_to_end(&mut bytes) {
                    Err(_) => AssessmentState::Failed("invalid Jev safety response"),
                    Ok(_) if bytes.len() as u64 > MAX_RESPONSE_BYTES => {
                        AssessmentState::Failed("oversized Jev safety response")
                    }
                    Ok(_) => AssessmentState::Decoding(bytes),
                }
            }
            AssessmentState::Decoding(bytes) => match serde_json::from_slice::<Response>(&bytes) {
                Ok(parsed) => AssessmentState::Complete(parsed.answers.safety),
                Err(_) => AssessmentState::Failed("invalid Jev safety response"),
            },
            AssessmentState::Complete(assessment) => return Ok(assessment),
            AssessmentState::Failed(reason) => return Err(reason),
        };
    }
}
