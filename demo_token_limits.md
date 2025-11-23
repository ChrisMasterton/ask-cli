# Token Limit Management

## Overview
The `ask` CLI now automatically manages conversation context to stay within API token limits, ensuring reliable operation even during long interactive sessions.

## How Token Management Works

### Token Estimation
- Uses a conservative estimate of 1 token per 4 characters
- Tracks total context size across all conversation history
- Reserves ~1000 tokens for model responses

### Context Limits
- **Maximum Context**: 3000 tokens (~12,000 characters)
- **Output Truncation**: Long outputs truncated to 200 chars when compacting
- **Automatic Compaction**: Keeps most recent interactions when limit approached

## Context Compaction Strategy

### When Context Exceeds Limits
1. **Prioritizes Recent History**: Keeps the most recent interactions
2. **Removes Oldest First**: Drops oldest context to make room
3. **Smart Truncation**: More aggressive output truncation during compaction
4. **User Notification**: Displays a note when compaction occurs

### Visual Indicators
```
ask> [continue working...]
Note: Context is being automatically compacted to fit within token limits.
```

When context is compacted, the system message includes:
```
(Note: Showing recent 5 of 12 total interactions due to length)
```

## Example Long Session

```
$ ask
Interactive mode. Type 'exit' or 'quit' to exit, 'clear' to reset.

ask> create a complex project structure
run> mkdir -p project/{src,tests,docs}? [Y/n/s/i] y

[... many interactions later ...]

ask> add final configuration
run> echo "config complete" > project/config.txt? [Y/n/s/i] y
Note: Context is being automatically compacted to fit within token limits.

ask> what have we been working on?
# Based on our recent interactions, we've been setting up a project with source, test, and documentation directories, and just added a configuration file. The earlier setup details have been condensed to stay within token limits.
```

## Token Limits by Component

### Character Estimates
- **Base Context Header**: ~50 characters
- **Per Interaction**:
  - User prompt: Variable (typically 20-100 chars)
  - Command: Variable (typically 20-80 chars)
  - Output: Max 500 chars (normal), 200 chars (compacted)

### Practical Limits
- **Short Commands**: ~20-30 interactions before compaction
- **Long Commands/Outputs**: ~10-15 interactions before compaction
- **Conversational**: ~30-40 exchanges before compaction

## Managing Long Sessions

### Best Practices

1. **Use `clear` Periodically**: Reset context when starting new tasks
   ```
   ask> clear
   ```

2. **Keep Outputs Concise**: Avoid commands with very long outputs
   ```
   # Instead of: cat large_file.txt
   # Use: head -20 large_file.txt
   ```

3. **Segment Work**: Break complex tasks into logical sections
   ```
   ask> # Finished setup phase
   ask> clear
   ask> # Starting implementation phase
   ```

## Configuration Options

### Current Settings (Hardcoded)
```rust
const MAX_CONTEXT_TOKENS: usize = 3000;
const TOKEN_ESTIMATE_RATIO: usize = 4;
```

### Future Enhancements (Not Yet Implemented)
- Model-specific token limits
- Configurable compaction strategy
- Token count display command
- Manual context management

## Technical Details

### Compaction Algorithm
1. Estimates tokens for each historical interaction
2. Builds context from most recent backwards
3. Stops when token limit would be exceeded
4. Adds truncation notice if history was shortened

### Token Estimation Formula
```
estimated_tokens = character_count / 4
```

This is conservative as most tokenizers average 3-5 characters per token.

## Benefits

### Reliability
- Prevents API errors from token limit exceeded
- Maintains functionality in long sessions
- Graceful degradation of context

### Performance
- Faster API responses with managed context size
- Lower API costs (tokens = cost)
- Consistent response times

### User Experience
- Automatic management (no manual intervention needed)
- Clear notifications when compaction occurs
- Most recent context always preserved

## Limitations

### What's Lost During Compaction
- Oldest interactions removed first
- Very long outputs truncated more aggressively
- Fine details from earlier commands

### Workarounds
- Use `clear` to reset when switching tasks
- Keep important information in recent context
- Re-explain context if needed after compaction

The token management system ensures that `ask` remains responsive and reliable even during extended interactive sessions!