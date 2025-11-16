use serde::Deserialize;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, exit};

const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";
const PROMPT_TEMPLATE: &str = r#"
You are a command-line assistant specialized in MacOS Zsh scripting. Your task is to generate correct and optimized MacOS terminal commands based on user requests.

**Instructions:**
- Always return **only the command**, unless explicitly asked to explain.
- Use **safe practices** (avoid dangerous commands like `rm -rf /`).
- Assume the user is using **MacOS** **Zsh** unless they specify otherwise.
- If multiple commands are needed, return them in sequence.
- Do not use any code blocks (```) in your response.
- If you need to explain something, do so **before** the command, not after, and prefix it with `# `.

**Example:**
User request: 
  How do I kill a process running on port 5234?
You would respond with:
  lsof -i :5234
  kill $(lsof -t -i :5234)

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

    let full_prompt = PROMPT_TEMPLATE.replace("{query}", &args.prompt);

    let body = serde_json::json!({
        "model": args.model,
        "messages": [
            {
                "role": "user",
                "content": full_prompt
            }
        ]
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
        return Err("No command returned from the model.".into());
    }

    for command in commands {
        if command.starts_with('#') {
            println!(
                "{}\n",
                theme.helper_text(command.trim_start_matches('#').trim())
            );
            continue;
        }

        if !confirm(&command, &theme)? {
            println!("Command execution cancelled");
            return Ok(());
        }

        run_command(&command)?;
    }

    Ok(())
}

fn confirm(command: &str, theme: &Theme) -> Result<bool, io::Error> {
    print!(
        "{}  {}?  [Y/n]  ",
        theme.prompt_text("Run command:"),
        theme.command_text(command)
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.contains('\u{1b}') {
        return Ok(false);
    }

    let trimmed = input.trim().to_lowercase();
    Ok(trimmed.is_empty() || trimmed == "y" || trimmed == "yes")
}

fn run_command(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let status = Command::new(&shell).arg("-c").arg(command).status()?;

    if !status.success() {
        return Err(format!("Command exited with status {status}").into());
    }
    Ok(())
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
    prompt: String,
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

    if prompt_parts.is_empty() {
        return Err("Usage: ask [--model MODEL] [--theme light|dark] <prompt>".into());
    }

    if save_theme {
        config.theme = theme;
        if let Err(err) = config.save() {
            eprintln!("Warning: could not save theme preference: {err}");
        }
    }

    Ok(Args {
        prompt: prompt_parts.join(" "),
        model,
        theme,
    })
}

fn print_help() {
    println!(
        "ask - MacOS command assistant

Usage:
  ask [--model MODEL] [--theme light|dark] <prompt>

Options:
  --model MODEL     Override the default LLM model ({DEFAULT_MODEL})
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show this help message

Environment:
  OPENROUTER_ASK_API_KEY must be set with your OpenRouter API key.

Config:
  Default theme preference is stored in ~/.ask/config (theme=light|dark).

The tool sends your prompt to OpenRouter, previews the generated commands,
and asks for confirmation before executing each one in your shell."
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
                prompt_color: "\u{001b}[97m",
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
