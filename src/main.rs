use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read as _, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio, exit};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

mod tools;

const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";
// Token limits - most models support 4K-128K, we'll be conservative
const MAX_CONTEXT_TOKENS: usize = 3000; // Reserve ~1000 for response
const TOKEN_ESTIMATE_RATIO: usize = 4; // Roughly 1 token per 4 characters
const MAX_PIPE_BYTES: usize = 64 * 1024; // 64 KB max piped input to keep context reasonable
static NEXT_CWD_CAPTURE_ID: AtomicU64 = AtomicU64::new(0);
const PROMPT_TEMPLATE: &str = r#"
You are a command-line assistant specialized in MacOS Zsh scripting, helping users both with commands and general assistance.

**Instructions:**
- Analyze if the user is requesting an action/command or making a statement/asking a question
- The FIRST line of every response must be a safety verdict: `SAFE: yes` or `SAFE: no`
  - `SAFE: yes` — every command is read-only or trivially reversible (listing, viewing, navigating, querying status)
  - `SAFE: no` — any command modifies, deletes, moves or overwrites files or data, kills processes, installs software, or changes system or git state
  - If the response contains no commands at all, use `SAFE: yes`
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
  SAFE: no
  lsof -i :5234
  kill $(lsof -t -i :5234)

User: this is a great tool
Response:
  SAFE: yes
  # Thank you! I'm glad you're finding it helpful. Feel free to ask me to run any commands or questions you have.

