//! Each AI query renders history, refreshes the tool catalog when allowed,
//! builds the user message, requests one reply, then parses command proposals.

use super::PromptResult;
use crate::MAX_PIPE_BYTES;
use crate::api::{
    self,
    response::{ApiResponse, commands_from_api_response},
};
use crate::conversation::{ConversationContext, render_context};
use crate::text::truncate_utf8_bytes;
use crate::tools;
use serde_json::{Value, json};

const PROMPT_TEMPLATE: &str = r#"
You are a command-line assistant specialized in MacOS Zsh scripting, helping users both with commands and general assistance.

**Instructions:**
- Analyze if the user is requesting an action/command or making a statement/asking a question
- Address each part of the request, including requests that combine a question with an action.
- Use recorded command statuses and output for follow-ups. A proposal is not execution, and a failed command may have partial effects. Do not repeat successful steps without a reason.
- For ACTION REQUESTS: Generate the appropriate terminal commands
  - Return **only the command**, unless explicitly asked to explain
  - Use **safe practices** (avoid dangerous commands like `rm -rf /`)
  - If multiple commands are needed, return them in sequence
  - Keep state-dependent steps such as `cd` or `export` in one `&&` chain
  - Explanations go **before** commands, prefixed with `# `
- For STATEMENTS/QUESTIONS: Respond conversationally
  - Prefix your entire response with `# ` to indicate it's not a command
  - Be helpful, concise, and friendly
  - If discussing the tool itself, acknowledge its capabilities
- Assume the user is using **MacOS** **Zsh** unless they specify otherwise
- Do not use any code blocks (```) in your response

{tool_catalog}
**Examples:**
User: How do I kill a process running on port 5234?
Response:
  lsof -i :5234
  kill $(lsof -t -i :5234)

User: this is a great tool
Response:
  # Thank you! I'm glad you're finding it helpful. Feel free to ask me to run any commands or questions you have.

User: what did we just do?
Response:
  # We just [explain the previous actions based on context]. Is there anything else you'd like to do?

**User request:** {query}
"#;

const PIPE_PROMPT_TEMPLATE: &str = r#"
You are a command-line assistant specialized in MacOS Zsh scripting and data analysis.

The user has piped the following data to you via stdin:

---BEGIN PIPED DATA---
{piped_data}
---END PIPED DATA---

**Instructions:**
- The user's request relates to the piped data above
- If the user asks you to analyze, summarize, filter, transform, or explain the data, respond conversationally (prefix lines with `# `)
- If the user asks you to generate a command that processes data like this, return the command
- If no specific request is given, provide a brief, useful summary of the data (prefix with `# `)
- Use **safe practices** (avoid dangerous commands like `rm -rf /`)
- Assume the user is using **MacOS** **Zsh** unless they specify otherwise
- Do not use any code blocks (```) in your response
- Be concise and directly useful

**User request:** {query}
"#;

fn build_prompt(prompt: &str, piped_data: Option<&str>, tool_catalog: &str) -> String {
    if let Some(data) = piped_data {
        let (prefix, was_truncated) = truncate_utf8_bytes(data, MAX_PIPE_BYTES);
        let display_data = if was_truncated {
            format!(
                "{prefix}...\n(input truncated to {} KB)",
                MAX_PIPE_BYTES / 1024
            )
        } else {
            data.to_string()
        };
        // No tool catalog in pipe mode: piped data is untrusted context, and
        // combining it with tool suggestions would stack two injection surfaces.
        // Insert the request before the earlier context slot, replacing each
        // slot once so literal placeholders in user content are left alone.
        PIPE_PROMPT_TEMPLATE
            .replacen("{query}", prompt, 1)
            .replacen("{piped_data}", &display_data, 1)
    } else {
        PROMPT_TEMPLATE
            .replacen("{query}", prompt, 1)
            .replacen("{tool_catalog}", tool_catalog, 1)
    }
}

enum QueryState {
    RenderingHistory,
    LoadingCatalog(Vec<Value>),
    BuildingPrompt {
        messages: Vec<Value>,
        catalog: String,
    },
    Sending(Vec<Value>),
    Parsing(ApiResponse),
    Complete(Vec<String>),
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn query_api(
    prompt: &str,
    model: &str,
    api_key: &str,
    history: &[ConversationContext],
    piped_data: Option<&str>,
) -> PromptResult<Vec<String>> {
    query_api_with(
        prompt,
        history,
        piped_data,
        || {
            tools::tools_root()
                .map(|root| tools::catalog_block(&root))
                .unwrap_or_default()
        },
        |messages| api::call_llm(messages, model, api_key),
    )
}

fn query_api_with(
    prompt: &str,
    history: &[ConversationContext],
    piped_data: Option<&str>,
    mut load_catalog: impl FnMut() -> String,
    mut send: impl FnMut(Vec<Value>) -> PromptResult<ApiResponse>,
) -> PromptResult<Vec<String>> {
    let mut state = QueryState::RenderingHistory;
    loop {
        state = match state {
            QueryState::RenderingHistory => {
                let messages = if history.is_empty() {
                    Vec::new()
                } else {
                    vec![json!({"role": "system", "content": render_context(history)})]
                };
                QueryState::LoadingCatalog(messages)
            }
            QueryState::LoadingCatalog(messages) => {
                // Re-read the catalog on each ordinary query. Pipe context
                // must not cause even an attempted catalog lookup.
                let catalog = if piped_data.is_none() {
                    load_catalog()
                } else {
                    String::new()
                };
                QueryState::BuildingPrompt { messages, catalog }
            }
            QueryState::BuildingPrompt {
                mut messages,
                catalog,
            } => {
                messages.push(json!({
                    "role": "user",
                    "content": build_prompt(prompt, piped_data, &catalog),
                }));
                QueryState::Sending(messages)
            }
            QueryState::Sending(messages) => match send(messages) {
                Ok(response) => QueryState::Parsing(response),
                Err(error) => QueryState::Failed(error),
            },
            QueryState::Parsing(response) => match commands_from_api_response(response) {
                Ok(commands) => QueryState::Complete(commands),
                Err(error) => QueryState::Failed(error),
            },
            QueryState::Complete(commands) => return Ok(commands),
            QueryState::Failed(error) => return Err(error),
        };
    }
}

#[cfg(test)]
mod tests;
