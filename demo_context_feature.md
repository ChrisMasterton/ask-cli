# Context Accumulation in Interactive Mode

## Overview
The interactive mode now maintains conversation context throughout your session. This means the LLM can see previous commands and their outputs, allowing for more intelligent suggestions based on what you've already done.

## How Context Works

### Automatic Context Building
- Each prompt you enter is remembered
- Commands that were executed are tracked
- Command outputs are captured and stored (truncated to 500 chars if too long)
- This context is sent with each new prompt to the LLM

### Clear Command
- Type `clear` to:
  - Clear the terminal screen
  - Reset all conversation context
  - Start fresh with no history

## Example Session with Context

```
$ ask
Interactive mode. Type 'exit' or 'quit' to exit, 'clear' to reset.

ask> create a test directory
run> mkdir test_project? [Y/n/s/i] y

ask> now create a Python file in that directory
# The LLM knows you just created test_project, so it suggests:
run> touch test_project/main.py? [Y/n/s/i] y

ask> list what we've created so far
# The LLM has context of the previous commands:
run> ls -la test_project? [Y/n/s/i] y
total 0
drwxr-xr-x  3 user  staff   96 Jan 15 10:30 .
drwxr-xr-x  5 user  staff  160 Jan 15 10:30 ..
-rw-r--r--  1 user  staff    0 Jan 15 10:30 main.py

ask> add a hello world to the python file
# The LLM knows about test_project/main.py from context:
run> echo 'print("Hello, World!")' > test_project/main.py? [Y/n/s/i] y

ask> clear
# Screen clears, context resets

ask> what files are in test_project?
# After clear, the LLM has no context of previous commands
run> ls test_project? [Y/n/s/i] y
```

## Benefits of Context

1. **Continuity**: The LLM understands what you've already done
2. **Smarter Suggestions**: Commands build on previous work
3. **Less Repetition**: No need to re-explain your project structure
4. **Natural Workflow**: Work on tasks step-by-step with understanding

## Context Limitations

- Output is truncated to 500 characters per command to avoid token limits
- Context is lost when you exit interactive mode
- Context is cleared when you use the `clear` command
- Very long sessions may hit API token limits

## Implementation Details

### What's Included in Context
- User prompts
- Executed commands (not skipped ones)
- Command outputs (stdout and stderr)
- Commands run via 'instruct' option are not tracked

### API Message Structure
```json
{
  "messages": [
    {
      "role": "system",
      "content": "Previous commands and outputs in this session:\n\nUser: create test file\nCommand: touch test.txt\nOutput: \n\n..."
    },
    {
      "role": "user",
      "content": "Your current prompt with template"
    }
  ]
}
```

## Use Cases

### Progressive Development
Build a project step by step with the LLM understanding your progress:
1. Create project structure
2. Add configuration files
3. Create source files
4. Set up dependencies

### Debugging Sessions
Work through debugging with context:
1. Run diagnostic commands
2. Based on output, get targeted fix suggestions
3. Apply fixes and verify

### Learning and Exploration
Explore a system with accumulated knowledge:
1. Check system information
2. Based on findings, explore specific areas
3. Build understanding progressively