User: what did we just do?
Response:
  SAFE: yes
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
- The FIRST line of every response must be `SAFE: yes` (no commands, or only read-only commands) or `SAFE: no` (any command that could modify or delete data)
- If the user asks you to analyze, summarize, filter, transform, or explain the data, respond conversationally (prefix lines with `# `)
- If the user asks you to generate a command that processes data like this, return the command
- If no specific request is given, provide a brief, useful summary of the data (prefix with `# `)
- Use **safe practices** (avoid dangerous commands like `rm -rf /`)
- Assume the user is using **MacOS** **Zsh** unless they specify otherwise
- Do not use any code blocks (```) in your response
- Be concise and directly useful

**User request:** {query}
"#;

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        exit(1);
    }
}

/// Reads up to `MAX_PIPE_BYTES` from stdin — plus one sentinel byte so
/// downstream truncation is detectable — returning `None` when stdin is a
/// terminal or the pipe is empty.
fn read_piped_stdin() -> Option<String> {
    if io::stdin().is_terminal() {
        return None;
    }
    let mut buf = Vec::with_capacity(8192);
    let _ = io::stdin()
        .lock()
        .take(MAX_PIPE_BYTES as u64 + 1)
        .read_to_end(&mut buf);
    if buf.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).to_string())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Read piped data BEFORE anything else touches stdin.
    let piped_data = read_piped_stdin();

    let args = parse_args()?;
    let theme = Theme::from_mode(args.theme);

    // `ask auto on` / `ask auto off` toggles auto mode without an API call.
    if let Some(prompt) = &args.prompt
        && let Some(enabled) = parse_auto_toggle(prompt)
    {
        set_auto_mode(enabled, &theme);
        return Ok(());
    }

    // `ask model [MODEL|reset]` shows or persists the default model.
    if let Some(prompt) = &args.prompt
        && let Some(command) = parse_model_command(prompt)
    {
        apply_model_command(command, &theme);
        return Ok(());
    }

    // `ask '$name args'` runs an approved saved tool directly — no API call,
    // so like the config commands above it works without an API key.
    if let Some(prompt) = &args.prompt
        && let Some((name, args_tail)) = tools::parse_tool_invocation(prompt)
    {
        let root = tools::tools_root().ok_or("HOME is not set; cannot locate ~/.ask/tools.")?;
        let resolved = tools::resolve_approved_tool(&root, &name, &args_tail)?;
        println!(
            "{} {}",
            theme.prompt_text("run>"),
            theme.command_text(&resolved)
        );
        run_command_with_output(&resolved)?;
        return Ok(());
    }

    // `ask tool ...` manages the saved tool library. Only `tool new` and
    // `tool improve` call the LLM; they read the API key themselves.
    if let Some(prompt) = &args.prompt
        && let Some(command) = tools::parse_tool_command(prompt)
    {
        tools::handle_tool_command(command, &args.model, &theme)?;
        return Ok(());
    }

    let api_key = env::var("OPENROUTER_ASK_API_KEY")
        .map_err(|_| "Please set the OPENROUTER_ASK_API_KEY environment variable.")?;

    match args.prompt {
        Some(prompt) => {
            // Single prompt mode (with optional piped data)
            process_prompt(
                &prompt,
                &args.model,
                &api_key,
                &theme,
                piped_data.as_deref(),
                args.auto,
            )?;
        }
        None if piped_data.is_some() => {
            // Data piped in but no prompt – summarize / analyse by default
            process_prompt(
                "Summarize and explain this data",
                &args.model,
                &api_key,
                &theme,
                piped_data.as_deref(),
                args.auto,
            )?;
        }
        None => {
            // Interactive mode (no pipe)
            run_interactive_mode(&args.model, &api_key, &theme, args.auto)?;
        }
    }

    Ok(())
}

/// Parses the `auto on` / `auto off` toggle command.
fn parse_auto_toggle(input: &str) -> Option<bool> {
    match input.trim().to_lowercase().as_str() {
        "auto on" => Some(true),
        "auto off" => Some(false),
        _ => None,
    }
}

/// Persists the auto-mode preference and reports the new state.
fn set_auto_mode(enabled: bool, theme: &Theme) {
    let mut config = Config::load();
    config.auto = enabled;
    if let Err(err) = config.save() {
        eprintln!("Warning: could not save auto preference: {err}");
    }
    println!("{}", theme.helper_text(auto_mode_description(enabled)));
}

fn auto_mode_description(enabled: bool) -> &'static str {
    if enabled {
        "Auto mode ON — commands the model marks as safe run without confirmation"
    } else {
        "Auto mode OFF — every generated command asks for confirmation"
    }
}

#[derive(Debug)]
enum ModelCommand {
    Show,
    Set(String),
    Reset,
}

/// Parses the `model` / `model <id>` / `model reset` command. Anything with
/// more than one argument is treated as an ordinary prompt, not a command.
fn parse_model_command(input: &str) -> Option<ModelCommand> {
    let mut tokens = input.split_whitespace();
    if !tokens.next()?.eq_ignore_ascii_case("model") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(ModelCommand::Show);
    };
    if tokens.next().is_some() {
        return None;
    }
    match arg.to_lowercase().as_str() {
        "reset" | "default" => Some(ModelCommand::Reset),
        _ => Some(ModelCommand::Set(arg.to_string())),
    }
}

/// Shows, persists, or resets the saved model preference. Returns the model
/// now in effect so interactive mode can switch the live session too.
fn apply_model_command(command: ModelCommand, theme: &Theme) -> String {
    let mut config = Config::load();
    match command {
        ModelCommand::Show => {
            let (model, source) = match &config.model {
                Some(model) => (model.clone(), "saved in ~/.ask/config"),
                None => (DEFAULT_MODEL.to_string(), "built-in default"),
            };
            println!("{}", theme.helper_text(&format!("Model: {model} ({source})")));
            model
        }
        ModelCommand::Set(model) => {
            if !model.contains('/') {
                println!(
                    "{}",
                    theme.helper_text("Note: OpenRouter model IDs usually look like vendor/model-name")
                );
            }
            config.model = Some(model.clone());
            if let Err(err) = config.save() {
                eprintln!("Warning: could not save model preference: {err}");
            }
            println!(
                "{}",
                theme.helper_text(&format!("Model set to {model} and saved as the default"))
            );
            model
        }
        ModelCommand::Reset => {
            config.model = None;
            if let Err(err) = config.save() {
                eprintln!("Warning: could not save model preference: {err}");
            }
            println!(
                "{}",
                theme.helper_text(&format!(
                    "Model reset to the built-in default ({DEFAULT_MODEL})"
                ))
            );
            DEFAULT_MODEL.to_string()
        }
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
        "ll", "la", "dir", "tree", // File reading (non-destructive)
        "less", "more", "file", "stat", // System information
        "date", "uptime", "whoami", "hostname", "uname", "id", "df", "du", "free", "top", "ps",
        "who", "w", // Network information (read-only)
        "ifconfig", "ping", "netstat", "curl", "wget", "dig", "nslookup", // Environment
        "env", "printenv", "which", "type", "alias", // Git read operations
        "git status", "git log", "git diff", "git branch", "git remote",
        // Package managers (list only)
        "brew list", "npm list", "pip list", "cargo search", // History and help
        "history", "help", "man",
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

fn print_banner(theme: &Theme, auto: bool) {
    println!(
        "{}",
        theme.prompt_text(
            "Interactive mode. Commands: 'exit', 'clear', 'finder', 'auto on|off', 'tool', '$name'"
        )
    );
    println!(
        "{}",
        theme.helper_text("Common commands and scripts execute directly without confirmation")
    );
    println!(
        "{}",
        theme.helper_text("Shortcuts: q=quit, .=pwd, ..=cd ..")
    );
    if auto {
        println!("{}", theme.helper_text(auto_mode_description(true)));
    }

    // Show current directory
    if let Ok(cwd) = env::current_dir() {
        println!("{}", theme.helper_text(&format!("📁 {}", cwd.display())));
    }
    println!();
}

fn run_interactive_mode(
    initial_model: &str,
    api_key: &str,
    theme: &Theme,
    initial_auto: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut model = initial_model.to_string();
    let mut auto = initial_auto;
    print_banner(theme, auto);

    let mut rl = DefaultEditor::new()?;
    let mut history: Vec<ConversationContext> = Vec::new();

    loop {
        // Get current directory for prompt - show folder name or ~ for home
        let cwd_display = if let Ok(cwd) = env::current_dir() {
            if let Ok(home) = env::var("HOME") {
                if cwd.to_string_lossy() == home {
                    "~".to_string()
                } else if let Some(relative) =
                    cwd.to_string_lossy().strip_prefix(&format!("{}/", home))
                {
                    format!("~/{}", relative.rsplit('/').next().unwrap_or(relative))
                } else if let Some(name) = cwd.file_name() {
                    name.to_string_lossy().to_string()
                } else {
                    "/".to_string() // Root directory
                }
            } else {
                cwd.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
            }
        } else {
            "?".to_string()
        };

        let prompt = format!("{} ", theme.prompt_text(&format!("ask [{}]>", cwd_display)));
        let input = match rl.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: cancel current line, continue loop
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                println!("Goodbye!");
                break;
            }
            Err(err) => {
                return Err(err.into());
            }
        };
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Add to readline history for arrow-key navigation
        let _ = rl.add_history_entry(input);

        // Shortcuts for common commands
        if input == "q" || input == "exit" || input == "quit" {
            println!("Goodbye!");
            break;
        }

        if input == "." {
            // Shortcut for pwd
            let cwd = env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            println!(
                "{} {}",
                theme.prompt_text("run>"),
                theme.command_text("pwd")
            );
            println!("{}", cwd);

            // Add to history
            history.push(ConversationContext {
                prompt: "pwd".to_string(),
                commands: vec!["pwd".to_string()],
                outputs: vec![cwd],
            });
            continue;
        }

        if input == ".." {
            // Shortcut for cd ..
            println!(
                "{} {}",
                theme.prompt_text("run>"),
                theme.command_text("cd ..")
            );
            match env::set_current_dir("..") {
                Ok(_) => {
                    let cwd = env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "unknown".to_string());
                    println!(
                        "{}",
                        theme.helper_text(&format!("Changed directory to: {}", cwd))
                    );

                    // Add to history
                    history.push(ConversationContext {
                        prompt: "cd ..".to_string(),
                        commands: vec!["cd ..".to_string()],
                        outputs: vec![format!("Changed to: {}", cwd)],
                    });
                }
                Err(e) => {
                    eprintln!("Failed to change directory: {}", e);
                }
            }
            continue;
        }

        if input == "clear" {
            // Clear the screen and reset context
            Command::new("clear").status()?;
            history.clear();
            print_banner(theme, auto);
            continue;
        }

        if let Some(enabled) = parse_auto_toggle(input) {
            auto = enabled;
            set_auto_mode(enabled, theme);
            continue;
        }

        if input == "auto" {
            println!("{}", theme.helper_text(auto_mode_description(auto)));
            continue;
        }

        if let Some(command) = parse_model_command(input) {
            model = apply_model_command(command, theme);
            continue;
        }

        if input == "finder" {
            // Open Finder at current directory
            match Command::new("open").arg(".").status() {
                Ok(_) => println!(
                    "{}",
                    theme.helper_text("Opened Finder at current directory")
                ),
                Err(e) => eprintln!("Failed to open Finder: {}", e),
            }
            continue;
        }

        // `$name args` runs an approved saved tool. This must come before
        // is_safe_direct_command: `$` is a shell metacharacter, so the line
        // would otherwise fall through to the LLM.
        if let Some((name, args_tail)) = tools::parse_tool_invocation(input) {
            let resolved = tools::tools_root()
                .ok_or_else(|| "HOME is not set; cannot locate ~/.ask/tools.".to_string())
                .and_then(|root| tools::resolve_approved_tool(&root, &name, &args_tail));
            match resolved {
                Ok(resolved) => {
                    println!(
                        "{} {}",
                        theme.prompt_text("run>"),
                        theme.command_text(&resolved)
                    );
                    match run_command_with_output(&resolved) {
                        Ok(output) => {
                            // Store what actually ran so the model sees it.
                            history.push(ConversationContext {
                                prompt: input.to_string(),
                                commands: vec![resolved],
                                outputs: vec![output],
                            });
                            warn_if_context_compacted(&history, theme);
                        }
                        Err(e) => eprintln!("Command failed: {e}"),
                    }
                }
                Err(e) => eprintln!("{e}"),
            }
            continue;
        }

        // `tool ...` manages the saved tool library.
        if let Some(command) = tools::parse_tool_command(input) {
            if let Err(e) = tools::handle_tool_command(command, &model, theme) {
                eprintln!("Error: {e}");
            }
            continue;
        }

        // Check if it's a safe direct command
        if is_safe_direct_command(input) {
            // Determine the actual command to run
            let command_to_run = if input == "ls" {
                // Special handling for plain 'ls' - convert to 'ls -l' for better info
                "ls -l".to_string()
            } else if is_script_execution(input) && !input.contains(' ') {
                // Bare script name — prepend the interpreter for its extension
                match interpreter_for_script(input) {
                    Some(interpreter) => format!("{interpreter} {input}"),
                    None => input.to_string(),
                }
            } else {
                input.to_string()
            };

            println!(
                "{} {}",
                theme.prompt_text("run>"),
                theme.command_text(&command_to_run)
            );

            match run_command_with_output(&command_to_run) {
                Ok(output) => {
                    // Add to history - store what was actually executed
                    history.push(ConversationContext {
                        prompt: input.to_string(),
                        commands: vec![command_to_run.clone()],
                        outputs: vec![output],
                    });
                }
                Err(e) => {
                    eprintln!("Command failed: {}", e);
                }
            }

            warn_if_context_compacted(&history, theme);

            continue;
        }

        match process_prompt_with_context(input, &model, api_key, theme, &history, None, auto) {
            Ok((commands, outputs)) => {
                // Add to history
                history.push(ConversationContext {
                    prompt: input.to_string(),
                    commands: commands.clone(),
                    outputs,
                });

                warn_if_context_compacted(&history, theme);
            }
            Err(err) => {
                eprintln!("Error: {}", err);
                // Continue the loop even on error in interactive mode
            }
        }

        println!(); // Add blank line between prompts
    }

    Ok(())
}

fn process_prompt(
    prompt: &str,
    model: &str,
    api_key: &str,
    theme: &Theme,
    piped_data: Option<&str>,
    auto_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let starting_dir = env::current_dir().ok();
    process_prompt_with_context(prompt, model, api_key, theme, &[], piped_data, auto_mode)?;
    if let (Some(starting_dir), Ok(final_dir)) = (starting_dir, env::current_dir())
        && starting_dir != final_dir
    {
        println!(
            "{}",
            theme.helper_text(&format!(
                "Note: the command used {}, but your calling shell remains in {}. Run ask interactively to keep directory changes between prompts.",
                final_dir.display(),
                starting_dir.display()
            ))
        );
    }
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / TOKEN_ESTIMATE_RATIO
}

fn estimate_context_tokens(history: &[ConversationContext]) -> usize {
    let mut total_chars = 0;
    for ctx in history {
        total_chars += ctx.prompt.len();
        for cmd in &ctx.commands {
            total_chars += cmd.len();
        }
        for output in &ctx.outputs {
            total_chars += output.len().min(500); // Count truncated size
        }
    }
    total_chars / TOKEN_ESTIMATE_RATIO
}

fn warn_if_context_compacted(history: &[ConversationContext], theme: &Theme) {
    if estimate_context_tokens(history) > MAX_CONTEXT_TOKENS {
        println!(
            "{}",
            theme.helper_text(
                "Note: Context is being automatically compacted to fit within token limits."
            )
        );
    }
}

fn compact_history(history: &[ConversationContext]) -> String {
    let mut context = String::from("Previous commands and outputs in this session:\n\n");
    let mut total_tokens = estimate_tokens(&context);
    let mut contexts_to_include = Vec::new();

    // Start from most recent and work backwards
    for ctx in history.iter().rev() {
        let mut ctx_str = format!("User: {}\n", ctx.prompt);
        for cmd in &ctx.commands {
            ctx_str.push_str(&format!("Command: {}\n", cmd));
        }
        for output in &ctx.outputs {
            if !output.is_empty() {
                // Truncate very long outputs more aggressively when compacting
                let (prefix, was_truncated) = truncate_utf8_bytes(output, 200);
                let truncated = if was_truncated {
                    format!("{prefix}... (truncated)")
                } else {
                    output.clone()
                };
                ctx_str.push_str(&format!("Output: {}\n", truncated));
            }
        }
        ctx_str.push('\n');

        let ctx_tokens = estimate_tokens(&ctx_str);
        if total_tokens + ctx_tokens > MAX_CONTEXT_TOKENS {
            // If adding this would exceed limit, stop
            break;
        }

        total_tokens += ctx_tokens;
        contexts_to_include.push(ctx_str);
    }

    // Reverse to get chronological order
    contexts_to_include.reverse();

    // Add a note if we had to truncate history
    if contexts_to_include.len() < history.len() {
        context.push_str(&format!(
            "(Note: Showing recent {} of {} total interactions due to length)\n\n",
            contexts_to_include.len(),
            history.len()
        ));
    }

    for ctx_str in contexts_to_include {
        context.push_str(&ctx_str);
    }

    context
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> (&str, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }

    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], true)
}

fn build_prompt(prompt: &str, piped_data: Option<&str>, tool_catalog: &str) -> String {
    if let Some(data) = piped_data {
        let (prefix, was_truncated) = truncate_utf8_bytes(data, MAX_PIPE_BYTES);
        let display_data = if was_truncated {
            format!("{prefix}...\n(input truncated to {} KB)", MAX_PIPE_BYTES / 1024)
        } else {
            data.to_string()
        };
        // No tool catalog in pipe mode: piped data is untrusted context, and
        // combining it with tool suggestions would stack two injection surfaces.
        PIPE_PROMPT_TEMPLATE
            .replace("{piped_data}", &display_data)
            .replace("{query}", prompt)
    } else {
        PROMPT_TEMPLATE
            .replace("{tool_catalog}", tool_catalog)
            .replace("{query}", prompt)
    }
}

/// Send a prompt to the LLM and return the parsed response lines plus the
/// model's safety verdict.
/// This is the core API call logic, separated from UI concerns for testability.
fn query_api(
    prompt: &str,
    model: &str,
    api_key: &str,
    history: &[ConversationContext],
    piped_data: Option<&str>,
) -> Result<LlmReply, Box<dyn std::error::Error>> {
    let mut messages = Vec::new();

    // Add conversation history as context
    if !history.is_empty() {
        let context = compact_history(history);

        messages.push(json!({
            "role": "system",
            "content": context
        }));
    }

    // Build the user prompt – use the pipe-aware template when data was piped
    // in. The saved-tool catalog is re-read on every call so tools created
    // mid-session appear immediately.
    let tool_catalog = if piped_data.is_none() {
        tools::tools_root()
            .map(|root| tools::catalog_block(&root))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let full_prompt = build_prompt(prompt, piped_data, &tool_catalog);

    messages.push(json!({
        "role": "user",
        "content": full_prompt
    }));

    commands_from_api_response(call_llm(messages, model, api_key)?)
}

/// Sends a raw message list to OpenRouter and returns the parsed response.
/// Shared by the command flow and the tool-library generation prompts.
fn call_llm(
    messages: Vec<serde_json::Value>,
    model: &str,
    api_key: &str,
) -> Result<ApiResponse, Box<dyn std::error::Error>> {
    let body = json!({
        "model": model,
        "messages": messages
    });

    // Bound the request so a stalled connection can't hang the tool forever.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build();

    let response = agent
        .post(API_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_json(body);

    match response {
        Ok(resp) => Ok(resp.into_json::<ApiResponse>()?),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_else(|_| String::new());
            Err(format!("API error {code}: {text}").into())
        }
        Err(err) => Err(format!("Network error: {err}").into()),
    }
}

fn commands_from_api_response(
    api_response: ApiResponse,
) -> Result<LlmReply, Box<dyn std::error::Error>> {
    let Some(content) = api_response.first_content() else {
        return Err("No command returned from the model.".into());
    };

    let (verdict, content) = extract_safety_marker(content);
    let commands = parse_commands(&content);

    if commands.is_empty() {
        return Err("No response returned from the model.".into());
    }

    // A response with no executable lines is trivially safe. Otherwise a
    // missing or malformed verdict means we must assume destructive.
    let safe = if commands.iter().all(|line| line.starts_with('#')) {
        true
    } else {
        verdict.unwrap_or(false)
    };

    Ok(LlmReply { commands, safe })
}

/// Extracts the leading `SAFE: yes|no` verdict from the model's response,
/// returning the verdict (None when absent or malformed) and the content
/// with the verdict line removed. Only the first meaningful line counts —
/// a `SAFE:` string later in the response is treated as ordinary content.
fn extract_safety_marker(content: &str) -> (Option<bool>, String) {
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("```") {
            continue;
        }
        // Tolerate models that decorate the verdict (`# SAFE: yes`, `**SAFE: no**`).
        let candidate = trimmed.trim_start_matches(['#', '*']).trim_start();
        let lower = candidate.to_lowercase();
        let verdict = lower.strip_prefix("safe:").and_then(|value| {
            match value.trim().trim_end_matches('*').trim_end() {
                "yes" | "true" => Some(true),
                "no" | "false" => Some(false),
                _ => None,
            }
        });
        return match verdict {
            Some(v) => {
                let rest: Vec<&str> = content
                    .lines()
                    .enumerate()
                    .filter_map(|(i, l)| (i != idx).then_some(l))
                    .collect();
                (Some(v), rest.join("\n"))
            }
            None => (None, content.to_string()),
        };
    }
    (None, content.to_string())
}

