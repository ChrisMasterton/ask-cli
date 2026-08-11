use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static CURRENT_DIR_TEST_LOCK: Mutex<()> = Mutex::new(());

struct CurrentDirGuard(PathBuf);

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.0);
    }
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ask-{name}-{}-{nonce}", std::process::id()))
}

#[test]
fn normalize_confirmation_input_strips_ansi_sequences() {
    let input = "\u{1b}[?2004lyes\u{1b}[?2004h\n";
    assert_eq!(normalize_confirmation_input(input), "yes");
}

#[test]
fn normalize_confirmation_input_keeps_valid_option() {
    assert_eq!(normalize_confirmation_input("  s  \r\n"), "s");
}

#[test]
fn parse_confirmation_choice_treats_escaped_yes_as_yes() {
    let input = "\u{1b}[?2004ly\u{1b}[?2004h\n";
    assert_eq!(parse_confirmation_choice(input), Some(ConfirmChoice::Yes));
}

#[test]
fn parse_confirmation_choice_supports_all_options() {
    assert_eq!(parse_confirmation_choice("n"), Some(ConfirmChoice::No));
    assert_eq!(parse_confirmation_choice("skip"), Some(ConfirmChoice::Skip));
    assert_eq!(
        parse_confirmation_choice("i"),
        Some(ConfirmChoice::Instruct)
    );
    assert_eq!(parse_confirmation_choice("maybe"), None);
}

#[test]
fn confirmed_commands_execute_exactly_once_and_comments_never_execute() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut confirmations = VecDeque::from([ConfirmResponse::Yes]);
    let mut confirmed = Vec::new();
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec!["# explanation".to_string(), "touch marker".to_string()],
        &theme,
        false,
        |command, _| {
            confirmed.push(command.to_string());
            Ok(confirmations.pop_front().expect("confirmation response"))
        },
        |command| {
            executed.push(command.to_string());
            Ok("created marker".to_string())
        },
    )
    .expect("command flow should succeed");

    assert_eq!(confirmed, vec!["touch marker"]);
    assert_eq!(executed, vec!["touch marker"]);
    assert_eq!(result.0, vec!["touch marker"]);
    assert_eq!(result.1, vec!["created marker"]);
}

#[test]
fn skip_and_cancel_never_execute_the_rejected_commands() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut confirmations = VecDeque::from([
        ConfirmResponse::Skip,
        ConfirmResponse::Yes,
        ConfirmResponse::No,
    ]);
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec![
            "skip-me".to_string(),
            "run-me".to_string(),
            "cancel-me".to_string(),
            "never-reached".to_string(),
        ],
        &theme,
        false,
        |_, _| Ok(confirmations.pop_front().expect("confirmation response")),
        |command| {
            executed.push(command.to_string());
            Ok(format!("output:{command}"))
        },
    )
    .expect("skip/cancel flow should succeed");

    assert_eq!(executed, vec!["run-me"]);
    assert_eq!(result.0, vec!["run-me"]);
    assert_eq!(result.1, vec!["output:run-me"]);
}

#[test]
fn failed_execution_stops_the_flow_without_claiming_later_commands_ran() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec!["fails".to_string(), "must-not-run".to_string()],
        &theme,
        false,
        |_, _| Ok(ConfirmResponse::Yes),
        |command| {
            executed.push(command.to_string());
            Err("simulated command failure".into())
        },
    );

    assert!(result.is_err());
    assert_eq!(executed, vec!["fails"]);
}

#[test]
fn instruct_runs_custom_command_then_original_once_after_reconfirmation() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut confirmations = VecDeque::from([
        ConfirmResponse::Instruct("pwd".to_string()),
        ConfirmResponse::Yes,
    ]);
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec!["rm old-file".to_string()],
        &theme,
        false,
        |_, _| Ok(confirmations.pop_front().expect("confirmation response")),
        |command| {
            executed.push(command.to_string());
            Ok(format!("output:{command}"))
        },
    )
    .expect("instruct flow should succeed");

    assert_eq!(executed, vec!["pwd", "rm old-file"]);
    assert_eq!(result.0, vec!["rm old-file"]);
    assert_eq!(result.1, vec!["output:rm old-file"]);
}

