# ask - AI-Powered MacOS Command Assistant

A command-line tool that converts natural language prompts into MacOS terminal commands using AI. Built with Rust for speed and reliability.

## See It in Action

Stop googling shell syntax — just say what you want:

```bash
$ ask "kill whatever is using port 3000"
run> kill $(lsof -t -i :3000)?  [Y/n]

$ ask "what's eating my disk space?"
run> du -sh * | sort -rh | head -10?  [Y/n]

$ ask "find every file over 500MB in my home folder"
run> find ~ -type f -size +500M 2>/dev/null?  [Y/n]

$ ask "undo my last commit but keep the changes"
run> git reset --soft HEAD~1?  [Y/n]
```

It knows the Mac-only tools you can never remember:

```bash
$ ask "convert all these HEIC photos to jpg"
run> for f in *.heic; do sips -s format jpeg "$f" --out "${f%.heic}.jpg"; done?  [Y/n]

$ ask "keep my mac awake for the next 2 hours"
run> caffeinate -d -t 7200?  [Y/n]

$ ask "what's my local IP?"
run> ipconfig getifaddr en0?  [Y/n]

$ ask "flush the DNS cache"
run> sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder?  [Y/n]
```

Pipe anything into it — logs, diffs, JSON, clipboard contents:

```bash
git diff | ask "write a commit message"
docker logs api 2>&1 | ask "why does it keep restarting?"
pbpaste | ask "pretty-print this JSON"
cat access.log | ask "top 10 IPs by request count"
ps aux | ask "what's using the most memory?"
```

And interactive mode keeps context between prompts, so follow-ups just work:

```
ask [Downloads]> find dmg files older than a month
run> find . -name "*.dmg" -mtime +30?  [Y/n]  y
./OldInstaller.dmg
./Slack-4.35.dmg

ask [Downloads]> now delete them
run> find . -name "*.dmg" -mtime +30 -delete?  [Y/n]
```

Once you trust it, turn on auto mode — commands the AI labels as safe run
instantly, and only destructive ones stop to ask:

```bash
$ ask auto on
Auto mode ON — commands the model marks as safe run without confirmation

$ ask "how many lines of rust in this project?"
run> find . -name "*.rs" -not -path "./target/*" | xargs wc -l (auto)
    2622 total

$ ask "delete the build cache"
run> rm -rf target?  [Y/n]        # destructive → still asks
```

When a one-liner isn't enough, have it write you a reusable tool — reviewed
once, saved forever, run again by name:

```bash
$ ask tool new ports "list processes listening on tcp ports with pid and command"
--- ports (bash) ---
#!/bin/bash
lsof -iTCP -sTCP:LISTEN -P -n | awk 'NR>1 {print $2, $1, $9}' | sort -u
---
Save and approve this script? [y/N] y
Saved ~/.ask/tools/ports/main.sh — run it with: $ports <args>

$ ask '$ports'
run> bash '/Users/you/.ask/tools/ports/main.sh'
515 rapportd *:50761
7723 node *:3000
```

Generated commands vary by model — but you always see the exact command, and anything destructive is confirmed before it runs.

## Quick Start

```bash
# Set your API key
export OPENROUTER_ASK_API_KEY="your-key"

# Single command
ask "show all python files"

# Pipe any command through ask for AI analysis
cat error.log | ask "what went wrong?"

# Interactive mode
ask
> ls              # Instant execution
> find large files # AI generates command
> q               # Quick exit
```

## Overview

`ask` is a CLI assistant that uses OpenRouter's API to generate MacOS Zsh commands from plain English descriptions. It provides an interactive workflow where generated commands are presented for confirmation before execution, ensuring safety and transparency. The new interactive mode transforms it into an AI-enhanced shell with context awareness and intelligent command generation.

## Features

