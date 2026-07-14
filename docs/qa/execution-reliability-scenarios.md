# Command Execution Reliability Scenarios

### 1. An approved command must run exactly once in the visible terminal

- Mutator(s): AGAIN + Another WHERE
- Actors & states: User at a real macOS terminal; generated command awaiting confirmation
- Setup: Ask returns one benign command with a durable filesystem side effect
- Action: The user confirms once, including an accidental extra Enter keystroke
- Durable assertion: The side effect exists exactly once and the command receives terminal stdin/stdout
- Negative assertion: Ask does not merely print the command, execute it twice, or require manual copy/paste
- Suggested seam: Deterministic execution-flow unit test plus pseudo-terminal E2E smoke test

### 2. Stateful command steps must share the shell state they depend on

- Mutator(s): HALFWAY + Another WHERE
- Actors & states: User asking for a directory to be created, entered, and initialized
- Setup: The target directory does not exist
- Action: The user approves `mkdir`, `cd`, and a following command returned as one sequence
- Durable assertion: The final artifact is inside the new directory and interactive Ask remains in the final directory
- Negative assertion: No artifact is created in the original directory and quoted `&&` text is not split
- Suggested seam: Command-parser and subprocess integration tests

### 3. A failed command must stop the sequence and report failure

- Mutator(s): HALFWAY
- Actors & states: User approving a multi-command response whose first executable step fails
- Setup: A later command would create a detectable marker
- Action: The user approves the response
- Durable assertion: Ask returns an error for the failing exit status
- Negative assertion: Later commands do not run and history does not claim they ran
- Suggested seam: Injected execution-flow unit test plus shell short-circuit test

### 4. Skip and cancel must produce no command side effects

- Mutator(s): Another WHO + AGAIN
- Actors & states: User reviewing several generated commands
- Setup: Each command would create a distinct marker if executed
- Action: The user skips one command, approves one, then cancels the sequence
- Durable assertion: Only the approved command's marker exists
- Negative assertion: Skipped, cancelled, and not-yet-reached commands create nothing
- Suggested seam: Deterministic confirmation-flow unit test

### 5. Commands that require a TTY must behave as they do when pasted into zsh

- Mutator(s): Another WHERE
- Actors & states: User running Ask from an interactive terminal; command checks or consumes terminal I/O
- Setup: Stdin, stdout, and stderr are attached to a pseudo-terminal
- Action: The user approves a command that requires all three streams to be terminals
- Durable assertion: The command completes and creates its success marker
- Negative assertion: Ask does not replace terminal streams with closed or captured pipes
- Suggested seam: Pseudo-terminal E2E smoke test

### 6. Instructions embedded in piped data must remain data

- Mutator(s): Another WHO + LYING STATE
- Actors & states: User analyzing untrusted logs containing command-like prompt-injection text
- Setup: Piped text includes an instruction to run a destructive command
- Action: The user asks a conversational question about the data
- Durable assertion: The response is conversational and answers the user's request
- Negative assertion: No line from the piped data becomes an executable command
- Suggested seam: Live OpenRouter contract test

### 7. One-shot Ask must not imply it changed the calling shell's directory

- Mutator(s): Another WHERE + LYING STATE
- Actors & states: User invoking `ask "change directory"` from zsh rather than an interactive Ask session
- Setup: The generated command changes directory successfully inside Ask's child shell
- Action: The user approves the command and Ask exits
- Durable assertion: Ask explains which directory its command used and that the calling zsh remains unchanged
- Negative assertion: Ask does not silently claim that parent-shell-only state persisted
- Suggested seam: One-shot CLI integration test plus manual shell verification

Biggest blind spot: the `/dev/tty` confirmation and inherited-terminal path is manually proven but not yet automated in CI with a pseudo-terminal.