#[test]
fn parse_commands_preserves_chained_commands() {
    let input = "mkdir myproject && cd myproject && git init";
    let commands = super::parse_commands(input);
    assert_eq!(
        commands,
        vec!["mkdir myproject && cd myproject && git init"]
    );
}

#[test]
fn parsed_command_sequence_preserves_cd_for_following_commands() {
    let _lock = CURRENT_DIR_TEST_LOCK
        .lock()
        .expect("current-dir test lock poisoned");
    let original_dir = env::current_dir().expect("current directory");
    let temp_dir = unique_temp_dir("generated-cd");
    let _cleanup = TempDirGuard(temp_dir.clone());
    let _restore_dir = CurrentDirGuard(original_dir);
    fs::create_dir_all(&temp_dir).expect("create temp directory");
    env::set_current_dir(&temp_dir).expect("enter temp directory");

    let commands = parse_commands("mkdir project && cd project && touch marker");
    for command in commands {
        run_command_with_output(&command).expect("generated command should execute");
    }

    assert!(
        temp_dir.join("project/marker").is_file(),
        "a successful generated cd must affect the following generated command"
    );
    assert!(
        !temp_dir.join("marker").exists(),
        "the following command must not silently run in the old directory"
    );
}

#[test]
fn parse_commands_does_not_split_and_and_inside_quotes() {
    let commands = parse_commands("printf '%s\\n' 'one && two'");
    assert_eq!(commands, vec!["printf '%s\\n' 'one && two'"]);
}

#[test]
fn parse_commands_preserves_comment_lines() {
    let input = "# This will create a directory && init git\nmkdir foo && cd foo";
    let commands = super::parse_commands(input);
    assert_eq!(
        commands,
        vec![
            "# This will create a directory && init git",
            "mkdir foo && cd foo",
        ]
    );
}

#[test]
fn command_execution_persists_a_standalone_cd() {
    let _lock = CURRENT_DIR_TEST_LOCK
        .lock()
        .expect("current-dir test lock poisoned");
    let original_dir = env::current_dir().expect("current directory");
    let temp_dir = unique_temp_dir("standalone-cd");
    let _cleanup = TempDirGuard(temp_dir.clone());
    let _restore_dir = CurrentDirGuard(original_dir);
    let nested_dir = temp_dir.join("nested");
    fs::create_dir_all(&nested_dir).expect("create nested temp directory");

    run_command_with_output(&format!("cd {}", shell_quote(&nested_dir)))
        .expect("generated cd should execute");

    assert_eq!(
        env::current_dir().expect("current directory"),
        nested_dir
            .canonicalize()
            .expect("canonical nested directory")
    );
}

#[test]
fn command_execution_preserves_and_and_short_circuiting() {
    let _lock = CURRENT_DIR_TEST_LOCK
        .lock()
        .expect("current-dir test lock poisoned");
    let original_dir = env::current_dir().expect("current directory");
    let temp_dir = unique_temp_dir("short-circuit");
    let _cleanup = TempDirGuard(temp_dir.clone());
    let _restore_dir = CurrentDirGuard(original_dir);
    fs::create_dir_all(&temp_dir).expect("create temp directory");
    env::set_current_dir(&temp_dir).expect("enter temp directory");

    let result = run_command_with_output("false && touch should-not-exist");

    assert!(result.is_err(), "the failed shell chain must be reported");
    assert!(!temp_dir.join("should-not-exist").exists());
}