### Core Features
- **Natural Language to Commands**: Describe what you want to do, get the exact shell commands
- **Interactive Confirmation**: Review and approve commands before they run with multiple options
- **Auto Mode**: Optionally let AI-labeled-safe commands run without confirmation (`ask auto on`)
- **Safe by Design**: Built-in safeguards against dangerous operations
- **Theme Support**: Light and dark color themes for terminal readability
- **Model Selection**: Choose from various LLM models via OpenRouter
- **Persistent Configuration**: Saves theme, model, and auto-mode preferences locally
- **MacOS & Zsh Optimized**: Tailored for MacOS terminal environment

### Interactive Mode (New!)
- **Persistent Session**: Run multiple prompts without restarting
- **Context Awareness**: Maintains conversation history with smart token management
- **Direct Command Execution**: Common commands (ls, pwd, cat, etc.) run instantly
- **Shortcuts**: Quick commands like `q` (quit), `.` (pwd), `..` (cd ..)
- **Finder Integration**: Type `finder` to open current directory in Finder
- **Directory Display**: Current folder shown in prompt for constant awareness

### Pipe Mode (New!)
- **Universal Data Interpreter**: Pipe output from any command for AI analysis
- **Auto-detection**: Automatically detects piped input — no flags needed
- **Auto-summarize**: Pipe data without a prompt and get an instant summary
- **Composable**: Works with every Unix tool — `grep`, `curl`, `docker`, `git`, etc.

### Tool Library (New!)
- **AI-Written Tools**: `tool new <name> <description>` has the LLM write a reusable bash/python script
- **Review Before Save**: The full source is shown and needs your explicit approval
- **Checksum-Gated**: Any change to an approved script blocks execution until re-reviewed
- **Run by Name**: `$name args` in interactive mode, `ask '$name args'` from the shell
- **Improve, Don't Regenerate**: `tool improve <name> <instructions>` updates the existing script

### Command Execution Options
- **Skip (s)**: Skip one command of a multi-command response and continue to the next
- **Conversational Responses**: AI can respond without generating commands
- **Auto Mode**: `auto on` lets commands the AI labels as safe run without
  confirmation — destructive ones still ask (persists across sessions)

