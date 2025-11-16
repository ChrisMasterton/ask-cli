# ask - AI-Powered MacOS Command Assistant

A command-line tool that converts natural language prompts into MacOS terminal commands using AI. Built with Rust for speed and reliability.

## Overview

`ask` is a CLI assistant that uses OpenRouter's API to generate MacOS Zsh commands from plain English descriptions. It provides an interactive workflow where generated commands are presented for confirmation before execution, ensuring safety and transparency.

## Features

- **Natural Language to Commands**: Describe what you want to do, get the exact shell commands
- **Interactive Confirmation**: Review and approve commands before they run
- **Safe by Design**: Built-in safeguards against dangerous operations
- **Theme Support**: Light and dark color themes for terminal readability
- **Model Selection**: Choose from various LLM models via OpenRouter
- **Persistent Configuration**: Saves theme preferences locally
- **MacOS & Zsh Optimized**: Tailored for MacOS terminal environment

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
ask <your natural language prompt>
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
ask [OPTIONS] <prompt>

Options:
  --model MODEL     Override the default LLM model (default: meta-llama/llama-3.3-70b-instruct)
  --theme MODE      Color theme for prompts (dark or light, default dark)
  -h, --help        Show help message
```

### Using Custom Models

```bash
ask --model anthropic/claude-3.5-sonnet "your prompt here"
```

## How It Works

1. **Prompt Processing**: Your natural language request is sent to OpenRouter's API
2. **Command Generation**: The AI model generates appropriate MacOS Zsh commands
3. **Interactive Review**: Generated commands are displayed with syntax highlighting
4. **User Confirmation**: You approve or reject each command before execution
5. **Safe Execution**: Approved commands run in your default shell

## Safety Features

- Commands are always shown before execution
- Requires explicit confirmation (Y/n prompt)
  - Return key will accept and run the operation
  - Escape key support to cancel operations
- Safe practices baked into the AI prompt
- No automatic execution without user approval

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


## Credits

Built with Rust and powered by OpenRouter's AI models.

Default model: Meta Llama 3.3 70B Instruct
