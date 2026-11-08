//! CLI, interactive, and generated tool calls share preparation and execution.
//! Rejected preparation never invokes the preview or execution callbacks. Once
//! prepared, the caller receives the resolved command and execution result.

use super::integrity::inspect_tool;
use super::{ToolManifest, is_valid_tool_name, script_path, shell_quote, tools_root};
use crate::contains_shell_metacharacters;
use crate::subprocess::run_command_with_output;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) enum InvocationOrigin {
    User,
    Model,
}

type ExecutionResult = Result<String, Box<dyn std::error::Error>>;

#[derive(Debug)]
pub(crate) struct ToolExecution {
    pub(crate) command: String,
    pub(crate) result: ExecutionResult,
}

enum InvocationState {
    CheckingArguments,
    LocatingRoot,
    CheckingName(PathBuf),
    Inspecting(PathBuf),
    Preparing {
        root: PathBuf,
        manifest: ToolManifest,
    },
    Ready(String),
    Running(String),
    Complete(ToolExecution),
    Rejected(String),
}

pub(crate) fn invoke_tool(
    name: &str,
    args_tail: &str,
    origin: InvocationOrigin,
    before_execute: impl FnMut(&str),
) -> Result<ToolExecution, String> {
    invoke_with(
        name,
        args_tail,
        origin,
        || tools_root().ok_or_else(|| "HOME is not set; cannot locate ~/.ask/tools.".to_string()),
        before_execute,
        run_command_with_output,
    )
}

pub(super) fn invoke_with(
    name: &str,
    args_tail: &str,
    origin: InvocationOrigin,
    mut locate_root: impl FnMut() -> Result<PathBuf, String>,
    mut before_execute: impl FnMut(&str),
    mut execute: impl FnMut(&str) -> ExecutionResult,
) -> Result<ToolExecution, String> {
    let mut state = InvocationState::CheckingArguments;
    loop {
        state = match state {
            InvocationState::CheckingArguments => {
                // User-written shell tails retain their existing interpretation.
                // Approval covers source, so a model cannot append shell syntax.
                if matches!(origin, InvocationOrigin::Model)
                    && contains_shell_metacharacters(args_tail)
                {
                    InvocationState::Rejected(format!(
                        "Refusing to run '${name}' with shell metacharacters in its arguments: {args_tail}"
                    ))
                } else {
                    InvocationState::LocatingRoot
                }
            }
            InvocationState::LocatingRoot => match locate_root() {
                Ok(root) => InvocationState::CheckingName(root),
                Err(error) => InvocationState::Rejected(error),
            },
            InvocationState::CheckingName(root) => {
                if is_valid_tool_name(name) {
                    InvocationState::Inspecting(root)
                } else {
                    InvocationState::Rejected(format!("Invalid tool name '{name}'."))
                }
            }
            InvocationState::Inspecting(root) => {
                match inspect_tool(&root, name).and_then(|tool| tool.into_approved_manifest(name)) {
                    Ok(manifest) => InvocationState::Preparing { root, manifest },
                    Err(error) => InvocationState::Rejected(error),
                }
            }
            InvocationState::Preparing { root, manifest } => {
                let script = script_path(&root, name, manifest.lang);
                let mut command = format!(
                    "{} {}",
                    manifest.lang.interpreter(),
                    shell_quote(&script.to_string_lossy()),
                );
                let args_tail = args_tail.trim();
                if !args_tail.is_empty() {
                    command.push(' ');
                    command.push_str(args_tail);
                }
                InvocationState::Ready(command)
            }
            InvocationState::Ready(command) => {
                before_execute(&command);
                InvocationState::Running(command)
            }
            InvocationState::Running(command) => {
                let result = execute(&command);
                InvocationState::Complete(ToolExecution { command, result })
            }
            InvocationState::Complete(execution) => return Ok(execution),
            InvocationState::Rejected(error) => return Err(error),
        };
    }
}

#[cfg(test)]
mod tests;
