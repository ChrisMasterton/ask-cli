# New Features: Skip and Instruct Options

## Overview
The `ask` CLI now supports two additional options when confirming commands:
- **Skip (s)**: Skip the current command and continue to the next one
- **Instruct (i)**: Execute a custom command first, then return to the original question

## Feature Details

### Skip Option (s)
- When you press `s` or type `skip`, the current command is skipped
- The application continues with the next command in the queue
- Unlike pressing `n` (which exits the app), skip allows you to continue processing remaining commands

### Instruct Option (i)
- When you press `i` or type `instruct`, you can enter a custom command
- The custom command is executed first
- After execution, you're asked again about the original command
- This is useful for running preparatory commands (e.g., checking directory contents before deleting files)

## Example Usage Scenarios

### Scenario 1: Skipping Unnecessary Commands
```
Run command: ls -la? [Y/n/s/i] s
Skipping command: ls -la
Run command: echo "Hello World"? [Y/n/s/i] y
Hello World
```

### Scenario 2: Using Instruct to Prepare
```
Run command: rm file.txt? [Y/n/s/i] i
Enter command: ls -la file.txt
-rw-r--r-- 1 user group 1024 Jan 1 12:00 file.txt
Returning to original command:
Run command: rm file.txt? [Y/n/s/i] y
```

## Implementation Changes

1. **New Enum**: `ConfirmResponse` replaces the boolean return
   - `Yes`: Execute the command
   - `No`: Cancel and exit
   - `Skip`: Skip this command, continue to next
   - `Instruct(String)`: Execute custom command first

2. **Updated Prompt**: Changed from `[Y/n]` to `[Y/n/s/i]`

3. **Flow Control**: Skip uses `continue` instead of `return Ok(())` to keep processing

4. **Input Validation**: Invalid responses now show a helpful message and re-prompt