fn process_prompt_with_context(
    prompt: &str,
    model: &str,
    api_key: &str,
    theme: &Theme,
    history: &[ConversationContext],
    piped_data: Option<&str>,
    auto_mode: bool,
) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let reply = query_api(prompt, model, api_key, history, piped_data)?;
    // Auto mode never applies when untrusted piped data is in context — its
    // contents could have coaxed the model into a bogus `SAFE: yes`.
    let auto_execute = auto_mode && reply.safe && piped_data.is_none();
    execute_commands_with(reply.commands, theme, auto_execute, confirm, run_line)
}

/// Executes one confirmed line from the model, resolving `$name args` tool
/// invocations to their approved script first. Ordinary lines run unchanged.
fn run_line(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    if let Some((name, args_tail)) = tools::parse_tool_invocation(command) {
        // The approved checksum covers the script, not the arguments — don't
        // let a model-written args tail smuggle extra shell along for the ride.
        if contains_shell_metacharacters(&args_tail) {
            return Err(format!(
                "Refusing to run '${name}' with shell metacharacters in its arguments: {args_tail}"
            )
            .into());
        }
        let root = tools::tools_root().ok_or("HOME is not set; cannot locate ~/.ask/tools.")?;
        let resolved = tools::resolve_approved_tool(&root, &name, &args_tail)?;
        return run_command_with_output(&resolved);
    }
    run_command_with_output(command)
}

