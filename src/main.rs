use std::process::exit;

mod api;
mod cli;
mod config;
mod confirmation;
mod conversation;
mod execution;
mod prompt;
mod safety;
mod session;
mod startup;
mod subprocess;
mod terminal_io;
mod text;
mod theme;
mod tools;
mod working_directory;

use subprocess::run_command_with_output;
use theme::Theme;
#[cfg(test)]
use theme::ThemeMode;

const MAX_PIPE_BYTES: usize = 64 * 1024;
fn main() {
    if let Err(err) = startup::run() {
        eprintln!("Error: {err}");
        exit(1);
    }
}

// Shell metacharacters that can smuggle extra commands past the whitelist
// (`ls && rm -rf ~`, `echo $(...)`, redirection, backgrounding). The whole
// line is handed to `$SHELL -c`, so any of these disqualifies the
// auto-execute fast path and routes through LLM confirmation instead.
fn contains_shell_metacharacters(cmd: &str) -> bool {
    cmd.chars().any(|ch| {
        matches!(
            ch,
            ';' | '&' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '\n' | '\r'
        )
    })
}

// Script extensions the fast path may run directly, mapped to the interpreter
// used when the user types a bare filename like `deploy.sh`. Extensions
// without an interpreter (.rs, .go, .java, …) are compiled languages and are
// deliberately absent — they cannot be executed directly.
const SCRIPT_INTERPRETERS: &[(&str, &str)] = &[
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "zsh"),
    ("py", "python3"),
    ("js", "node"),
    ("mjs", "node"),
    ("rb", "ruby"),
    ("pl", "perl"),
    ("php", "php"),
];

const INTERPRETERS: &[&str] = &[
    "python", "python3", "node", "ruby", "perl", "php", "bash", "sh", "zsh",
];

fn interpreter_for_script(path: &str) -> Option<&'static str> {
    let (stem, extension) = path.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    SCRIPT_INTERPRETERS
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, interpreter)| *interpreter)
}

// Check if the input looks like a script file to run. Only plain invocations
// count: an interpreter followed by a script path (`python app.py args…`), a
// `./relative` path, or a bare single-token filename with a known script
// extension. Interpreter flags such as `bash -c` or `python -m` execute
// arbitrary inline code and must NOT bypass confirmation, and `rm build.sh`
// is a destructive command whose argument happens to end in `.sh`, not a
// script execution.
fn is_script_execution(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if contains_shell_metacharacters(cmd) {
        return false;
    }

    let mut tokens = cmd.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };

    if first.starts_with("./") {
        return true;
    }

    if INTERPRETERS.contains(&first) {
        // The first argument must be a script path, not a flag like -c/-m/-e.
        return matches!(tokens.next(), Some(arg) if !arg.starts_with('-'));
    }

    tokens.next().is_none() && interpreter_for_script(first).is_some()
}

// Safe commands that can be executed directly without LLM confirmation.
// Matching is on the first token only, and contains_shell_metacharacters has
// already ruled out anything that could chain a second command onto the line.
fn is_safe_direct_command(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if contains_shell_metacharacters(cmd) {
        return false;
    }

    if is_script_execution(cmd) {
        return true;
    }

    // Read-only commands that stay safe with arbitrary arguments.
    const SAFE_WITH_ARGS: &[&str] = &[
        "ls", "cd", "cat", "echo", "pwd", "head", "tail", "grep", "find", "wc", "diff",
    ];

    // Commands that are only safe as an exact invocation — adding any
    // argument falls through to LLM confirmation (the conservative direction).
    const SAFE_EXACT: &[&str] = &[
        // File listing and navigation
        "ll",
        "la",
        "dir",
        "tree", // File reading (non-destructive)
        "less",
        "more",
        "file",
        "stat", // System information
        "date",
        "uptime",
        "whoami",
        "hostname",
        "uname",
        "id",
        "df",
        "du",
        "free",
        "top",
        "ps",
        "who",
        "w", // Network information (read-only)
        "ifconfig",
        "ping",
        "netstat",
        "curl",
        "wget",
        "dig",
        "nslookup", // Environment
        "env",
        "printenv",
        "which",
        "type",
        "alias", // Git read operations
        "git status",
        "git log",
        "git diff",
        "git branch",
        "git remote",
        // Package managers (list only)
        "brew list",
        "npm list",
        "pip list",
        "cargo search", // History and help
        "history",
        "help",
        "man",
    ];

    let cmd_lower = cmd.to_lowercase();
    let first_token = cmd_lower.split_whitespace().next().unwrap_or("");

    if SAFE_WITH_ARGS.contains(&first_token) {
        // `find` can destroy files through its own flags, no shell
        // metacharacters required — those variants still need confirmation.
        if first_token == "find" {
            const DANGEROUS_FIND_FLAGS: &[&str] =
                &["-delete", "-exec", "-execdir", "-ok", "-okdir"];
            return !cmd_lower
                .split_whitespace()
                .any(|token| DANGEROUS_FIND_FLAGS.contains(&token));
        }
        return true;
    }

    SAFE_EXACT.contains(&cmd_lower.as_str())
}

/// Executes one confirmed line from the model, resolving `$name args` tool
/// invocations to their approved script first. Ordinary lines run unchanged.
fn run_line(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    if let Some((name, args_tail)) = tools::parse_tool_invocation(command) {
        return tools::invocation::invoke_tool(
            &name,
            &args_tail,
            tools::invocation::InvocationOrigin::Model,
            |_| {},
        )?
        .result;
    }
    run_command_with_output(command)
}

#[cfg(test)]
mod tests;

/// Integration tests that make real API calls to the configured LLM.
/// Run with: cargo test -- --ignored --show-output
#[cfg(test)]
mod integration_tests;
