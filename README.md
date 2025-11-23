# ask - AI-Powered MacOS Command Assistant

A command-line tool that converts natural language prompts into MacOS terminal commands using AI. Built with Rust for speed and reliability.

## Quick Start

```bash
# Set your API key
export OPENROUTER_ASK_API_KEY="your-key"

# Single command
ask "show all python files"

# Interactive mode (new!)
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
- **Safe by Design**: Built-in safeguards against dangerous operations
- **Theme Support**: Light and dark color themes for terminal readability
- **Model Selection**: Choose from various LLM models via OpenRouter
- **Persistent Configuration**: Saves theme preferences locally
- **MacOS & Zsh Optimized**: Tailored for MacOS terminal environment

### Interactive Mode (New!)
- **Persistent Session**: Run multiple prompts without restarting
- **Context Awareness**: Maintains conversation history with smart token management
- **Direct Command Execution**: Common commands (ls, pwd, cat, etc.) run instantly
- **Shortcuts**: Quick commands like `q` (quit), `.` (pwd), `..` (cd ..)
- **Finder Integration**: Type `finder` to open current directory in Finder
- **Directory Display**: Current folder shown in prompt for constant awareness

### Command Execution Options
- **Skip (s)**: Skip current command and continue to next
- **Instruct (i)**: Run a custom command first, then return to original
- **Conversational Responses**: AI can respond without generating commands

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
Interactive mode. Commands: 'exit', 'clear', 'finder'
Common commands (ls, pwd, cat, etc.) execute directly without confirmation
Shortcuts: q=quit, .=pwd, ..=cd ..
📁 /Users/chris/Projects

ask [Projects]> ls
run> ls -l
total 48
drwxr-xr-x  12 chris  staff   384 Jan 15 10:30 ask-cli
drwxr-xr-x   8 chris  staff   256 Jan 14 09:15 other-project

ask [Projects]> cd ask-cli
run> cd ask-cli
Changed directory to: /Users/chris/Projects/ask-cli

ask [ask-cli]> create a readme file
run> touch README.md? [Y/n/s/i] y

ask [ask-cli]> q
Goodbye!
```

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

Options:
  --model MODEL     Override the default LLM model (default: meta-llama/llama-3.3-70b-instruct)
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show help message

Modes:
  With prompt:      Single command execution mode
  Without prompt:   Interactive mode with persistent session
```

### Command Confirmation Options

When a command is presented for confirmation, you have multiple options:

```
run> command? [Y/n/s/i]

Y/yes (Enter)     Execute the command
n/no              Cancel and exit (or return to prompt in interactive mode)
s/skip            Skip this command, continue to next
i/instruct        Execute a custom command first, then return to original
```

### Using Custom Models

```bash
ask --model anthropic/claude-3.5-sonnet "your prompt here"
```

## Interactive Mode Features

### Direct Commands
These commands execute immediately without LLM processing or confirmation:

- **Navigation**: `ls`, `pwd`, `cd`, `tree`
- **File Reading**: `cat`, `head`, `tail`, `grep`, `find`, `diff`
- **System Info**: `date`, `whoami`, `hostname`, `df`, `ps`
- **Git Status**: `git status`, `git log`, `git diff`, `git branch`
- **Environment**: `echo`, `env`, `which`, `type`

Note: Plain `ls` automatically executes as `ls -l` for better file information.

### Shortcuts

| Shortcut | Action | Description |
|----------|--------|-------------|
| `q` | Quit | Exit interactive mode |
| `.` | `pwd` | Show current directory |
| `..` | `cd ..` | Go up one directory |
| `finder` | Open Finder | Open current directory in Finder |
| `clear` | Clear & Reset | Clear screen and reset context |

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
3. **Interactive Review**: Generated commands are displayed with syntax highlighting
4. **User Confirmation**: You approve or reject each command before execution
5. **Safe Execution**: Approved commands run in your default shell

## Safety Features

- Commands are always shown before execution
- Multiple confirmation options (Y/n/s/i)
  - Return key accepts and runs the operation
  - Skip option to bypass without exiting
  - Instruct option to run custom commands first
- Safe practices baked into the AI prompt
- No automatic execution without user approval
- Direct execution limited to read-only commands
- Dangerous operations always require confirmation

## Configuration File

The config file is located at `~/.ask/config` and uses a simple key-value format:

```
theme=dark
```

This file is automatically created and updated when using the `--theme` flag.

## Dependencies

- `serde` - JSON serialization/deserialization
- `serde_json` - JSON handling
- `ureq` - HTTP client for API requests
- `dirs` - Cross-platform path utilities

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