// Commands that never run without explicit confirmation, even when the model
// marks its response safe — a backstop against a misjudged or injected
// `SAFE: yes` verdict.
fn never_auto_execute(command: &str) -> bool {
    // LLM-proposed `$tool` invocations always require confirmation — the
    // approved checksum covers the script, not the arguments it runs with.
    if command.trim_start().starts_with('$') {
        return true;
    }
    const DENY: &[&str] = &[
        "rm", "rmdir", "sudo", "dd", "shutdown", "reboot", "halt", "kill", "killall",
    ];
    command
        .split_whitespace()
        .any(|token| DENY.contains(&token) || token.starts_with("mkfs"))
}

fn execute_commands_with<C, E>(
    commands: Vec<String>,
    theme: &Theme,
    auto_execute: bool,
    mut confirm_command: C,
    mut execute_command: E,
) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>>
where
    C: FnMut(&str, &Theme) -> Result<ConfirmResponse, io::Error>,
    E: FnMut(&str) -> Result<String, Box<dyn std::error::Error>>,
{
    let mut executed_commands = Vec::new();
    let mut command_outputs = Vec::new();

    for command in commands {
        if command.starts_with('#') {
            println!(
                "{}\n",
                theme.helper_text(command.trim_start_matches('#').trim())
            );
            continue;
        }

        if auto_execute && !never_auto_execute(&command) {
            println!(
                "{} {} {}",
                theme.prompt_text("run>"),
                theme.command_text(&command),
                theme.helper_text("(auto)")
            );
            let output = execute_command(&command)?;
            executed_commands.push(command.clone());
            command_outputs.push(output);
            continue;
        }

        // Resolve at most one instruct round, then act on the final response.
        let response = match confirm_command(&command, theme)? {
            ConfirmResponse::Instruct(custom_command) => {
                if !custom_command.is_empty() {
                    println!(
                        "Running custom command: {}",
                        theme.command_text(&custom_command)
                    );
                    execute_command(&custom_command)?;
                }
                // After running custom command, continue with the original flow
                println!("\nReturning to original command:");
                match confirm_command(&command, theme)? {
                    ConfirmResponse::Instruct(_) => {
                        // Don't allow nested instruct for simplicity
                        println!("Nested instruct not allowed. Skipping command.");
                        continue;
                    }
                    other => other,
                }
            }
            other => other,
        };

        match response {
            ConfirmResponse::Yes => {
                let output = execute_command(&command)?;
                executed_commands.push(command.clone());
                command_outputs.push(output);
            }
            ConfirmResponse::No => {
                println!("Command execution cancelled");
                return Ok((executed_commands, command_outputs));
            }
            ConfirmResponse::Skip => {
                println!("Skipping command: {}", theme.command_text(&command));
            }
            ConfirmResponse::Instruct(_) => unreachable!("instruct is resolved above"),
        }
    }

    Ok((executed_commands, command_outputs))
}

