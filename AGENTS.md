# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project Overview

`ask` is an AI-powered macOS command-line assistant written in Rust. It sends natural language prompts to the OpenRouter API and returns executable shell commands.

Runtime modules:

- `src/main.rs`: process entry point, interactive direct-command policy, and confirmed-line routing
- `src/api.rs`: shared chat-completion request state machine and HTTP/transport errors
- `src/api/response.rs`: provider response data and first-choice parsing state machine
- `src/prompt.rs`: prompt execution and one-shot completion state machines
- `src/prompt/request.rs`: message-preparation/query state machine and prompt templates
- `src/startup.rs`: startup state machine, local dispatch, API-key gate, and execution-mode selection
- `src/cli.rs`: argument parsing state machine and help text
- `src/config.rs`: configuration load/save state machines and saved preference format
- `src/config/preferences.rs`: model and auto preference command state machines
- `src/theme.rs`: theme values and terminal colors
- `src/text.rs`: shared UTF-8 byte truncation for piped prompts and tool descriptions
- `src/session.rs`: interactive session state machine, input dispatch, settings, and history-update effects
- `src/conversation.rs`: retained conversation updates, outcome rendering, and context-selection state machine
- `src/execution.rs`: command execution state machine, batch stop reasons, automatic execution availability, and outcome reports
- `src/confirmation.rs`: confirmation state machine, command/tool approval policies, and terminal input
- `src/subprocess.rs`: subprocess preparation, spawning, waiting, and completion for shell commands and inherited-I/O utilities
- `src/terminal_io.rs`: terminal/pipe selection, concurrent output capture, bounded retention, and terminal-control decoding
- `src/working_directory.rs`: child-directory reports, parent navigation, and before/after directory observation
- `src/safety.rs`: safety request/assessment data, confidence thresholds, and local eligibility policy
- `src/safety/assessment.rs`: bounded provider assessment state machine
- `src/safety/cache.rs`: policy-gated assessment/cache state machine and atomic cache-write state machine
- `src/tools.rs`: saved-tool CLI routing, model-response parsing, manifest format, and invocation resolution
- `src/tools/workflow.rs`: saved-tool creation/improvement/reapproval workflow and approval persistence
- `src/tools/integrity.rs`: fresh integrity inspection shared by listing, the model catalog, and invocation

## Build & Run Commands

```bash
make build          # Debug build
make release        # Optimized release build (stripped binary)
make install        # Build release and install to /usr/local/bin/ask
make clean          # Clean build artifacts
```

## Testing

```bash
cargo test                              # Run deterministic unit and CLI tests
cargo test -- --ignored --show-output   # Run integration tests (requires OPENROUTER_ASK_API_KEY)
```

Opt-in tests in `src/integration_tests.rs` and the ignored safety evaluation make real OpenRouter calls and require `OPENROUTER_ASK_API_KEY`. Deterministic tests live in `src/tests.rs`, runtime module test sections, and `tests/cli.rs`.

## Architecture

The application has three execution modes, dispatched from `startup::run()`:

1. **Single Prompt Mode** (`ask "prompt"`) — processes one prompt, confirms, executes, exits
2. **Interactive Mode** (`ask` with no args) — persistent session with conversation history via rustyline
3. **Pipe Mode** (`command | ask "prompt"`) — analyzes piped stdin data using a specialized prompt template

Key flow: user prompt → `prompt::process_prompt_with_context()` → message/query machine → `api::call_llm()` (OpenRouter) → response parsing machine → execution machine → confirmation when required → `run_line()` → subprocess machine

API and prompt machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| AP-01: Chat request | `api.rs` | Build model/messages body → send → decode successful JSON or read HTTP error body → complete/failed. Connection/transport failures remain network errors. |
| AP-02: Response parsing | `api/response.rs` | Select first choice → trim outer content → parse for the caller → complete/failed. Command parsing filters blank/fence lines and rejects an empty result; tool-source parsing preserves indentation and fences for its existing parser. |
| PM-01: Query preparation | `prompt/request.rs` | Render prior history → load current catalog unless piped → build user message → request → parse commands → complete/failed. One provider invocation per query; no new retry loop. |
| PM-02: Prompt execution | `prompt.rs` | Query → execute proposals → complete with the full report, or fail before execution on request/parsing errors. The existing execution machine owns per-command stops and outcomes. |
| PM-03: One-shot completion | `prompt.rs` | Capture directory → process prompt → check report → observe directory → optionally print calling-shell note → complete/failed. Report errors return before directory observation/notification. |

Chat requests retain the existing endpoint, JSON model/messages payload, Bearer authorization, 10-second connect timeout, 120-second overall timeout, and ureq response readers. HTTP errors keep `API error CODE: BODY`; body-read failures use empty body text. Successful JSON decoding errors remain decoding errors. Saved-tool generation uses the same request machine; safety assessments use their separate bounded provider machine.

