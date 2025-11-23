# Finder Command in Interactive Mode

## Overview
The `ask` CLI now includes a custom `finder` command in interactive mode that opens a Finder window at your current working directory.

## Usage

In interactive mode, simply type `finder`:

```
$ ask
Interactive mode. Commands: 'exit', 'clear', 'finder'

ask> finder
Opened Finder at current directory

ask>
```

## How It Works

The `finder` command uses macOS's built-in `open .` command to open Finder at the current directory. This is equivalent to running:
```bash
open .
```

## Example Workflow

```
$ ask
Interactive mode. Commands: 'exit', 'clear', 'finder'

ask> create a new project folder
run> mkdir my-new-project? [Y/n/s/i] y

ask> go into it
run> cd my-new-project? [Y/n/s/i] y

ask> create some files
run> touch README.md main.py requirements.txt? [Y/n/s/i] y

ask> finder
Opened Finder at current directory

# Now you can see your new project files in Finder!
```

## Benefits

1. **Quick Visual Access**: Instantly see your files in Finder without leaving the terminal
2. **Seamless Workflow**: No need to manually navigate to the directory in Finder
3. **Context Aware**: Always opens at your current working directory
4. **Simple Command**: Just type 'finder' - easy to remember

## Interactive Mode Commands Summary

| Command | Action |
|---------|--------|
| `exit` or `quit` | Exit interactive mode |
| `clear` | Clear screen and reset conversation context |
| `finder` | Open Finder at current directory |

## Tips

- Use `finder` after creating or modifying files to visually verify changes
- Combine with `cd` commands to navigate and then open Finder at specific locations
- The Finder window opens in the background - you stay in the terminal

## Technical Notes

- Uses macOS's `open` command with `.` (current directory) argument
- Only works on macOS (as Finder is macOS-specific)
- Non-blocking: Opens Finder without interrupting your terminal session

## Alternative Uses

While the built-in command opens the current directory, you can always ask the LLM for variations:

```
ask> open finder at the parent directory
run> open ..? [Y/n/s/i] y

ask> open finder at my home folder
run> open ~? [Y/n/s/i] y

ask> open finder at a specific path
run> open /Applications? [Y/n/s/i] y
```

But having `finder` as a quick built-in command makes the most common case (current directory) instant and effortless!