fn confirm(command: &str, theme: &Theme) -> Result<ConfirmResponse, io::Error> {
    loop {
        print!(
            "{} {}?  [Y/n/s/i]  ",
            theme.prompt_text("run>"),
            theme.command_text(command)
        );
        io::stdout().flush()?;

        let input = read_confirmation_line()?;

        match parse_confirmation_choice(&input) {
            Some(ConfirmChoice::Yes) => return Ok(ConfirmResponse::Yes),
            Some(ConfirmChoice::No) => return Ok(ConfirmResponse::No),
            Some(ConfirmChoice::Skip) => return Ok(ConfirmResponse::Skip),
            Some(ConfirmChoice::Instruct) => {
                print!("{} ", theme.prompt_text("enter>"));
                io::stdout().flush()?;
                let custom_command = read_confirmation_line()?;
                return Ok(ConfirmResponse::Instruct(custom_command.trim().to_string()));
            }
            None => {
                println!("Invalid response. Please use Y(es), n(o), s(kip), or i(nstruct).");
            }
        }
    }
}

fn parse_confirmation_choice(input: &str) -> Option<ConfirmChoice> {
    let trimmed = normalize_confirmation_input(input);

    match trimmed.as_str() {
        "" | "y" | "yes" => Some(ConfirmChoice::Yes),
        "n" | "no" => Some(ConfirmChoice::No),
        "s" | "skip" => Some(ConfirmChoice::Skip),
        "i" | "instruct" => Some(ConfirmChoice::Instruct),
        _ => None,
    }
}