Only the first choice is considered. No choices reports `No command returned from the model.` for command generation or `The model returned an empty reply.` for tool source. A present empty/fence-only command reply reports `No response returned from the model.`; present empty tool content reaches the existing tool draft validation. Whole shell lines, including chains, are retained. History adds one system message before the user message only when retained history is nonempty. Catalogs are refreshed on each ordinary query and never read in pipe mode. Template text, literal-placeholder handling, and the 64-KB UTF-8 pipe truncation remain unchanged.

Interactive prompt processing returns full reports even after command or confirmation failures; the session decides what to record. One-shot processing returns the first report error, while cancellation stays a successful completion. Neither path treats failed requests or missing proposals as completed executions.

Startup and configuration machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| CL-01: Startup | `startup.rs` | Load configuration → parse → optionally save theme → dispatch local command, or require API key → read pipe → select AI mode → execute → complete/failed. Help goes directly from parsing to display and completion. |
| CL-02: Arguments | `cli.rs` | Next argument → await option value or collect literal/tool tail → finalize → help/run/failed. Help and errors discard partial overrides; only a completed run can request theme persistence. |
| CF-01: Configuration load | `config.rs` | Resolve HOME → read → parse → complete. Missing HOME or any read/UTF-8 error completes with defaults. Valid recognized values overwrite earlier values; invalid/unknown lines and empty model values are ignored. |
| CF-02: Configuration save | `config.rs` | Resolve HOME → create directory → serialize → write → complete/failed. Retains the existing direct, non-atomic write and key order. |
| CF-03: Preference commands | `config/preferences.rs` | Load fresh configuration → apply model/auto change → save → report → complete/failed. Model show skips save. Interactive settings change only after a successful result. |

Local auto/model commands, saved-tool invocation, and tool management dispatch before the startup API-key gate and pipe read. Tool creation/improvement retain their own API-key checks. Only AI modes consume piped context, after a successful API-key lookup; empty or terminal stdin selects interactive mode when no prompt was provided. A supplied empty prompt remains a one-shot request. Pipe reads retain at most 64 KB plus one sentinel byte, replace invalid UTF-8, and preserve any partial data after a read error.

Options remain recognized among ordinary prompt tokens until `--` or a first saved-tool invocation claims the remaining argv. Unknown flags remain prompt text; option values consume the next argument even when it resembles a flag. Separate saved-tool arguments are shell-quoted; embedded arguments in the first invocation argument retain their existing shell interpretation.

Generated commands transition through `Next`, `Assessing`, `AwaitingConfirmation`, `Running`, and `Complete`. Cancellation, execution failure, or confirmation failure stops later executable lines; comment lines still display. Skip continues the batch. An unavailable safety check requires manual confirmation for the rest of that AI request.

Confirmation transitions through `Prompting`, `AwaitingInput`, `InvalidInput`, `Decided`, and `Failed`. Command confirmation defaults to yes on Enter and retries invalid input. Saved-tool approval requires explicit yes and declines other input without retrying. EOF without input fails both policies.

Interactive and conversation machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| IS-01: Interactive session | `session.rs` | Banner → open editor → read → normalize/remember/route → handle → record/notify/reset/continue/exit. Ctrl-C returns to reading; EOF/quit closes. Input/editor errors and clear utility spawn/wait errors are fatal; ordinary action errors recover as before. |
| CH-01: Conversation updates | `conversation.rs` | Record a completed action → optionally check the retained-history estimate → return notice, or clear → complete. All writes go through this owner; the editor separately retains input history. |
| CH-02: Context selection | `conversation.rs` | Select whole turns newest-first until the first overflow → restore chronological order → add omission note → append → complete. Rendering does not mutate retained history. |

The model sees prior completed turns, never a pre-recorded copy of the current prompt. Direct commands and resolved tool invocations record both success and execution failure; AI reports retain all command outcomes, including comments, skipped/cancelled/unrun commands, and partial failures. API/parsing errors, unresolved tool invocations, and management/settings commands do not add conversation entries. `.` and successful `..` shortcuts record silently. Other recorded command/AI outcomes check the full-history estimate and can repeat the budget warning.

`clear` resets conversation context after the utility returns, even for a nonzero utility status. It keeps readline input history, model/auto settings, and working directory. Prompt context selection uses a 3000-token estimate at four UTF-8 bytes per token, with 4000 output bytes shared per turn before truncation-marker overhead. The header counts toward selection; the omission note does not. An oversized newest turn can produce a context containing no turns. The warning estimate excludes the header and omission note, preserving its separate threshold.

