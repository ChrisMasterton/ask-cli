# Context Accumulation in Interactive Mode

The interactive session keeps prior outcomes as evidence for follow-up AI requests. Conversation context and readline input history have separate owners and lifetimes within the session.

## What is remembered

Readline remembers every nonempty, trimmed input before dispatch, including local commands and requests that later fail. Empty input and Ctrl-C interruptions do not create entries.

Conversation history records:

- AI replies and command reports, including comments, success, failure, skip, cancellation, confirmation failure, and commands not run.
- Direct command outcomes and approved saved-tool executions, including execution errors.
- The `.` shortcut as `pwd`, and successful `..` navigation as `cd ..`.

API/parsing failures and saved-tool resolution failures do not create conversation turns. Model/auto preferences, tool management, Finder, and exit commands remain in input history without becoming AI context.

A failed command may have partial effects. Its recorded status and diagnostics remain available alongside earlier successes; later commands stopped by the execution machine are recorded as not executed.

## Follow-up requests

The model receives selected prior turns as a system message before the current request. A recorded command can appear as:

```text
User: list project files
Command: ls -la
Status: succeeded (exit status 0)
Output: main.py
```

Conversational replies appear as assistant text in the same context. The current prompt is recorded after its outcome returns, so it is not duplicated in the prior-history message.

Long output keeps final diagnostics. Recent whole turns are selected for the prompt using the existing token estimate; full retained history is not pruned by selection. See [token management](demo_token_limits.md) for exact limits and overflow behavior.

## Clearing and exit

`clear` runs the screen-clearing utility, resets conversation context, and redraws the banner. It keeps readline history, the selected model, auto mode, and working directory. A utility spawn/wait error ends the session before reset; a returned nonzero status still permits reset.

Ctrl-C while reading abandons the current input and returns to the prompt. EOF, `q`, `quit`, or `exit` ends the session. Conversation context is held in memory and is not restored in a new session.

## Implementation

- `src/session.rs` owns input routing and session effects.
- `src/conversation.rs` owns record/clear transitions, outcome rendering, and prompt context selection.
- `src/execution.rs` supplies the authoritative command outcomes used in context.
