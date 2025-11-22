# Interactive Mode Feature

## Overview
The `ask` CLI now supports an interactive mode that allows you to enter multiple prompts in a single session without having to restart the application.

## How to Enter Interactive Mode
Simply run `ask` without providing a prompt:
```bash
ask
# or with options
ask --model gpt-4 --theme light
```

## Interactive Mode Features

### Continuous Prompting
- Enter prompts one at a time
- Each prompt is processed through the LLM
- Commands are executed/skipped as usual
- After completion, returns to the prompt for the next query

### Exit Commands
- Type `exit` or `quit` to leave interactive mode
- The application will display "Goodbye!" and exit cleanly

### Error Handling
- If an error occurs (API error, network issue, etc.), the error is displayed
- The interactive session continues - you can try again with a new prompt
- Errors don't exit the interactive mode

## Example Session

```
$ ask
Interactive mode. Type 'exit' or 'quit' to exit.

ask> show me the current directory
Run command: pwd? [Y/n/s/i] y
/Users/chris/Projects/agent-tools/ask-cli

ask> list files in detail
Run command: ls -la? [Y/n/s/i] y
total 48
drwxr-xr-x  12 chris  staff   384 Jan 15 10:30 .
drwxr-xr-x   5 chris  staff   160 Jan 15 09:00 ..
-rw-r--r--   1 chris  staff  1234 Jan 15 10:30 Cargo.toml
...

ask> create a test file
Run command: touch test.txt? [Y/n/s/i] s
Skipping command: touch test.txt

ask> exit
Goodbye!
```

## Benefits of Interactive Mode

1. **Efficiency**: No need to restart the application for multiple queries
2. **Context**: Stay in the same working environment
3. **Exploration**: Great for learning and exploring commands
4. **Workflow**: Perfect for step-by-step task completion

## Behavior Differences

### In Single Prompt Mode
- `n/no` exits the entire application
- One prompt → commands → exit

### In Interactive Mode
- `n/no` cancels current command set but returns to prompt
- Multiple prompts → commands → continue until exit
- Errors don't terminate the session

## Use Cases

1. **Learning**: Explore different commands interactively
2. **Debugging**: Try various approaches to solve a problem
3. **Development**: Run multiple related commands in sequence
4. **Administration**: Perform multiple system tasks in one session

## Implementation Notes

- The API key and model are loaded once at startup
- Theme settings persist throughout the session
- Each prompt is independent (no conversation history with LLM)
- Commands from each prompt are executed before returning to the prompt