Subprocess and directory machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| SP-01: Child lifecycle | `subprocess.rs` | Prepare → spawn → capture → wait → resolve output → synchronize directory → complete/failed. Shell commands reject nonzero exits after directory sync; utilities return raw exit statuses. |
| IO-01: Child output | `terminal_io.rs` | Inherit streams, drain a PTY, or drain stdout/stderr concurrently → join → return deferred capture results. Child wait errors precede capture errors; stdout errors precede stderr errors. |
| IO-02: Stream retention | `terminal_io.rs` | Read → forward → flush → retain → read again or complete/failed. Interrupted reads retry; all bytes stream visibly while only the latest 64 KB per stream are retained. |
| IO-03: Terminal decoding | `terminal_io.rs` | Text → escape → CSI/OSC/OSC-escape → text. PTY history strips control sequences and normalizes CR; incomplete sequences retain the existing discard behavior. |
| WD-01: Directory change | `working_directory.rs` | Read child report → compare with starting directory → apply if changed → clean up → complete. The `..` shortcut enters the same apply transition directly. Missing, blank, or unreadable reports are ignored. |

Shell commands use a new shell each time, preserving state within a returned shell line and propagating only its reported directory back to Ask. Full terminal attachment selects `/usr/bin/script`; redirected output keeps terminal stdin when available, and nonterminal stdin is closed for shell commands. Utilities such as clear/open inherit all streams. Capture failures occur before directory synchronization; directory-change failures preserve the existing early return before report-file cleanup. One-shot prompts observe before/after directories and retain the calling-shell note on successful reports.

Saved-tool machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| ST-01: Authoring/review | `tools/workflow.rs` | Validate → generate (new/improve only) → review → write → record approval → complete. Decline discards; errors fail. Standalone approval enters review after loading source. |
| ST-02: Approval persistence | `tools/workflow.rs` | Read current manifest/script (improve/reapprove) or use the created draft → hash language and source → save manifest → approved/failed. Only accepted reviews reach this machine. |
| ST-03: Integrity inspection | `tools/integrity.rs` | Load manifest → read script → verify → approved/unapproved/mismatch/broken. Missing or invalid manifests fail inspection; readable metadata remains available when a script is broken. |

Tool integrity is derived from the existing manifest and script on every inspection; it is not a second persisted status. All three workflows retain the current manifest format, explicit-yes review prompts, language/fence parsing, and separate script/manifest writes. A failure after a script write does not roll it back, and integrity checks determine whether it can run.

Safety machines:

| ID | Owner | Transition contract |
| --- | --- | --- |
| SA-01: Provider assessment | `safety/assessment.rs` | Check command byte length → send → read bounded response → decode → complete/failed. A decoded uncertain or low-confidence assessment returns normally; transport/decoding errors remain errors. |
| SA-02: Assessment cache | `safety/cache.rs` | Check local policy → resolve context → load/revalidate → hit or assess once → consider caching → evict oldest if needed → persist → complete. Missing HOME/current directory/time bypasses caching. |
| SA-03: Cache persistence | `safety/cache.rs` | Create private directory/temporary file → serialize → flush → rename → clean up → return result. Cleanup only removes a file created by this call. |

The public safety entry point remains `safety::assess_cached()`. Cache keys and serialized entries retain the existing format and policy version. Only confident positive assessments are cached; TTL remains seven days with a 512-entry/512-KB read limit. Cache hits do not refresh timestamps or rewrite the file. Cache write failures preserve fresh assessment results; provider errors trigger the execution machine's per-response manual-confirmation fallback. The opt-in classifier evaluation calls the provider machine directly so its unsafe fixture commands are classified without executing them.

**Interactive direct commands** bypass the LLM entirely via `is_safe_direct_command()`. This includes supported read-only commands and recognized script invocations, independently of the generated-command auto setting.

**Conversation context** is maintained in interactive mode by `ConversationHistory`, containing `ConversationContext` turns. Prompt rendering selects recent interactions using `MAX_CONTEXT_TOKENS` (3000); it does not shrink the in-memory history.

## Configuration

User config is stored at `~/.ask/config` (key=value format) with theme, model, and auto preferences. `--model` overrides only the current run. After successful parsing, `--theme` saves only the theme change while retaining the saved model/auto values; save failures warn and keep the active override. Help and parse errors do not save. Model/auto command save failures are errors and do not report success.

## Key Constants

- `API_URL`: OpenRouter endpoint
- `DEFAULT_MODEL`: `meta-llama/llama-3.3-70b-instruct`
- `MAX_CONTEXT_TOKENS`: 3000 (historical context-selection budget in `conversation.rs`)
- `MAX_PIPE_BYTES`: 64KB limit for piped stdin data

## Rust Edition

Uses Rust edition 2024 (requires rustc 1.85+).