#[test]
fn command_execution_returns_stdout_and_stderr_for_history() {
    let output =
        run_command_with_output("printf out; printf err >&2").expect("command should execute");
    assert!(output.contains("out"));
    assert!(output.contains("err"));
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[test]
fn parse_commands_strips_code_fences() {
    let input = "```bash\nls -la\n```";
    assert_eq!(parse_commands(input), vec!["ls -la"]);
}

#[test]
fn parse_commands_filters_blank_lines_and_trims() {
    let input = "  ls -la  \n\n   \npwd";
    assert_eq!(parse_commands(input), vec!["ls -la", "pwd"]);
}

// --- is_safe_direct_command: the whitelist that auto-executes WITHOUT confirmation ---

#[test]
fn safe_direct_command_allows_read_only_commands() {
    for cmd in [
        "ls",
        "ls -la",
        "cd ..",
        "cat /etc/hosts",
        "pwd",
        "echo hi",
        "head -n 5 file",
        "tail file",
        "grep foo bar.txt",
        "find . -name x",
        "wc -l file",
        "git status",
        "git log",
    ] {
        assert!(is_safe_direct_command(cmd), "expected safe: {cmd}");
    }
}

#[test]
fn safe_direct_command_is_case_insensitive_for_exact_matches() {
    assert!(is_safe_direct_command("GIT STATUS"));
    assert!(is_safe_direct_command("PWD"));
}

// Documents a quirk: git/brew/npm/pip entries are matched as exact strings,
// so adding ANY argument falls through to requiring confirmation. This is the
// conservative direction (err toward asking), but worth pinning down.
#[test]
fn safe_direct_command_git_with_args_requires_confirmation() {
    assert!(!is_safe_direct_command("git log --oneline"));
    assert!(!is_safe_direct_command("git diff HEAD"));
    assert!(!is_safe_direct_command("brew list --versions"));
}

#[test]
fn safe_direct_command_rejects_destructive_or_unknown_commands() {
    for cmd in [
        "rm -rf /",
        "sudo rm -rf /",
        "git push",
        "mv a b",
        "dd if=/dev/zero of=/dev/disk2",
        "chmod 777 /",
        "kill -9 1",
    ] {
        assert!(!is_safe_direct_command(cmd), "expected NOT safe: {cmd}");
    }
}

// --- is_script_execution ---

#[test]
fn script_execution_detects_interpreters_and_relative_paths() {
    for cmd in [
        "python script.py",
        "python3 a.py",
        "node app.js",
        "bash deploy.sh",
        "./run.sh",
    ] {
        assert!(is_script_execution(cmd), "expected script: {cmd}");
    }
}

#[test]
fn script_execution_detects_by_extension() {
    assert!(is_script_execution("myscript.py"));
    assert!(is_script_execution("deploy.sh"));
}

// Compiled-language sources have no interpreter that can run them directly;
// treating them as "scripts" only produced a shell error after skipping
// confirmation, so they must not match.
#[test]
fn compiled_source_files_are_not_treated_as_scripts() {
    for cmd in ["build.rs", "main.go", "App.java", "lib.ts", "Main.kt"] {
        assert!(!is_script_execution(cmd), "expected not runnable: {cmd}");
        assert!(!is_safe_direct_command(cmd), "must not auto-execute: {cmd}");
    }
}

#[test]
fn script_execution_ignores_plain_commands() {
    for cmd in ["ls -la", "git status", "cat notes.md", "make build"] {
        assert!(!is_script_execution(cmd), "expected not a script: {cmd}");
    }
}

// Regression: a destructive verb whose argument ends in a script extension
// (e.g. `rm build.sh`) must NOT be treated as script execution, otherwise it
// would slip into the auto-execute whitelist and skip confirmation.
#[test]
fn script_execution_does_not_match_verb_with_script_arg() {
    for cmd in [
        "rm build.sh",
        "rm -rf build.sh",
        "rm notes.py",
        "rm config.rs",
        "mv a.js b",
    ] {
        assert!(
            !is_script_execution(cmd),
            "must require confirmation: {cmd}"
        );
    }
    // ...but a bare script path still counts.
    assert!(is_script_execution("deploy.sh"));
}

#[test]
fn safe_direct_command_does_not_whitelist_rm_of_script_files() {
    for cmd in [
        "rm build.sh",
        "rm -rf build.sh",
        "rm notes.py",
        "rm config.rs",
    ] {
        assert!(
            !is_safe_direct_command(cmd),
            "rm of a script file must require confirmation: {cmd}"
        );
    }
}

// Regression: the whitelist matched prefixes but handed the WHOLE line to
// `$SHELL -c`, so anything after `;`, `&&`, `|`, `>`, `$(…)` or a backtick
// rode along with no confirmation (e.g. `echo hi > f; rm -rf ~`).
#[test]
fn safe_direct_command_rejects_shell_metacharacters() {
    for cmd in [
        "echo hi > marker.txt",
        "echo hi; rm -rf ~",
        "ls -la && rm -rf ~",
        "cat notes.txt | sh",
        "cat `whoami`.txt",
        "echo $(rm -rf ~)",
        "pwd;rm file",
        "ls $(pwd)",
        "wc -l < input.txt",
        "ls &",
        "cd /tmp && rm -rf .",
        "grep foo bar.txt > out.txt",
    ] {
        assert!(!is_safe_direct_command(cmd), "must require confirmation: {cmd}");
    }
}

// Regression: `bash -c` / `python -c` matched the interpreter prefix and
// auto-ran arbitrary inline code without confirmation.
#[test]
fn script_execution_rejects_inline_interpreter_code_and_chains() {
    for cmd in [
        "bash -c \"echo pwned\"",
        "sh -c ls",
        "zsh -c 'rm x'",
        "python -c 'print(1)'",
        "python3 -m http.server",
        "node -e 'process.exit()'",
        "bash deploy.sh; rm -rf ~",
        "./run.sh && rm -rf ~",
    ] {
        assert!(!is_script_execution(cmd), "expected not a script: {cmd}");
        assert!(!is_safe_direct_command(cmd), "must require confirmation: {cmd}");
    }
}

// `find` can destroy files through its own flags without any shell
// metacharacters, so those variants must fall back to confirmation.
#[test]
fn find_destructive_flags_require_confirmation() {
    assert!(!is_safe_direct_command("find . -delete"));
    assert!(!is_safe_direct_command("find . -name x -exec rm {} +"));
    assert!(is_safe_direct_command("find . -name x"));
}

#[test]
fn interpreter_for_script_maps_known_extensions_only() {
    assert_eq!(interpreter_for_script("deploy.sh"), Some("bash"));
    assert_eq!(interpreter_for_script("app.py"), Some("python3"));
    assert_eq!(interpreter_for_script("./run.sh"), Some("bash"));
    assert_eq!(interpreter_for_script("noext"), None);
    assert_eq!(interpreter_for_script(".sh"), None);
    assert_eq!(interpreter_for_script("build.rs"), None);
}

// --- parse_confirmation_choice edge cases ---

#[test]
fn confirmation_empty_input_defaults_to_yes() {
    assert_eq!(parse_confirmation_choice(""), Some(ConfirmChoice::Yes));
}

#[test]
fn confirmation_is_case_insensitive_and_trims() {
    assert_eq!(parse_confirmation_choice("YES"), Some(ConfirmChoice::Yes));
    assert_eq!(parse_confirmation_choice("  No  "), Some(ConfirmChoice::No));
}

// --- token estimation ---

#[test]
fn estimate_tokens_uses_four_chars_per_token() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
}