fn read_confirmation_line() -> Result<String, io::Error> {
    let mut input = String::new();

    // Prefer reading from controlling TTY so confirmations still work
    // when stdin is redirected or line editing is active.
    match fs::OpenOptions::new().read(true).open("/dev/tty") {
        Ok(tty) => {
            // Flush any stale input left in the TTY buffer (e.g. from rustyline)
            // so we only read the user's fresh response.
            let fd = tty.as_raw_fd();
            unsafe {
                libc::tcflush(fd, libc::TCIFLUSH);
            }

            // Read byte-by-byte and accept both \r and \n as line terminators.
            // After rustyline restores the terminal, ICRNL may not be set,
            // causing Enter to send \r instead of \n — which read_line() ignores.
            let mut reader = io::BufReader::new(tty);
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if byte[0] == b'\n' || byte[0] == b'\r' {
                            break;
                        }
                        input.push(byte[0] as char);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Err(_) => {
            io::stdin().read_line(&mut input)?;
        }
    }

    Ok(input)
}

fn normalize_confirmation_input(input: &str) -> String {
    let mut cleaned = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            // Strip ANSI escape sequences that can leak into terminal input.
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for seq_char in chars.by_ref() {
                    if ('@'..='~').contains(&seq_char) {
                        break;
                    }
                }
            }
            continue;
        }

        if !ch.is_control() {
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_lowercase()
}

