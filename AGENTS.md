# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project Overview

`ask` is an AI-powered macOS command-line assistant written in Rust. It sends natural language prompts to the OpenRouter API and returns executable shell commands. All source code lives in a single file: `src/main.rs`.

## Build & Run Commands

```bash
make build          # Debug build
make release        # Optimized release build (stripped binary)
make install        # Build release and install to /usr/local/bin/ask
make clean          # Clean build artifacts
```

## Testing

```bash
cargo test                              # Run unit tests only
cargo test -- --ignored --show-output   # Run integration tests (requires OPENROUTER_ASK_API_KEY)
```

Integration tests make real API calls to OpenRouter and require the `OPENROUTER_ASK_API_KEY` environment variable. Unit tests are at the bottom of `src/main.rs` and cover input normalization, confirmation parsing, and command parsing.

## Architecture

The application has three execution modes, dispatched from `run()`:

1. **Single Prompt Mode** (`ask "prompt"`) — processes one prompt, confirms, executes, exits
2. **Interactive Mode** (`ask` with no args) — persistent session with conversation history via rustyline
3. **Pipe Mode** (`command | ask "prompt"`) — analyzes piped stdin data using a specialized prompt template

Key flow: user prompt → `process_prompt_with_context()` → `query_api()` (OpenRouter) → `parse_commands()` → `confirm()` → `run_command_with_output()`

**Safe commands** bypass the LLM entirely — read-only commands like `ls`, `pwd`, `cat`, `git status` are executed directly via `is_safe_direct_command()`.

**Conversation context** is maintained in interactive mode via `ConversationContext` structs, with automatic compaction when approaching `MAX_CONTEXT_TOKENS` (3000).

## Configuration

User config is stored at `~/.ask/config` (key=value format) with theme and model preferences. CLI flags `--model` and `--theme` override saved config.

## Key Constants

- `API_URL`: OpenRouter endpoint
- `DEFAULT_MODEL`: `meta-llama/llama-3.3-70b-instruct`
- `MAX_CONTEXT_TOKENS`: 3000 (triggers history compaction)
- `MAX_PIPE_BYTES`: 64KB limit for piped stdin data

## Rust Edition

Uses Rust edition 2024 (requires rustc 1.85+).