#[test]
fn estimate_context_tokens_caps_each_output_at_500_chars() {
    let history = vec![ConversationContext {
        prompt: "abcde".to_string(),       // 5
        commands: vec!["xyz".to_string()], // 3
        outputs: vec!["o".repeat(1000)],   // capped at 500
    }];
    assert_eq!(
        estimate_context_tokens(&history),
        (5 + 3 + 500) / TOKEN_ESTIMATE_RATIO
    );
}

#[test]
fn truncate_utf8_bytes_never_splits_a_character() {
    let value = "a".repeat(199) + "🚀tail";
    let (prefix, truncated) = truncate_utf8_bytes(&value, 200);
    assert!(truncated);
    assert_eq!(prefix, "a".repeat(199));
}

#[test]
fn normal_prompt_requests_state_dependent_commands_as_one_chain() {
    let prompt = build_prompt("create and enter a directory", None);
    assert!(
        prompt
            .contains("Keep state-dependent steps such as `cd` or `export` in one `&&` chain")
    );
    assert!(prompt.contains("**User request:** create and enter a directory"));
}

#[test]
fn piped_prompt_includes_request_and_unicode_data_without_panicking() {
    let data = "🚀".repeat((MAX_PIPE_BYTES / 4) + 1);
    let prompt = build_prompt("summarize", Some(&data));
    assert!(prompt.contains("**User request:** summarize"));
    assert!(prompt.contains("truncated"));
    assert!(prompt.contains("---BEGIN PIPED DATA---"));
}