fn run_command_with_output(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let starting_dir = env::current_dir().ok();
    let cwd_capture_path = env::temp_dir().join(format!(
        "ask-cwd-{}-{}",
        std::process::id(),
        NEXT_CWD_CAPTURE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let shell_script = format!(
        "{command}\nask_command_status=$?\npwd -P > \"$ASK_CWD_CAPTURE\"\nexit $ask_command_status"
    );
    let mut child = Command::new(&shell);
    child
        .arg("-c")
        .arg(shell_script)
        .env("ASK_CWD_CAPTURE", &cwd_capture_path);

    let has_terminal =
        io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal();

    let (status, result) = if has_terminal {
        let status = child
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        (status, String::new())
    } else {
        let output = child.output()?;

        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            io::stdout().flush()?;
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            io::stderr().flush()?;
        }

        let mut result = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        (output.status, result)
    };

    if let Ok(cwd) = fs::read_to_string(&cwd_capture_path) {
        let cwd = cwd.trim();
        if !cwd.is_empty() {
            let final_dir = PathBuf::from(cwd);
            if starting_dir.as_ref() != Some(&final_dir) {
                env::set_current_dir(final_dir)?;
            }
        }
    }
    let _ = fs::remove_file(cwd_capture_path);

    if !status.success() {
        return Err(format!("Command exited with status {status}").into());
    }

    Ok(result)
}

fn parse_commands(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("```") && !line.ends_with("```"))
        // Preserve each shell line exactly. Splitting on `&&` breaks quoting,
        // short-circuiting, environment changes, and stateful commands like `cd`.
        .map(str::to_string)
        .collect()
}

struct Args {
    prompt: Option<String>, // None indicates interactive mode
    model: String,
    theme: ThemeMode,
    auto: bool,
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut prompt_parts = Vec::new();
    let mut config = Config::load();
    let mut model = config
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let mut theme = config.theme;
    let mut save_theme = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "--model" => {
                if let Some(value) = args.next() {
                    model = value;
                } else {
                    return Err("--model requires a value".into());
                }
            }
            "--theme" => {
                if let Some(value) = args.next() {
                    theme = ThemeMode::from_str(&value)
                        .ok_or_else(|| "Invalid theme. Use 'light' or 'dark'.".to_string())?;
                    save_theme = true;
                } else {
                    return Err("--theme requires a value".into());
                }
            }
            "--" => {
                prompt_parts.extend(args);
                break;
            }
            _ => prompt_parts.push(arg),
        }
    }

    // If no prompt provided, enter interactive mode
    let prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join(" "))
    };

    if save_theme {
        config.theme = theme;
        if let Err(err) = config.save() {
            eprintln!("Warning: could not save theme preference: {err}");
        }
    }

    Ok(Args {
        prompt,
        model,
        theme,
        auto: config.auto,
    })
}

