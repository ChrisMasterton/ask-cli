# Command confirmation

Generated commands ask for confirmation before execution unless eligible for
automatic execution under the saved auto-mode setting.

- **Enter, y, or yes** runs the command.
- **n or no** cancels execution of the remaining commands. Interactive mode
  returns to its prompt.
- **s or skip** skips the current command and continues with the next one.
  The prompt displays the skip option when there are multiple executable commands.

For example:

```text
run> ls -la?  [Y/n/s]  s
Skipping command: ls -la
run> echo "Hello World"?  [Y/n/s]  y
Hello World
```

Invalid command responses show a hint and ask again. Closing input without an
answer fails confirmation; it does not count as pressing Enter. Cancellation,
command failure, or confirmation failure stops later executable commands, while
commentary from the response can still display.

Saved-tool approval uses a separate policy: only an explicit **y** or **yes**
approves the reviewed source. Enter and other responses decline without retrying;
EOF without input reports a confirmation error.