#[test]
fn api_response_requires_a_choice_and_nonempty_content() {
    let no_choice = commands_from_api_response(ApiResponse { choices: vec![] });
    assert!(no_choice.is_err());

    let empty_content = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "  \n".to_string(),
            },
        }],
    });
    assert!(empty_content.is_err());
}

#[test]
fn api_response_is_parsed_into_comments_and_intact_shell_lines() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "SAFE: no\n# Set up the repo\nmkdir app && cd app && git init"
                    .to_string(),
            },
        }],
    })
    .expect("valid model response");

    assert_eq!(
        reply.commands,
        vec!["# Set up the repo", "mkdir app && cd app && git init"]
    );
    assert!(!reply.safe);
}

// --- safety verdict and auto mode ---

#[test]
fn safety_marker_is_parsed_and_stripped() {
    let (verdict, rest) = extract_safety_marker("SAFE: yes\nls -la");
    assert_eq!(verdict, Some(true));
    assert_eq!(rest, "ls -la");

    let (verdict, rest) = extract_safety_marker("safe: NO\nrm -rf build");
    assert_eq!(verdict, Some(false));
    assert_eq!(rest, "rm -rf build");
}

#[test]
fn safety_marker_tolerates_decoration_and_fences() {
    let (verdict, _) = extract_safety_marker("# SAFE: yes\nls");
    assert_eq!(verdict, Some(true));

    let (verdict, _) = extract_safety_marker("**SAFE: no**\nrm x");
    assert_eq!(verdict, Some(false));

    let (verdict, rest) = extract_safety_marker("```\nSAFE: no\nrm x\n```");
    assert_eq!(verdict, Some(false));
    assert!(rest.contains("rm x"));
}

#[test]
fn missing_or_malformed_marker_yields_no_verdict() {
    assert_eq!(extract_safety_marker("ls -la").0, None);
    assert_eq!(extract_safety_marker("SAFE: maybe\nls").0, None);
    // Only the FIRST meaningful line counts as a verdict.
    assert_eq!(extract_safety_marker("# hello\nSAFE: yes\nls").0, None);
}

#[test]
fn reply_without_verdict_defaults_to_requiring_confirmation() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "mkdir app".to_string(),
            },
        }],
    })
    .expect("valid response");
    assert!(!reply.safe, "a missing verdict must be treated as unsafe");
}

#[test]
fn verdict_yes_marks_reply_safe() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "SAFE: yes\nls -la".to_string(),
            },
        }],
    })
    .expect("valid response");
    assert!(reply.safe);
    assert_eq!(reply.commands, vec!["ls -la"]);
}

#[test]
fn conversational_reply_without_commands_is_always_safe() {
    let reply = commands_from_api_response(ApiResponse {
        choices: vec![Choice {
            message: Message {
                content: "# hello there!".to_string(),
            },
        }],
    })
    .expect("valid response");
    assert!(reply.safe, "a reply with no commands defaults to safe");
}

#[test]
fn auto_mode_executes_safe_commands_without_confirmation() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec!["# listing files".to_string(), "ls -la".to_string()],
        &theme,
        true,
        |_, _| -> Result<ConfirmResponse, io::Error> {
            panic!("auto mode must not prompt for a safe command")
        },
        |command| {
            executed.push(command.to_string());
            Ok("ok".to_string())
        },
    )
    .expect("auto flow should succeed");

    assert_eq!(executed, vec!["ls -la"]);
    assert_eq!(result.0, vec!["ls -la"]);
}

#[test]
fn auto_mode_still_confirms_denylisted_commands() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let mut confirmed = Vec::new();
    let mut executed = Vec::new();

    let result = execute_commands_with(
        vec!["rm -rf ./build".to_string()],
        &theme,
        true,
        |command, _| {
            confirmed.push(command.to_string());
            Ok(ConfirmResponse::No)
        },
        |command| {
            executed.push(command.to_string());
            Ok(String::new())
        },
    )
    .expect("deny-listed flow should succeed");

    assert_eq!(confirmed, vec!["rm -rf ./build"]);
    assert!(executed.is_empty());
    assert!(result.0.is_empty());
}

