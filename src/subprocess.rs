//! Own each child from preparation through capture, wait, and directory sync.
//! Shell commands and inherited-I/O utilities share the same lifecycle.

use crate::terminal_io::{
    CapturedOutput, IoMode, MAX_CAPTURE_BYTES, capture_child_output, output_tail,
};
use crate::working_directory::DirectoryCapture;
use std::env;
use std::process::{Child, Command, ExitStatus};

type ProcessResult<T> = Result<T, Box<dyn std::error::Error>>;

enum ProcessRequest<'a> {
    Shell(&'a str),
    Utility {
        program: &'a str,
        args: &'a [&'a str],
    },
}

enum DirectoryTracking {
    Reported(DirectoryCapture),
    Untracked,
}

struct PreparedProcess {
    command: Command,
    mode: IoMode,
    directory: DirectoryTracking,
}

impl PreparedProcess {
    fn new(request: ProcessRequest<'_>) -> Self {
        match request {
            ProcessRequest::Shell(source) => {
                let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
                let capture = DirectoryCapture::new();
                let script = capture.shell_script(source);
                let mode = IoMode::for_shell();
                let mut command = match mode {
                    IoMode::Terminal => {
                        // A PTY preserves terminal stdin/stdout/stderr while output
                        // is streamed into bounded memory. No transcript is saved.
                        let mut command = Command::new("/usr/bin/script");
                        #[cfg(target_os = "macos")]
                        command.args(["-q", "/dev/null", &shell, "-c", &script]);
                        #[cfg(not(target_os = "macos"))]
                        command.args([
                            "-q",
                            "-e",
                            "-c",
                            &format!(
                                "'{}' -c '{}'",
                                shell.replace('\'', "'\\''"),
                                script.replace('\'', "'\\''")
                            ),
                            "/dev/null",
                        ]);
                        command
                    }
                    _ => {
                        let mut command = Command::new(&shell);
                        command.arg("-c").arg(&script);
                        command
                    }
                };
                command.env("ASK_CWD_CAPTURE", capture.path());
                Self {
                    command,
                    mode,
                    directory: DirectoryTracking::Reported(capture),
                }
            }
            ProcessRequest::Utility { program, args } => {
                let mut command = Command::new(program);
                command.args(args);
                Self {
                    command,
                    mode: IoMode::Inherited,
                    directory: DirectoryTracking::Untracked,
                }
            }
        }
    }
}

struct ProcessOutcome {
    status: ExitStatus,
    output: String,
}

enum ProcessState<'a> {
    Preparing(ProcessRequest<'a>),
    Spawning(PreparedProcess),
    Capturing {
        child: Child,
        mode: IoMode,
        directory: DirectoryTracking,
    },
    Waiting {
        child: Child,
        output: CapturedOutput,
        directory: DirectoryTracking,
    },
    ResolvingOutput {
        status: ExitStatus,
        output: CapturedOutput,
        directory: DirectoryTracking,
    },
    Synchronizing {
        outcome: ProcessOutcome,
        directory: DirectoryTracking,
    },
    Complete(ProcessOutcome),
    Failed(Box<dyn std::error::Error>),
}

fn run_process(request: ProcessRequest<'_>) -> ProcessResult<ProcessOutcome> {
    let mut state = ProcessState::Preparing(request);
    loop {
        state = match state {
            ProcessState::Preparing(request) => {
                ProcessState::Spawning(PreparedProcess::new(request))
            }
            ProcessState::Spawning(mut prepared) => {
                prepared.mode.configure(&mut prepared.command);
                match prepared.command.spawn() {
                    Ok(child) => ProcessState::Capturing {
                        child,
                        mode: prepared.mode,
                        directory: prepared.directory,
                    },
                    Err(error) => ProcessState::Failed(error.into()),
                }
            }
            ProcessState::Capturing {
                mut child,
                mode,
                directory,
            } => {
                let output = capture_child_output(&mut child, mode);
                ProcessState::Waiting {
                    child,
                    output,
                    directory,
                }
            }
            ProcessState::Waiting {
                mut child,
                output,
                directory,
            } => match child.wait() {
                // Wait errors take precedence over deferred capture errors.
                Ok(status) => ProcessState::ResolvingOutput {
                    status,
                    output,
                    directory,
                },
                Err(error) => ProcessState::Failed(error.into()),
            },
            ProcessState::ResolvingOutput {
                status,
                output,
                directory,
            } => match output.into_result() {
                Ok(output) => ProcessState::Synchronizing {
                    outcome: ProcessOutcome { status, output },
                    directory,
                },
                Err(error) => ProcessState::Failed(error.into()),
            },
            ProcessState::Synchronizing { outcome, directory } => {
                let result = match directory {
                    DirectoryTracking::Reported(capture) => capture.synchronize(),
                    DirectoryTracking::Untracked => Ok(()),
                };
                match result {
                    Ok(()) => ProcessState::Complete(outcome),
                    Err(error) => ProcessState::Failed(error.into()),
                }
            }
            ProcessState::Complete(outcome) => return Ok(outcome),
            ProcessState::Failed(error) => return Err(error),
        };
    }
}

pub(crate) fn run_command_with_output(command: &str) -> ProcessResult<String> {
    let outcome = run_process(ProcessRequest::Shell(command))?;
    let output = output_tail(&outcome.output, MAX_CAPTURE_BYTES);
    if !outcome.status.success() {
        return Err(format!("{output}\nCommand exited with status {}", outcome.status).into());
    }
    Ok(output)
}

pub(crate) fn run_utility(program: &str, args: &[&str]) -> ProcessResult<ExitStatus> {
    run_process(ProcessRequest::Utility { program, args }).map(|outcome| outcome.status)
}