fn print_help() {
    println!(
        "ask - MacOS command assistant

Usage:
  ask [--model MODEL] [--theme light|dark] <prompt>   # Single prompt mode
  ask [--model MODEL] [--theme light|dark]             # Interactive mode
  command | ask \"prompt\"                                # Pipe mode
  command | ask                                         # Pipe mode (auto-summarize)

Modes:
  Single prompt:    Provide a prompt and get commands to execute
  Interactive:      Enter multiple prompts in a session (type 'exit' or 'quit' to end)
  Pipe:             Pipe data from any command for AI analysis and transformation

Options:
  --model MODEL     Override the default LLM model ({DEFAULT_MODEL})
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show this help message

Environment:
  OPENROUTER_ASK_API_KEY must be set with your OpenRouter API key.

Config:
  Preferences are stored in ~/.ask/config
  (theme=light|dark, model=MODEL, auto=on|off).

The tool sends your prompt to OpenRouter, previews the generated commands,
and asks for confirmation before executing each one in your shell.

Model selection:
  ask model             Show the model in use and where it comes from
  ask model MODEL       Save MODEL as the default (persisted in ~/.ask/config)
  ask model reset       Return to the built-in default ({DEFAULT_MODEL})
  All three also work inside interactive mode; --model overrides for one run.

Auto mode:
  ask auto on / ask auto off (also works inside interactive mode)

  The model labels each response safe or destructive. When auto mode is ON,
  commands labeled safe run immediately without the [Y/n/s/i] prompt.
  Destructive or unlabeled commands, piped-data sessions, and a deny-list
  (rm, sudo, dd, kill, ...) always ask for confirmation. The setting is
  remembered between sessions.

Tool library:
  ask tool new NAME WHAT IT DOES    Have the LLM write a reusable bash/python
                                    script; you review the source, then approve
  ask 'tool improve NAME ...'       Update an existing tool (review + approve)
  ask tool list                     List saved tools
  ask tool show NAME                Print a tool's source and approval status
  ask tool approve NAME             Re-approve after reviewing a changed script
  ask tool rm NAME                  Delete a tool
  ask '$NAME args'                  Run an approved tool (quote the sigil so
                                    your shell doesn't expand $NAME first)

  Tools live in ~/.ask/tools/NAME/. Each approval stores a checksum of the
  script; if the file changes, ask refuses to run it until re-approved.
  All of these also work inside interactive mode (no quoting needed there).

Pipe mode examples:
  git diff | ask \"write a commit message\"
  cat error.log | ask \"what went wrong?\"
  ps aux | ask \"what's using the most memory?\"
  curl -s api.example.com | ask \"extract all emails\"
  docker logs app | ask \"summarize errors\"
  cat data.csv | ask                                   # auto-summarizes

Command confirmation options:
  Y/yes (or Enter)  Execute the command
  n/no              Cancel execution and exit (in interactive mode, returns to prompt)
  s/skip            Skip this command and continue to the next
  i/instruct        Execute a custom command first, then return to the original

Interactive mode commands:
  exit / quit       Exit interactive mode
  clear             Clear screen and reset conversation context
  finder            Open Finder window at current directory
  auto on|off       Toggle auto-execution of model-labeled-safe commands
  auto              Show whether auto mode is on
  model [MODEL]     Show or change the saved LLM model (also: model reset)
  tool ...          Manage the saved tool library (new, improve, list, show,
                    approve, rm)
  $NAME args        Run an approved saved tool"
    );
}

/// Parsed model response: the returned lines plus the model's own verdict on
/// whether every command is non-destructive (consumed by auto mode).
#[derive(Debug)]
struct LlmReply {
    commands: Vec<String>,
    safe: bool,
}

impl std::ops::Deref for LlmReply {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.commands
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    choices: Vec<Choice>,
}

impl ApiResponse {
    fn first_content(&self) -> Option<&str> {
        self.choices
            .first()
            .map(|choice| choice.message.content.trim())
    }
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}

enum ConfirmResponse {
    Yes,
    No,
    Skip,
    Instruct(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmChoice {
    Yes,
    No,
    Skip,
    Instruct,
}

#[derive(Clone)]
struct ConversationContext {
    prompt: String,
    commands: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Clone, Copy)]
enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

struct Theme {
    helper_color: &'static str,
    command_color: &'static str,
    prompt_color: &'static str,
}

const RESET: &str = "\u{001b}[0m";

impl Theme {
    fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                helper_color: "\u{001b}[35m",
                command_color: "\u{001b}[31m",
                prompt_color: "\u{001b}[34m",
            },
            ThemeMode::Dark => Self {
                helper_color: "\u{001b}[36;1m",
                command_color: "\u{001b}[93m",
                prompt_color: "\u{001b}[92m", // bright green - distinct from regular text
            },
        }
    }

    fn helper_text(&self, text: &str) -> String {
        format!("{}{}{}", self.helper_color, text, RESET)
    }

    fn command_text(&self, text: &str) -> String {
        format!("{}{}{}", self.command_color, text, RESET)
    }

    fn prompt_text(&self, text: &str) -> String {
        format!("{}{}{}", self.prompt_color, text, RESET)
    }
}

#[derive(Clone)]
struct Config {
    theme: ThemeMode,
    model: Option<String>,
    auto: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            model: None,
            auto: false,
        }
    }
}

fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

impl Config {
    fn load() -> Self {
        let path = match config_path() {
            Some(path) => path,
            None => return Self::default(),
        };

        let contents = match fs::read_to_string(path).ok() {
            Some(c) => c,
            None => return Self::default(),
        };

        let mut config = Self::default();
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("theme=") {
                if let Some(theme) = ThemeMode::from_str(value.trim()) {
                    config.theme = theme;
                }
            } else if let Some(value) = line.strip_prefix("model=") {
                let value = value.trim();
                if !value.is_empty() {
                    config.model = Some(value.to_string());
                }
            } else if let Some(value) = line.strip_prefix("auto=")
                && let Some(auto) = parse_on_off(value.trim())
            {
                config.auto = auto;
            }
        }

        config
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = match config_path() {
            Some(path) => path,
            None => return Ok(()),
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut contents = format!("theme={}\n", self.theme.as_str());
        if let Some(ref model) = self.model {
            contents.push_str(&format!("model={}\n", model));
        }
        contents.push_str(&format!(
            "auto={}\n",
            if self.auto { "on" } else { "off" }
        ));
        fs::write(path, contents)?;
        Ok(())
    }
}

fn config_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".ask").join("config"))
}

#[cfg(test)]
mod tests;

/// Integration tests that make real API calls to the configured LLM.
/// Run with: cargo test -- --ignored --show-output
#[cfg(test)]
mod integration_tests;