#[test]
fn deny_list_blocks_dangerous_commands_from_auto_execution() {
    for cmd in [
        "rm -rf /",
        "sudo shutdown -h now",
        "echo done && rm cache.txt",
        "dd if=/dev/zero of=/dev/disk2",
        "kill $(lsof -t -i :3000)",
        "mkfs.ext4 /dev/sdb1",
    ] {
        assert!(never_auto_execute(cmd), "must never auto-run: {cmd}");
    }
    for cmd in ["ls -la", "git status", "du -sh * | sort -rh"] {
        assert!(!never_auto_execute(cmd), "safe to auto-run: {cmd}");
    }
}

#[test]
fn auto_toggle_parses_only_exact_commands() {
    assert_eq!(parse_auto_toggle("auto on"), Some(true));
    assert_eq!(parse_auto_toggle("  AUTO OFF "), Some(false));
    assert_eq!(parse_auto_toggle("auto"), None);
    assert_eq!(parse_auto_toggle("automate everything"), None);
}

#[test]
fn on_off_values_parse_case_insensitively() {
    assert_eq!(parse_on_off("on"), Some(true));
    assert_eq!(parse_on_off("TRUE"), Some(true));
    assert_eq!(parse_on_off("off"), Some(false));
    assert_eq!(parse_on_off("0"), Some(false));
    assert_eq!(parse_on_off("sometimes"), None);
}

// --- compact_history ---

#[test]
fn compact_history_empty_returns_header_only() {
    let out = compact_history(&[]);
    assert!(out.contains("Previous commands and outputs"));
    assert!(!out.contains("(Note: Showing recent"));
}

#[test]
fn compact_history_keeps_chronological_order_for_small_history() {
    let history = vec![
        ConversationContext {
            prompt: "first-prompt".to_string(),
            commands: vec!["ls".to_string()],
            outputs: vec![],
        },
        ConversationContext {
            prompt: "second-prompt".to_string(),
            commands: vec!["pwd".to_string()],
            outputs: vec![],
        },
    ];
    let out = compact_history(&history);
    let first = out.find("first-prompt").expect("first present");
    let second = out.find("second-prompt").expect("second present");
    assert!(first < second, "expected chronological order");
    assert!(!out.contains("(Note: Showing recent"));
}

#[test]
fn compact_history_truncates_when_over_token_budget() {
    let history: Vec<ConversationContext> = (0..40)
        .map(|_| ConversationContext {
            prompt: "p".repeat(1000),
            commands: vec![],
            outputs: vec![],
        })
        .collect();
    let out = compact_history(&history);
    assert!(
        out.contains("(Note: Showing recent"),
        "expected truncation note"
    );
    assert!(
        estimate_tokens(&out) <= MAX_CONTEXT_TOKENS,
        "compacted output must respect budget"
    );
}

#[test]
fn compact_history_handles_unicode_at_the_truncation_boundary() {
    let history = vec![ConversationContext {
        prompt: "show output".to_string(),
        commands: vec!["printf".to_string()],
        outputs: vec!["a".repeat(199) + "🚀" + &"b".repeat(20)],
    }];

    let compacted = compact_history(&history);

    assert!(compacted.contains("... (truncated)"));
}

// --- ThemeMode ---

#[test]
fn theme_mode_parses_case_insensitively_and_rejects_unknown() {
    assert!(matches!(
        ThemeMode::from_str("light"),
        Some(ThemeMode::Light)
    ));
    assert!(matches!(ThemeMode::from_str("DARK"), Some(ThemeMode::Dark)));
    assert!(ThemeMode::from_str("blue").is_none());
}

#[test]
fn theme_mode_str_roundtrips() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        assert_eq!(
            ThemeMode::from_str(mode.as_str()).unwrap().as_str(),
            mode.as_str()
        );
    }
}

#[test]
fn theme_wraps_text_with_color_and_reset() {
    let theme = Theme::from_mode(ThemeMode::Dark);
    let painted = theme.helper_text("hello");
    assert!(painted.contains("hello"));
    assert!(painted.ends_with(RESET));
    assert!(painted.starts_with(theme.helper_color));
}
