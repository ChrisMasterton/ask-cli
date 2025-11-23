# Direct Command Execution

## Overview
The `ask` CLI now executes common, non-destructive commands directly without requiring LLM processing or confirmation. This makes interactive mode much more fluid for everyday tasks.

## Direct Execution Commands

### File System Navigation & Listing
- `ls`, `ll`, `la` - List files and directories
- `pwd` - Print working directory
- `cd [path]` - Change directory (special handling to actually change the shell's directory)
- `tree` - Display directory tree

### File Reading (Non-destructive)
- `cat [file]` - Display file contents
- `head [file]` - Show first lines of file
- `tail [file]` - Show last lines of file
- `wc [file]` - Word/line/character count
- `file [file]` - Determine file type
- `stat [file]` - File statistics
- `grep [pattern]` - Search for patterns
- `find [path]` - Find files and directories
- `diff [file1] [file2]` - Compare files

### System Information
- `date` - Current date and time
- `uptime` - System uptime
- `whoami` - Current username
- `hostname` - System hostname
- `uname` - System information
- `df` - Disk space usage
- `du` - Directory space usage
- `ps` - Process status
- `who`, `w` - Who's logged in

### Environment & Shell
- `echo [text]` - Print text
- `env`, `printenv` - Show environment variables
- `which [command]` - Locate command
- `type [command]` - Display command type
- `alias` - Show aliases

### Git (Read Operations)
- `git status` - Repository status
- `git log` - Commit history
- `git diff` - Show changes
- `git branch` - List branches
- `git remote` - Show remotes

## How It Works

When you type a safe command in interactive mode:
1. It's recognized as a direct command
2. Executed immediately without LLM confirmation
3. Output is displayed
4. Command and output are added to context history
5. LLM can reference it in future interactions

## Example Session

```
$ ask
Interactive mode. Commands: 'exit', 'clear', 'finder'
Common commands (ls, pwd, cat, etc.) execute directly without confirmation

ask> pwd
run> pwd
/Users/chris/Projects/ask-cli

ask> ls
run> ls
Cargo.toml    src/    target/    README.md

ask> cd src
run> cd src
Changed directory to: /Users/chris/Projects/ask-cli/src

ask> cat main.rs | head -5
run> cat main.rs | head -5
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::io::{self, Write};

ask> what file did we just look at?
# We just looked at main.rs in the src directory. It's the main Rust source file for your ask CLI application, showing the imports at the top of the file.
```

## Special Features

### CD Command
The `cd` command actually changes your working directory in the interactive session:
```
ask> cd /tmp
Changed directory to: /tmp

ask> pwd
/tmp

ask> cd
Changed directory to: /Users/chris
```

### Context Preservation
All direct commands and their outputs are added to the conversation context:
```
ask> ls -la | grep "^d"
run> ls -la | grep "^d"
drwxr-xr-x  12 chris  staff   384 Jan 15 10:30 .
drwxr-xr-x   5 chris  staff   160 Jan 15 09:00 ..
drwxr-xr-x   8 chris  staff   256 Jan 15 10:30 src

ask> how many directories do we have?
# Based on the output from your grep command, you have 3 directories: the current directory (.), the parent directory (..), and the src directory.
```

## Benefits

### Speed & Efficiency
- **Instant execution**: No API call needed
- **No confirmation**: Skip the [Y/n/s/i] prompt
- **Natural workflow**: Type commands as you would in a normal shell

### Safety
- **Read-only operations**: Only non-destructive commands
- **No modifications**: Can't accidentally delete or modify files
- **Explicit dangerous commands**: Anything destructive still goes through LLM

### Context Awareness
- **Full history**: All commands tracked in context
- **LLM awareness**: AI can reference previous direct commands
- **Intelligent responses**: Better assistance based on what you've done

## Commands NOT Direct-Executed

These commands still require LLM processing and confirmation:
- `rm`, `mv`, `cp` - File modifications
- `mkdir`, `touch` - File creation
- `chmod`, `chown` - Permission changes
- `git commit`, `git push` - Git write operations
- `npm install`, `pip install` - Package installations
- Any command with `>`, `>>` - Output redirection
- Any command with `sudo` - Privileged operations

## Tips

1. **Mix and match**: Use direct commands for exploration, LLM for complex tasks
   ```
   ask> ls
   ask> cat config.json
   ask> update the port in the config to 8080
   ```

2. **Quick navigation**: Use `cd` to move around freely
   ```
   ask> cd ~/Projects
   ask> ls
   ask> cd my-app
   ```

3. **Rapid exploration**: Chain direct commands for quick analysis
   ```
   ask> pwd
   ask> ls -la
   ask> cat package.json
   ask> grep "test" package.json
   ```

The direct command execution makes `ask` feel more like an enhanced shell rather than just a command generator!