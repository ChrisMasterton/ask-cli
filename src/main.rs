use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, exit};

const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";
// Token limits - most models support 4K-128K, we'll be conservative
const MAX_CONTEXT_TOKENS: usize = 3000;  // Reserve ~1000 for response
const TOKEN_ESTIMATE_RATIO: usize = 4;   // Roughly 1 token per 4 characters
const PROMPT_TEMPLATE: &str = r#"
You are a command-line assistant specialized in MacOS Zsh scripting, helping users both with commands and general assistance.

**Instructions:**
- Analyze if the user is requesting an action/command or making a statement/asking a question
- For ACTION REQUESTS: Generate the appropriate terminal commands
  - Return **only the command**, unless explicitly asked to explain
  - Use **safe practices** (avoid dangerous commands like `rm -rf /`)
  - If multiple commands are needed, return them in sequence
  - Explanations go **before** commands, prefixed with `# `
- For STATEMENTS/QUESTIONS: Respond conversationally
  - Prefix your entire response with `# ` to indicate it's not a command
  - Be helpful, concise, and friendly
  - If discussing the tool itself, acknowledge its capabilities
- Assume the user is using **MacOS** **Zsh** unless they specify otherwise
- Do not use any code blocks (```) in your response

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

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let theme = Theme::from_mode(args.theme);

    let api_key = env::var("OPENROUTER_ASK_API_KEY")
        .map_err(|_| "Please set the OPENROUTER_ASK_API_KEY environment variable.")?;

    match args.prompt {
        Some(prompt) => {
            // Single prompt mode
            process_prompt(&prompt, &args.model, &api_key, &theme)?;
        }
        None => {
            // Interactive mode
            run_interactive_mode(&args.model, &api_key, &theme)?;
        }
    }

    Ok(())
}

// Check if the input looks like a script file to run
fn is_script_execution(cmd: &str) -> bool {
    let cmd = cmd.trim();

    // Check for explicit script interpreters
    if cmd.starts_with("python ") || cmd.starts_with("python3 ") ||
       cmd.starts_with("node ") || cmd.starts_with("ruby ") ||
       cmd.starts_with("perl ") || cmd.starts_with("php ") ||
       cmd.starts_with("bash ") || cmd.starts_with("sh ") ||
       cmd.starts_with("zsh ") || cmd.starts_with("./") {
        return true;
    }

    // Check if it's a script file by extension
    if let Some(extension) = cmd.split('.').last() {
        matches!(extension,
            "sh" | "bash" | "zsh" |
            "py" | "python" |
            "js" | "mjs" | "ts" |
            "rb" | "ruby" |
            "pl" | "perl" |
            "php" |
            "r" | "R" |
            "go" | "rs" |
            "java" | "class" |
            "swift" | "kt"
        )
    } else {
        false
    }
}

// Safe commands that can be executed directly without LLM confirmation
fn is_safe_direct_command(cmd: &str) -> bool {
    // Check if it's a script first
    if is_script_execution(cmd) {
        return true;
    }

    let safe_commands = [
        // File listing and navigation
        "ls", "ll", "la", "dir", "pwd", "tree",
        // File reading (non-destructive)
        "cat", "head", "tail", "less", "more", "wc", "file", "stat",
        // System information
        "date", "uptime", "whoami", "hostname", "uname", "id",
        "df", "du", "free", "top", "ps", "who", "w",
        // Network information (read-only)
        "ifconfig", "ping", "netstat", "curl", "wget", "dig", "nslookup",
        // Environment
        "env", "printenv", "echo", "which", "type", "alias",
        // Git read operations
        "git status", "git log", "git diff", "git branch", "git remote",
        // Package managers (list only)
        "brew list", "npm list", "pip list", "cargo search",
        // History and help
        "history", "help", "man",
    ];

    // Check if the command starts with any safe command
    let cmd_lower = cmd.trim().to_lowercase();

    // Special handling for commands with arguments
    if cmd_lower.starts_with("ls ") || cmd_lower == "ls" { return true; }
    if cmd_lower.starts_with("cd ") || cmd_lower == "cd" { return true; }
    if cmd_lower.starts_with("cat ") || cmd_lower == "cat" { return true; }
    if cmd_lower.starts_with("echo ") || cmd_lower == "echo" { return true; }
    if cmd_lower.starts_with("pwd") { return true; }
    if cmd_lower.starts_with("head ") || cmd_lower == "head" { return true; }
    if cmd_lower.starts_with("tail ") || cmd_lower == "tail" { return true; }
    if cmd_lower.starts_with("grep ") || cmd_lower == "grep" { return true; }
    if cmd_lower.starts_with("find ") || cmd_lower == "find" { return true; }
    if cmd_lower.starts_with("wc ") || cmd_lower == "wc" { return true; }
    if cmd_lower.starts_with("diff ") || cmd_lower == "diff" { return true; }

    // Check exact matches for commands without arguments
    safe_commands.iter().any(|&cmd_str| cmd_lower == cmd_str)
}

fn run_interactive_mode(model: &str, api_key: &str, theme: &Theme) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", theme.prompt_text("Interactive mode. Commands: 'exit', 'clear', 'finder'"));
    println!("{}", theme.helper_text("Common commands and scripts execute directly without confirmation"));
    println!("{}", theme.helper_text("Shortcuts: q=quit, .=pwd, ..=cd .."));

    // Show current directory on start
    if let Ok(cwd) = env::current_dir() {
        println!("{}", theme.helper_text(&format!("📁 {}", cwd.display())));
    }
    println!();

    let mut rl = DefaultEditor::new()?;
    let mut history: Vec<ConversationContext> = Vec::new();

    loop {
        // Get current directory for prompt - show folder name or ~ for home
        let cwd_display = if let Ok(cwd) = env::current_dir() {
            if let Ok(home) = env::var("HOME") {
                if cwd.to_string_lossy() == home {
                    "~".to_string()
                } else if let Some(relative) = cwd.to_string_lossy().strip_prefix(&format!("{}/", home)) {
                    format!("~/{}", relative.split('/').last().unwrap_or(relative))
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
            println!("{} {}", theme.prompt_text("run>"), theme.command_text("pwd"));
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
            println!("{} {}", theme.prompt_text("run>"), theme.command_text("cd .."));
            match env::set_current_dir("..") {
                Ok(_) => {
                    let cwd = env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "unknown".to_string());
                    println!("{}", theme.helper_text(&format!("Changed directory to: {}", cwd)));

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
            println!("{}", theme.prompt_text("Interactive mode. Commands: 'exit', 'clear', 'finder'"));
            println!("{}", theme.helper_text("Common commands and scripts execute directly without confirmation"));
            println!("{}", theme.helper_text("Shortcuts: q=quit, .=pwd, ..=cd .."));

            // Show current directory after clear
            if let Ok(cwd) = env::current_dir() {
                println!("{}", theme.helper_text(&format!("📁 {}", cwd.display())));
            }
            println!();
            continue;
        }

        if input == "finder" {
            // Open Finder at current directory
            match Command::new("open").arg(".").status() {
                Ok(_) => println!("{}", theme.helper_text("Opened Finder at current directory")),
                Err(e) => eprintln!("Failed to open Finder: {}", e),
            }
            continue;
        }

        // Check if it's a safe direct command
        if is_safe_direct_command(input) {
            // Determine the actual command to run
            let command_to_run = if input.trim() == "ls" {
                // Special handling for plain 'ls' - convert to 'ls -l' for better info
                "ls -l".to_string()
            } else if is_script_execution(input) && !input.contains(" ") {
                // If it's just a script name without interpreter, add appropriate interpreter
                let script = input.trim();
                if script.ends_with(".py") {
                    format!("python3 {}", script)
                } else if script.ends_with(".js") || script.ends_with(".mjs") {
                    format!("node {}", script)
                } else if script.ends_with(".rb") {
                    format!("ruby {}", script)
                } else if script.ends_with(".sh") || script.ends_with(".bash") {
                    format!("bash {}", script)
                } else if script.ends_with(".pl") {
                    format!("perl {}", script)
                } else if script.ends_with(".php") {
                    format!("php {}", script)
                } else {
                    input.to_string()
                }
            } else {
                input.to_string()
            };

            println!("{} {}", theme.prompt_text("run>"), theme.command_text(&command_to_run));

            // Special handling for cd command
            if input.trim().starts_with("cd") {
                let path = if input.trim() == "cd" {
                    env::var("HOME").unwrap_or_else(|_| "/".to_string())
                } else {
                    input.trim().strip_prefix("cd ").unwrap_or("").trim().to_string()
                };

                match env::set_current_dir(&path) {
                    Ok(_) => {
                        let cwd = env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| "unknown".to_string());
                        println!("{}", theme.helper_text(&format!("Changed directory to: {}", cwd)));

                        // Add to history
                        history.push(ConversationContext {
                            prompt: input.to_string(),
                            commands: vec![input.to_string()],
                            outputs: vec![format!("Changed to: {}", cwd)],
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to change directory: {}", e);
                    }
                }
            } else {
                // Execute other safe commands (including scripts)
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
            }

            // Check context size after adding
            let estimated_total = estimate_total_context_size(&history);
            if estimated_total > MAX_CONTEXT_TOKENS * TOKEN_ESTIMATE_RATIO {
                println!(
                    "{}",
                    theme.helper_text("Note: Context is being automatically compacted to fit within token limits.")
                );
            }

            continue;
        }

        match process_prompt_with_context(input, model, api_key, theme, &history) {
            Ok((commands, outputs)) => {
                // Add to history
                history.push(ConversationContext {
                    prompt: input.to_string(),
                    commands: commands.clone(),
                    outputs,
                });

                // Check if we should display a warning about context size
                let estimated_total = estimate_total_context_size(&history);
                if estimated_total > MAX_CONTEXT_TOKENS * TOKEN_ESTIMATE_RATIO {
                    println!(
                        "{}",
                        theme.helper_text("Note: Context is being automatically compacted to fit within token limits.")
                    );
                }
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
) -> Result<(), Box<dyn std::error::Error>> {
    process_prompt_with_context(prompt, model, api_key, theme, &[])?;
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / TOKEN_ESTIMATE_RATIO
}

fn estimate_total_context_size(history: &[ConversationContext]) -> usize {
    let mut total = 0;
    for ctx in history {
        total += ctx.prompt.len();
        for cmd in &ctx.commands {
            total += cmd.len();
        }
        for output in &ctx.outputs {
            total += output.len().min(500); // Count truncated size
        }
    }
    total
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
                let truncated = if output.len() > 200 {
                    format!("{}... (truncated)", &output[..200])
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
        context.push_str(&format!("(Note: Showing recent {} of {} total interactions due to length)\n\n",
                                  contexts_to_include.len(), history.len()));
    }

    for ctx_str in contexts_to_include {
        context.push_str(&ctx_str);
    }

    context
}

fn process_prompt_with_context(
    prompt: &str,
    model: &str,
    api_key: &str,
    theme: &Theme,
    history: &[ConversationContext],
) -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let mut messages = Vec::new();

    // Add conversation history as context
    if !history.is_empty() {
        let context = compact_history(history);

        messages.push(json!({
            "role": "system",
            "content": context
        }));
    }

    // Add the main prompt
    let full_prompt = PROMPT_TEMPLATE.replace("{query}", prompt);
    messages.push(json!({
        "role": "user",
        "content": full_prompt
    }));

    let body = json!({
        "model": model,
        "messages": messages
    });

    let response = ureq::post(API_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_json(body);

    let api_response = match response {
        Ok(resp) => resp.into_json::<ApiResponse>()?,
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_else(|_| String::new());
            return Err(format!("API error {code}: {text}").into());
        }
        Err(err) => return Err(format!("Network error: {err}").into()),
    };

    let Some(content) = api_response
        .choices
        .first()
        .map(|choice| choice.message.content.trim())
    else {
        return Err("No command returned from the model.".into());
    };

    let commands = parse_commands(content);

    if commands.is_empty() {
        return Err("No response returned from the model.".into());
    }

    // Check if all lines are conversational (start with #)
    let all_conversational = commands.iter().all(|cmd| cmd.starts_with('#'));

    let mut executed_commands = Vec::new();
    let mut command_outputs = Vec::new();

    // If it's purely conversational, we still want to track it in history
    if all_conversational {
        for command in commands {
            println!(
                "{}\n",
                theme.helper_text(command.trim_start_matches('#').trim())
            );
        }
        // Return empty commands but indicate success for conversation tracking
        return Ok((Vec::new(), Vec::new()));
    }

    for command in commands {
        if command.starts_with('#') {
            println!(
                "{}\n",
                theme.helper_text(command.trim_start_matches('#').trim())
            );
            continue;
        }

        match confirm(&command, &theme)? {
            ConfirmResponse::Yes => {
                executed_commands.push(command.clone());
                let output = run_command_with_output(&command)?;
                command_outputs.push(output);
            }
            ConfirmResponse::No => {
                println!("Command execution cancelled");
                return Ok((executed_commands, command_outputs));
            }
            ConfirmResponse::Skip => {
                println!("Skipping command: {}", theme.command_text(&command));
                continue;
            }
            ConfirmResponse::Instruct(custom_command) => {
                if !custom_command.is_empty() {
                    println!("Running custom command: {}", theme.command_text(&custom_command));
                    run_command_with_output(&custom_command)?;
                }
                // After running custom command, continue with the original flow
                println!("\nReturning to original command:");
                match confirm(&command, &theme)? {
                    ConfirmResponse::Yes => {
                        executed_commands.push(command.clone());
                        let output = run_command_with_output(&command)?;
                        command_outputs.push(output);
                    }
                    ConfirmResponse::No => {
                        println!("Command execution cancelled");
                        return Ok((executed_commands, command_outputs));
                    }
                    ConfirmResponse::Skip => {
                        println!("Skipping command: {}", theme.command_text(&command));
                        continue;
                    }
                    ConfirmResponse::Instruct(_) => {
                        // Don't allow nested instruct for simplicity
                        println!("Nested instruct not allowed. Skipping command.");
                        continue;
                    }
                }
            }
        }
    }

    Ok((executed_commands, command_outputs))
}

fn confirm(command: &str, theme: &Theme) -> Result<ConfirmResponse, io::Error> {
    print!(
        "{} {}?  [Y/n/s/i]  ",
        theme.prompt_text("run>"),
        theme.command_text(command)
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.contains('\u{1b}') {
        return Ok(ConfirmResponse::No);
    }

    let trimmed = input.trim().to_lowercase();

    match trimmed.as_str() {
        "" | "y" | "yes" => Ok(ConfirmResponse::Yes),
        "n" | "no" => Ok(ConfirmResponse::No),
        "s" | "skip" => Ok(ConfirmResponse::Skip),
        "i" | "instruct" => {
            // Get custom command from user
            print!("{} ", theme.prompt_text("enter>"));
            io::stdout().flush()?;
            let mut custom_command = String::new();
            io::stdin().read_line(&mut custom_command)?;
            Ok(ConfirmResponse::Instruct(custom_command.trim().to_string()))
        }
        _ => {
            println!("Invalid response. Please use Y(es), n(o), s(kip), or i(nstruct).");
            confirm(command, theme)
        }
    }
}

fn run_command_with_output(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = Command::new(&shell)
        .arg("-c")
        .arg(command)
        .output()?;

    // Print the output to the console as it would normally appear
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        io::stdout().flush()?;
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        io::stderr().flush()?;
    }

    if !output.status.success() {
        return Err(format!("Command exited with status {}", output.status).into());
    }

    // Return the combined output for history
    let mut result = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        result.push_str("\n");
        result.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(result)
}

fn parse_commands(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("```") && !line.ends_with("```"))
        .map(|line| line.to_string())
        .collect()
}

struct Args {
    prompt: Option<String>,  // None indicates interactive mode
    model: String,
    theme: ThemeMode,
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut prompt_parts = Vec::new();
    let mut model = DEFAULT_MODEL.to_string();
    let mut config = Config::load();
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
    })
}

fn print_help() {
    println!(
        "ask - MacOS command assistant

Usage:
  ask [--model MODEL] [--theme light|dark] <prompt>   # Single prompt mode
  ask [--model MODEL] [--theme light|dark]             # Interactive mode

Modes:
  Single prompt:    Provide a prompt and get commands to execute
  Interactive:      Enter multiple prompts in a session (type 'exit' or 'quit' to end)

Options:
  --model MODEL     Override the default LLM model ({DEFAULT_MODEL})
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show this help message

Environment:
  OPENROUTER_ASK_API_KEY must be set with your OpenRouter API key.

Config:
  Default theme preference is stored in ~/.ask/config (theme=light|dark).

The tool sends your prompt to OpenRouter, previews the generated commands,
and asks for confirmation before executing each one in your shell.

Command confirmation options:
  Y/yes (or Enter)  Execute the command
  n/no              Cancel execution and exit (in interactive mode, returns to prompt)
  s/skip            Skip this command and continue to the next
  i/instruct        Execute a custom command first, then return to the original

Interactive mode commands:
  exit / quit       Exit interactive mode
  clear             Clear screen and reset conversation context
  finder            Open Finder window at current directory"
    );
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
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

enum ConfirmResponse {
    Yes,
    No,
    Skip,
    Instruct(String),
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

#[derive(Clone, Copy)]
struct Config {
    theme: ThemeMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
        }
    }
}

impl Config {
    fn load() -> Self {
        let path = match config_path() {
            Some(path) => path,
            None => return Self::default(),
        };

        let contents = fs::read_to_string(path).ok();
        if let Some(contents) = contents {
            for line in contents.lines() {
                if let Some(value) = line.strip_prefix("theme=") {
                    if let Some(theme) = ThemeMode::from_str(value.trim()) {
                        return Self { theme };
                    }
                }
            }
        }

        Self::default()
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = match config_path() {
            Some(path) => path,
            None => return Ok(()),
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let contents = format!("theme={}\n", self.theme.as_str());
        fs::write(path, contents)?;
        Ok(())
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ask").join("config"))
}
