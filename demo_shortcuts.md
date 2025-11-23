# Interactive Mode Shortcuts

## Overview
The `ask` CLI now includes convenient shortcuts for the most common operations, making navigation and exploration even faster.

## Available Shortcuts

| Shortcut | Equivalent | Description |
|----------|------------|-------------|
| `q` | `quit` or `exit` | Exit interactive mode |
| `.` | `pwd` | Show current directory |
| `..` | `cd ..` | Go up one directory |

## Usage Examples

### Quick Exit
```
ask [Projects]> q
Goodbye!
```

### Check Current Directory
```
ask [src]> .
run> pwd
/Users/chris/Projects/ask-cli/src
```

### Navigate Up
```
ask [src]> ..
run> cd ..
Changed directory to: /Users/chris/Projects/ask-cli

ask [ask-cli]>
```

## Workflow Example

```
$ ask
Interactive mode. Commands: 'exit', 'clear', 'finder'
Common commands (ls, pwd, cat, etc.) execute directly without confirmation
Shortcuts: q=quit, .=pwd, ..=cd ..
📁 /Users/chris/Projects/ask-cli

ask [ask-cli]> ls
run> ls
Cargo.toml    src/    target/    README.md

ask [ask-cli]> cd src
run> cd src
Changed directory to: /Users/chris/Projects/ask-cli/src

ask [src]> .
run> pwd
/Users/chris/Projects/ask-cli/src

ask [src]> ..
run> cd ..
Changed directory to: /Users/chris/Projects/ask-cli

ask [ask-cli]> q
Goodbye!
```

## Benefits

### Speed
- **Single character**: `q` is faster than typing `exit`
- **Muscle memory**: `.` and `..` are familiar from shell navigation
- **Less typing**: Reduces keystrokes for common operations

### Intuitive
- **`.`**: Single dot = current location (standard Unix convention)
- **`..`**: Two dots = parent directory (universal convention)
- **`q`**: Common quit shortcut in many CLI tools (vim, less, etc.)

### Context Preservation
All shortcuts that execute commands (`.` and `..`) are tracked in the conversation history, so the LLM knows where you've navigated:

```
ask [Projects]> .
run> pwd
/Users/chris/Projects

ask [Projects]> what directory am i in?
# You're in /Users/chris/Projects, as shown by the pwd command you just ran.
```

## Combined with Other Features

The shortcuts work seamlessly with all other interactive mode features:

### With Direct Commands
```
ask [src]> ls
ask [src]> ..
ask [ask-cli]> ls
```

### With Finder
```
ask [Documents]> .
ask [Documents]> finder  # Opens Finder at current location
```

### With Clear
```
ask [tmp]> clear
ask [tmp]> .
run> pwd
/tmp
```

## Complete Shortcut Reference

### Navigation & Information
- `.` - Show current directory (pwd)
- `..` - Go up one directory (cd ..)
- `finder` - Open Finder window at current location

### Session Control
- `q` - Quick exit
- `exit` or `quit` - Exit interactive mode
- `clear` - Clear screen and reset context

### Direct Commands
- `ls`, `pwd`, `cd`, etc. - Execute without confirmation
- Any safe read-only command runs directly

## Tips

1. **Quick Navigation**: Use `..` repeatedly to go up multiple levels
   ```
   ask [deep/nested/folder]> ..
   ask [nested]> ..
   ask [deep]> ..
   ```

2. **Location Check**: Use `.` when you're unsure where you are
   ```
   ask [?]> .
   run> pwd
   /Users/chris/Desktop
   ```

3. **Fast Exit**: Just hit `q` and Enter to leave quickly

The shortcuts make `ask` feel even more like a natural extension of your terminal workflow!