# ask - AI-Powered MacOS Command Assistant

A Rust CLI that turns plain English into MacOS terminal commands — and saves the good ones as reusable tools.

## See It in Action

Stop googling shell syntax — just say what you want:

```bash
$ ask "kill whatever is using port 3000"
run> kill $(lsof -t -i :3000)?  [Y/n]

$ ask "convert all these HEIC photos to jpg"
run> for f in *.heic; do sips -s format jpeg "$f" --out "${f%.heic}.jpg"; done?  [Y/n]

$ ask "undo my last commit but keep the changes"
run> git reset --soft HEAD~1?  [Y/n]
```

Pipe anything into it — logs, diffs, JSON, clipboard contents:

```bash
git diff | ask "write a commit message"
docker logs api 2>&1 | ask "why does it keep restarting?"
pbpaste | ask "pretty-print this JSON"
ps aux | ask "what's using the most memory?"
cat data.csv | ask                      # no prompt = auto-summarize
```

Interactive mode keeps context between prompts, so follow-ups just work:

```
ask [Downloads]> find dmg files older than a month
run> find . -name "*.dmg" -mtime +30?  [Y/n]  y
./OldInstaller.dmg

ask [Downloads]> now delete them
run> find . -name "*.dmg" -mtime +30 -delete?  [Y/n]
```

When a one-liner isn't enough, have it write you a reusable tool — reviewed
once, saved forever, run again by name with zero API calls:

```bash
$ ask tool new ports "list processes listening on tcp ports with pid and command"
--- ports (bash) ---
#!/bin/bash
lsof -iTCP -sTCP:LISTEN -P -n | awk 'NR>1 {print $2, $1, $9}' | sort -u
---
Save and approve this script? [y/N] y
Saved ~/.ask/tools/ports/main.sh — run it with: $ports <args>

$ ask '$ports'
run> bash '/Users/you/.ask/tools/ports/main.sh'
515 rapportd *:50761
7723 node *:3000
```

## Install

Requires the [Rust toolchain](https://rustup.rs) and an [OpenRouter](https://openrouter.ai) API key.

```bash
git clone <repository-url> && cd ask-cli
make install                # builds --release and copies to /usr/local/bin

export OPENROUTER_ASK_API_KEY="your-key"   # add to ~/.zshrc to persist
```

## Usage

Three modes, detected automatically:

```bash
ask "your request"          # single prompt: generate, confirm, run
ask                         # interactive session with context
some-command | ask "..."    # pipe mode: analyze the piped data (64 KB cap)
```

### Confirmation

Every generated command is shown before it runs: `Enter`/`y` executes, `n`
cancels. When a response contains several commands the prompt becomes
`[Y/n/s]`, where `s` skips just the current one.

`ask auto on` uses **Jev** to independently assess each eligible generated
command through OpenRouter, using your existing `OPENROUTER_ASK_API_KEY`.
Commands confidently assessed as read-only run without the prompt. Fresh checks
are additional API requests; they send only the command and shell type.

Confident read-only assessments are cached for seven days in
`~/.ask/jev-cache.json`, with a maximum of 512 entries. Exact command text,
the assessment prompt/model/thresholds, and the current directory, shell and
PATH contribute to a SHA-256 key. Only hashes, scores and timestamps are saved;
command text and API keys are not stored in the cache. Repeat commands with a
valid cache entry skip the Jev request. Local restrictions still apply on every
run. Uncertain or failed assessments are not cached; damaged or expired entries
trigger a fresh check. Delete the cache file to clear it.

Local policy limits this to supported inspection commands such as `ls`, `du`,
`ps`, `grep`, and `git status`. Writes, unknown programs, scripts, complex
shell syntax (including pipelines, quotes, and redirections), piped-data
sessions, and `$tool` lines still ask. Uncertain, malformed, or unavailable
assessments also fall back to confirmation, with a five-second request timeout.
Auto mode defaults to off; the setting persists, and `ask auto off` disables it.

Jev's confidence is evidence, not a guarantee. The current conservative cutoff
requires both read-only probability and confidence of at least 0.99. The
[classification fixture](tests/fixtures/command-safety.json) includes ordinary
reads, writes, hidden side effects, scripts, and injected instructions. Run
`cargo test live_jev_safety_evaluation -- --ignored --nocapture` to evaluate
the live model without executing any fixture commands. The integration uses
[OpenRouter's Decisions API](https://openrouter.ai/labs/jev/compile) and
[TypeSafe's Choice response format](https://docs.typesafe.ai/primitives/choice).

### Tool Library

`ask` can save the scripts it writes so they never need regenerating. Each
tool lives in `~/.ask/tools/<name>/` — a `main.sh` or `main.py` (bash and
Python 3 stdlib only) plus a manifest with its description and an approval
checksum.

```bash
ask tool new dedupe "find duplicate files by content hash in a directory"
ask '$dedupe ~/Downloads'         # run any time — no API call, works offline
ask tool improve dedupe "add a --delete flag that keeps the newest copy"

ask tool list                     # tools with lang, description, approval state
ask tool show dedupe              # print source and status
ask tool approve dedupe           # re-approve after reviewing an edited script
ask tool rm dedupe                # delete
```

Quote the `$` sigil in your shell (`ask '$dedupe ...'`); in interactive mode
no quoting is needed. Approved tools are also advertised to the LLM, so a
prompt like "clean up my downloads" may come back proposing `$dedupe
~/Downloads` — those lines always require confirmation, even in auto mode.

The safety model: you approve a script's exact source, and its SHA-256
(covering the interpreter too) is stored in the manifest. If the file changes
in any way, `ask` refuses to run it until you review and `tool approve` again.

### Interactive Mode

Common read-only commands (`ls`, `cd`, `cat`, `git status`, ...) execute
instantly without the LLM; anything containing shell metacharacters or
destructive flags goes through review instead. Handy extras:

| Command | Action |
|---------|--------|
| `q` / `exit` | Quit |
| `.` / `..` | `pwd` / `cd ..` |
| `clear` | Clear screen and reset conversation context |
| `finder` | Open Finder at the current directory |
| `auto on\|off`, `auto` | Toggle / show auto mode |
| `model [MODEL]` | Show or switch the LLM (`model reset` for the default) |
| `tool ...`, `$NAME args` | Tool library management and execution |

The session keeps conversation history as LLM context (auto-compacted as it
grows), including answers, command outcomes, and visible terminal output.
If a command fails, the next prompt includes its error and any earlier
successful steps. Skipped, cancelled, and unrun commands are recorded as such.
Long output is trimmed from the beginning to retain final diagnostics; terminal
capture stays in memory. The model can also answer questions conversationally —
replies prefixed with `#` are commentary, not commands.

## Configuration

Preferences live in `~/.ask/config` (simple `key=value`):

| Key | Values | Set by |
|-----|--------|--------|
| `theme` | `dark`, `light` | `ask --theme dark ...` |
| `model` | any OpenRouter model ID | `ask model <id>` (`ask model reset` to clear) |
| `auto` | `on`, `off` | `ask auto on\|off` |

`--model` overrides for a single run without persisting. The built-in default
model is `meta-llama/llama-3.3-70b-instruct`.

## Development

```bash
cargo build                 # dev build
cargo run -- "prompt"       # run without installing
make test                   # deterministic tests, no API calls
make test-live              # opt-in live OpenRouter contract tests (needs API key)
```

Built with Rust and powered by OpenRouter's AI models.
