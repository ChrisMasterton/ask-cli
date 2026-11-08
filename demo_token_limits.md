# Token Limit Management

`ask` selects recent conversation turns for each AI request. Selection limits the history sent to the model; it does not delete retained turns or summarize them.

## Current limits

- Token estimation uses UTF-8 byte length divided by four, rounded down.
- Historical context selection has a 3000-token estimated budget.
- Each turn shares a 4000-byte output allowance equally across its nonempty outputs. Truncation keeps the tail so final diagnostics survive; truncation markers add overhead.
- Command output streams visibly in full. Subprocess capture retains the latest 64 KB per stream before the final command-output limit and context rendering apply.

This is an estimate for historical context, not a tokenizer count or a limit on the entire API request. The current prompt and saved-tool catalog are assembled separately.

## Selection lifecycle

The context machine in `src/conversation.rs`:

1. Renders whole turns from newest to oldest.
2. Includes each turn while it fits with the context header.
3. Stops at the first turn that would exceed the budget.
4. Restores chronological order and adds an omission note when needed.

The omission note is outside the selection budget:

```text
(Note: Showing recent 5 of 12 total interactions due to length)
```

An oversized newest turn can leave zero turns selected. An oversized older turn also stops selection; the machine does not skip it to search for smaller earlier turns. Retained history remains available until `clear` or session exit.

## Notifications

After ordinary command and AI outcomes are recorded, the session checks the estimate of all retained turns. If it exceeds 3000 tokens, it prints:

```text
Note: Context is being automatically compacted to fit within token limits.
```

This check can repeat on later turns. It excludes the context header and omission note, so its threshold differs from actual selection. The `.` and successful `..` shortcuts record context without checking this warning.

## Resetting context

`clear` resets conversation context and redraws the banner. Readline input history, model selection, auto mode, and working directory remain in effect. Context is otherwise retained in memory for the session and is not restored after exit.