## Installation

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- OpenRouter API key (get one at [openrouter.ai](https://openrouter.ai))

### Build from Source

```bash
# Clone the repository
git clone <repository-url>
cd ask-cli

# Build release binary
cargo build --release

# The binary will be at target/release/ask
# Optionally, move it to your PATH
sudo cp target/release/ask /usr/local/bin/
```

## Configuration

### Environment Variable

Set your OpenRouter API key:

```bash
export OPENROUTER_ASK_API_KEY="your-api-key-here"
```

Add this to your `~/.zshrc` to make it permanent:

```bash
echo 'export OPENROUTER_ASK_API_KEY="your-api-key-here"' >> ~/.zshrc
source ~/.zshrc
```

### Theme Configuration

Theme preferences are automatically saved to `~/.ask/config`:

```bash
# Set your preferred theme (light or dark)
ask --theme dark "example prompt"
```

## Usage

### Basic Usage

```bash
# Single prompt mode
ask <your natural language prompt>

# Interactive mode (new!)
ask
```

### Interactive Mode

Start an interactive session by running `ask` without arguments:

```bash
$ ask
Interactive mode. Commands: 'exit', 'clear', 'finder', 'auto on|off'
Common commands and scripts execute directly without confirmation
Shortcuts: q=quit, .=pwd, ..=cd ..
📁 /Users/chris/Projects

ask [Projects]> ls
run> ls -l
total 48
drwxr-xr-x  12 chris  staff   384 Jan 15 10:30 ask-cli
drwxr-xr-x   8 chris  staff   256 Jan 14 09:15 other-project

ask [Projects]> cd ask-cli
run> cd ask-cli

ask [ask-cli]> create a readme file
run> touch README.md? [Y/n] y

ask [ask-cli]> q
Goodbye!
```

### Pipe Mode

Pipe the output of any command into `ask` for AI-powered analysis:

```bash
# Analyze logs
cat /var/log/system.log | ask "summarize recent errors"
docker logs myapp | ask "what's causing the crashes?"

# Code review and git
git diff | ask "write a commit message for this"
git log --oneline -20 | ask "summarize recent activity"

# Data analysis
cat data.csv | ask "find outliers and anomalies"
curl -s https://api.example.com/users | ask "extract all email addresses"

# System diagnostics
ps aux | ask "what's using the most memory?"
df -h | ask "which volumes are running low on space?"

# Auto-summarize (no prompt needed)
cat README.md | ask
```

Pipe mode is automatically detected — no flags required. Data up to 64 KB is sent as context to the LLM alongside your prompt.

### Examples

```bash
# Find and kill a process on a specific port
ask "kill the process running on port 8080"

# File operations
ask "find all python files modified in the last week"

# System information
ask "show me disk usage for each directory"

# Git operations
ask "create a new branch called feature-xyz and switch to it"

# Network operations
ask "show all active network connections"
```

### Command-Line Options

```bash
ask [OPTIONS] [prompt]
command | ask [OPTIONS] [prompt]

Options:
  --model MODEL     Override the LLM model (default: meta-llama/llama-3.3-70b-instruct)
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show help message

Modes:
  With prompt:          Single command execution mode
  Without prompt:       Interactive mode with persistent session
  With piped input:     Pipe mode — AI analyses the piped data with your prompt

Preferences (persisted; no API key required):
  ask auto on|off       Enable/disable auto-execution of AI-labeled-safe commands
  ask model [MODEL]     Show or save the default LLM model
  ask model reset       Return to the built-in default model

Tool library (only new/improve need the API key):
  ask tool new NAME DESCRIPTION     LLM writes a reusable script; review + approve
  ask 'tool improve NAME ...'       Update an existing tool; review + approve
  ask tool list|show|approve|rm     Manage saved tools
  ask '$NAME args'                  Run an approved tool
```

### Command Confirmation Options

When a command is presented for confirmation:

```
run> command?  [Y/n]

Y/yes (Enter)     Execute the command
n/no              Cancel and exit (or return to prompt in interactive mode)
```

When a response contains several commands, the prompt becomes `[Y/n/s]` —
`s`/`skip` passes over the current command and continues to the next, while
`n` cancels the rest of the sequence.

### Using Custom Models

```bash
# One run only (not saved)
ask --model anthropic/claude-haiku-4.5 "your prompt here"

# Save as the default — persists in ~/.ask/config
ask model anthropic/claude-haiku-4.5

# Show which model is active and where it comes from
ask model

# Return to the built-in default
ask model reset
```

The `model` command also works inside interactive mode and switches the
running session immediately.

## Tool Library

`ask` can save the scripts it writes so they never need regenerating. Each
tool lives in `~/.ask/tools/<name>/` — a `main.sh` or `main.py` (bash and
Python 3 stdlib only, no package managers) plus a small manifest with its
description and an approval checksum.

```bash
# Create: the LLM writes the script, you review the source, approve saves it
ask tool new dedupe "find duplicate files by content hash in a directory"

# Run it any time — no API call, works offline (quote the sigil in your shell)
ask '$dedupe ~/Downloads'

# Improve it later: the current source + your instructions go to the LLM,
# you review the new version, approving overwrites and re-checksums
ask tool improve dedupe "add a --delete flag that keeps the newest copy"

# Manage the library
ask tool list             # every tool with lang, description, approval state
ask tool show dedupe      # print the source and status
ask tool approve dedupe   # re-approve after reviewing a hand-edited script
ask tool rm dedupe        # delete it
```

In interactive mode the same commands work without quoting: `$dedupe ~/Downloads`,
`tool list`, and so on. Approved tools are also advertised to the LLM, so a
prompt like "clean up my downloads" may come back proposing `$dedupe ~/Downloads`
— such lines always ask for confirmation, even in auto mode.

Safety model: you approve a script's exact source, and its SHA-256 (covering
the interpreter too) is stored in the manifest. If the file on disk changes in
any way, `ask` refuses to run it until you review and `tool approve` it again.
Tool names are restricted to `a-z 0-9 - _`, so they can't escape the tools
directory.

## Interactive Mode Features

### Direct Commands
These commands execute immediately without LLM processing or confirmation:

- **Navigation**: `ls`, `pwd`, `cd`, `tree`
- **File Reading**: `cat`, `head`, `tail`, `grep`, `find`, `diff`
- **System Info**: `date`, `whoami`, `hostname`, `df`, `ps`
- **Git Status**: `git status`, `git log`, `git diff`, `git branch`
- **Environment**: `echo`, `env`, `which`, `type`

Note: Plain `ls` automatically executes as `ls -l` for better file information.

Direct execution is conservative: any line containing shell metacharacters
(`;`, `&`, `|`, `` ` ``, `$`, parentheses, or redirection) or destructive
`find` flags (`-delete`, `-exec`) always goes through AI review and
confirmation instead, so nothing can chain onto a whitelisted command.

### Shortcuts

| Shortcut | Action | Description |
|----------|--------|-------------|
| `q` | Quit | Exit interactive mode |
| `.` | `pwd` | Show current directory |
| `..` | `cd ..` | Go up one directory |
| `finder` | Open Finder | Open current directory in Finder |
| `clear` | Clear & Reset | Clear screen and reset context |
| `auto on` / `auto off` | Toggle auto mode | Run AI-labeled-safe commands without confirmation |
| `auto` | Show auto state | Report whether auto mode is on |
| `model [MODEL]` | Show / switch model | Change the LLM for the session and save as default (`model reset` for built-in) |
| `tool ...` | Tool library | Create, improve, list, show, approve, or remove saved tools |
| `$NAME args` | Run a tool | Execute an approved saved tool by name |

### Auto Mode

Every AI response starts with a safety verdict (`SAFE: yes` / `SAFE: no`)
judging whether its commands are read-only or destructive. With auto mode on,
commands the model marks safe run immediately — no `[Y/n]` prompt:

```
ask [Projects]> how big is this folder?
run> du -sh . (auto)
1.2G    .
```

Toggle it with `auto on` / `auto off` in interactive mode, or from the shell:

```bash
ask auto on
ask auto off
```

The setting is saved to `~/.ask/config` and remembered across sessions.

Auto mode is deliberately conservative. Confirmation is still required when:

- the model marks the response destructive (`SAFE: no`)
- the response has no verdict at all (treated as destructive)
- the command contains `rm`, `sudo`, `dd`, `kill`, or similar — a built-in
  deny-list that overrides even a `SAFE: yes` verdict
- data was piped in (piped content could trick the model into a false verdict)

### Context Management

Interactive mode maintains conversation history:
- Previous commands and outputs are sent as context to the LLM
- Context is automatically compacted when approaching token limits
- Use `clear` to reset context and start fresh
- The LLM can reference previous commands and their outputs

### Conversational AI

The AI can now respond conversationally without always generating commands:

```bash
ask [Projects]> this is a great tool!
# Thank you! I'm glad you're finding it helpful. Feel free to ask for any commands or help.

ask [Projects]> what did we just do?
# We just listed the files in the Projects directory, showing two subdirectories...
```

## How It Works

1. **Prompt Processing**: Your natural language request is sent to OpenRouter's API
2. **Command Generation**: The AI model generates appropriate MacOS Zsh commands
3. **Safety Verdict**: The model labels its own response `SAFE: yes` (read-only) or `SAFE: no` (destructive)
4. **Interactive Review**: Generated commands are displayed with syntax highlighting
5. **User Confirmation**: You approve or reject each command before execution — or, with auto mode on, safe-labeled commands run immediately
6. **Safe Execution**: Approved commands run in your default shell

## Safety Features

- Commands are always shown before execution
- Simple confirmation prompt ([Y/n], plus skip for multi-command responses)
  - Return key accepts and runs the operation
  - Skip option to bypass one command without cancelling the rest
- Safe practices baked into the AI prompt
- No automatic execution without user approval (unless auto mode is explicitly enabled)
- Direct execution limited to read-only commands
- Dangerous operations always require confirmation
- Auto mode has layered safeguards: a missing safety verdict counts as
  destructive, a deny-list (`rm`, `sudo`, `dd`, ...) overrides the model's
  verdict, and piped-data sessions never auto-execute

## Configuration File

The config file is located at `~/.ask/config` and uses a simple key-value format:

```
theme=dark
model=anthropic/claude-haiku-4.5
auto=off
```

Available settings:

| Key | Values | Description |
|-----|--------|-------------|
| `theme` | `dark`, `light` | Color theme for terminal output |
| `model` | Any OpenRouter model ID | LLM model to use (set via `ask model <id>`; overrides the built-in default) |
| `auto` | `on`, `off` | Run AI-labeled-safe commands without confirmation (set via `ask auto on`) |

The `--model` and `--theme` CLI flags take precedence over config file values. If no model is set in the config, the built-in default (`meta-llama/llama-3.3-70b-instruct`) is used.

## Testing

The default suite is deterministic and does not make API calls:

```bash
make test
```

Live OpenRouter contract tests are opt-in, require `OPENROUTER_ASK_API_KEY`, and make real API calls. The 22 live tests cover 26 model samples across action phrasing, conversational responses, safety verdicts, quoting, Unicode, stateful command chains, piped data, prompt-injection text, and repeated-response variance:

```bash
make test-live
```

## Model Benchmarks

Integration tests run real prompts against each model via OpenRouter. Results from the test suite:

| Model | Total Time | Fastest Test | All Tests Pass |
|-------|-----------|-------------|----------------|
| `meta-llama/llama-3.3-70b-instruct` | 1.65s | — | Yes |
| `openai/gpt-4o-mini` | 1.87s | — | Yes |
| `anthropic/claude-haiku-4.5` | 3.32s | — | Yes |
| `qwen/qwen3-coder-next` | 3.70s | 420ms | Yes |
| `meta-llama/llama-4-maverick` | 17.21s | 637ms | Yes |

To benchmark with your configured model:

```bash
make test-live
```

Times vary by run due to API latency, but relative rankings are consistent.

## Dependencies

- `serde` / `serde_json` - JSON serialization/deserialization
- `ureq` - HTTP client for API requests
- `rustyline` - Line editing and history in interactive mode
- `libc` - Terminal input flushing for confirmations
- `sha2` - Checksums that gate saved-tool execution on approval

## Building

```bash
# Development build
cargo build

# Release build (optimized and stripped)
cargo build --release

# Run without installing
cargo run -- "your prompt here"
```

## Troubleshooting

### API Key Not Set

```
Error: Please set the OPENROUTER_ASK_API_KEY environment variable.
```

**Solution**: Export your OpenRouter API key as shown in the Configuration section. Take note of the _ASK_ in the environment variable.

### No Command Returned

```
Error: No command returned from the model.
```

**Solution**: Try rephrasing your prompt to be more specific about what you want to accomplish.

### Permission Denied

```
Error: Command exited with status exit status: 1
```

**Solution**: Some commands may require `sudo`. The tool will ask for your password if needed.

### Context Too Large

```
Note: Context is being automatically compacted to fit within token limits.
```

**Solution**: This is automatic and normal. Use `clear` command to reset context if needed.

### Interactive Mode Tips

- **Lost track of directory?** Type `.` to see current path
- **Want to go back?** Type `..` to go up a directory
- **Need to see files visually?** Type `finder` to open Finder
- **Context getting cluttered?** Type `clear` to reset


## Credits

Built with Rust and powered by OpenRouter's AI models.

Default model: Meta Llama 3.3 70B Instruct
