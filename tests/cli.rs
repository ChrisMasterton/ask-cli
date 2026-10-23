use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn ask_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ask"));
    command.stdin(Stdio::null());
    command
}

fn fake_home(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let home = std::env::temp_dir().join(format!("ask-{name}-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&home).expect("create fake home");
    home
}

/// Writes an approved bash tool into the fake home, computing the same
/// `lang + "\n" + script` checksum the binary uses.
fn seed_approved_tool(home: &Path, name: &str, script: &str) -> PathBuf {
    let dir = home.join(".ask").join("tools").join(name);
    std::fs::create_dir_all(&dir).expect("create tool dir");
    std::fs::write(dir.join("main.sh"), script).expect("write script");
    let mut hasher = Sha256::new();
    hasher.update(b"bash\n");
    hasher.update(script.as_bytes());
    let checksum = format!("{:x}", hasher.finalize());
    std::fs::write(
        dir.join("manifest"),
        format!("description=smoke test tool\nlang=bash\napproved_sha256={checksum}\n"),
    )
    .expect("write manifest");
    dir
}

#[test]
fn auto_toggle_persists_to_config_without_requiring_api_key() {
    // Point HOME at a scratch dir so the test never touches the real config.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let fake_home = std::env::temp_dir().join(format!("ask-auto-home-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&fake_home).expect("create fake home");

    let output = ask_command()
        .args(["auto", "on"])
        .env("HOME", &fake_home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask auto on");

    assert!(output.status.success(), "auto toggle must not need an API key");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Auto mode ON"));

    let config = std::fs::read_to_string(fake_home.join(".ask").join("config"))
        .expect("config file written");
    assert!(config.contains("auto=on"), "config should persist auto: {config}");

    let output = ask_command()
        .args(["auto", "off"])
        .env("HOME", &fake_home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask auto off");
    assert!(output.status.success());

    let config = std::fs::read_to_string(fake_home.join(".ask").join("config"))
        .expect("config file written");
    assert!(config.contains("auto=off"), "config should persist auto: {config}");

    let _ = std::fs::remove_dir_all(&fake_home);
}

#[test]
fn model_command_persists_shows_and_resets_without_api_key() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let fake_home =
        std::env::temp_dir().join(format!("ask-model-home-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&fake_home).expect("create fake home");

    // Set a model — persists without an API key.
    let output = ask_command()
        .args(["model", "openai/gpt-4o-mini"])
        .env("HOME", &fake_home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask model <id>");
    assert!(output.status.success(), "model set must not need an API key");
    assert!(String::from_utf8_lossy(&output.stdout).contains("saved as the default"));

    let config = std::fs::read_to_string(fake_home.join(".ask").join("config"))
        .expect("config file written");
    assert!(
        config.contains("model=openai/gpt-4o-mini"),
        "config should persist model: {config}"
    );

    // Show reports the saved model.
    let output = ask_command()
        .arg("model")
        .env("HOME", &fake_home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask model");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("openai/gpt-4o-mini"));

    // Reset removes the override.
    let output = ask_command()
        .args(["model", "reset"])
        .env("HOME", &fake_home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask model reset");
    assert!(output.status.success());

    let config = std::fs::read_to_string(fake_home.join(".ask").join("config"))
        .expect("config file written");
    assert!(
        !config.contains("model="),
        "reset should drop the model line: {config}"
    );

    let _ = std::fs::remove_dir_all(&fake_home);
}

#[test]
fn model_show_preserves_the_session_override_and_saved_default() {
    let home = fake_home("model-override");
    std::fs::create_dir_all(home.join(".ask")).expect("create config directory");
    let saved = "theme=light\nmodel=vendor/saved-model\nauto=off\n";
    std::fs::write(home.join(".ask/config"), saved).expect("write saved default");

    let output = ask_command()
        .args(["--model", "vendor/session-model", "model"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("show overridden model");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Model: vendor/session-model"),
        "show should report the active override: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".ask/config")).unwrap(),
        saved
    );

    let output = ask_command()
        .arg("model")
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("show persisted model in a new session");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Model: vendor/saved-model"));
    std::fs::remove_dir_all(home).expect("remove scratch home");
}

#[test]
fn preference_commands_report_failed_saves_without_claiming_success() {
    let home = fake_home("config-save-failure");
    let config = home.join(".ask/config");
    std::fs::create_dir_all(&config).expect("create a config path that cannot be overwritten");
    std::fs::write(config.join("sentinel"), "keep me").unwrap();
    for args in [
        vec!["model", "vendor/new-model"],
        vec!["model", "reset"],
        vec!["auto", "on"],
        vec!["auto", "off"],
    ] {
        let output = ask_command()
            .args(&args)
            .env("HOME", &home)
            .env_remove("OPENROUTER_ASK_API_KEY")
            .output()
            .expect("run preference command with a blocked save");
        assert!(
            !output.status.success(),
            "ask {args:?} must report a failed preference save"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("saved as the default")
                && !stdout.contains("Model reset")
                && !stdout.contains("Auto mode ON")
                && !stdout.contains("Auto mode OFF"),
            "ask {args:?} claimed success: {stdout}"
        );
        assert!(
            !output.stderr.is_empty(),
            "a save failure must explain the error"
        );
        assert_eq!(
            std::fs::read_to_string(config.join("sentinel")).unwrap(),
            "keep me"
        );
    }
    std::fs::remove_dir_all(home).expect("remove scratch home");
}

#[test]
fn preference_commands_require_a_home_to_save() {
    for args in [vec!["model", "vendor/new-model"], vec!["auto", "on"]] {
        let output = ask_command()
            .args(&args)
            .env_remove("HOME")
            .env_remove("OPENROUTER_ASK_API_KEY")
            .output()
            .expect("run preference command without HOME");
        assert!(!output.status.success(), "ask {args:?} has nowhere to save");
        assert!(String::from_utf8_lossy(&output.stderr).contains("HOME"));
    }
}

#[test]
fn help_exits_successfully_without_an_api_key() {
    let output = ask_command()
        .arg("--help")
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("Command confirmation options:"));
}

#[test]
fn local_commands_do_not_wait_for_piped_input_to_close() {
    let home = fake_home("startup-stdin");
    for (args, success, expected) in [
        (vec!["--help"], true, "Usage:"),
        (vec!["--model"], false, "--model requires a value"),
        (vec!["model"], true, "Model:"),
        (vec!["tool", "list"], true, "No saved tools"),
    ] {
        let mut child = ask_command()
            .args(&args)
            .env("HOME", &home)
            .env_remove("OPENROUTER_ASK_API_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn local command with an open stdin pipe");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll local command") {
                break Some(status);
            }
            if std::time::Instant::now() >= deadline {
                child.kill().expect("stop command blocked on stdin");
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let output = child.wait_with_output().expect("collect local command");
        assert!(status.is_some(), "ask {args:?} waited for stdin EOF");
        assert_eq!(status.unwrap().success(), success);
        let result = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(result.contains(expected), "ask {args:?}: {result}");
    }
    std::fs::remove_dir_all(home).expect("remove scratch home");
}

#[test]
fn missing_model_value_is_reported_before_api_startup() {
    let output = ask_command()
        .arg("--model")
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask with missing model");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--model requires a value"));
}

#[test]
fn invalid_theme_is_rejected() {
    let output = ask_command()
        .args(["--theme", "sepia"])
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask with invalid theme");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid theme"));
}

#[test]
fn a_prompt_requires_the_api_key() {
    let output = ask_command()
        .arg("list files")
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask without API key");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("OPENROUTER_ASK_API_KEY"));
}

#[test]
fn tool_list_works_keyless_and_hints_at_creation_when_empty() {
    let home = fake_home("tool-empty");

    let output = ask_command()
        .args(["tool", "list"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool list");

    assert!(output.status.success(), "tool list must not need an API key");
    assert!(String::from_utf8_lossy(&output.stdout).contains("tool new"));

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn approved_tool_lists_shows_and_runs_without_an_api_key() {
    let home = fake_home("tool-run");
    seed_approved_tool(&home, "hello", "echo hello-from-tool\n");

    let output = ask_command()
        .args(["tool", "list"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool list");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("$hello"));

    let output = ask_command()
        .args(["tool", "show", "hello"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool show");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("echo hello-from-tool"), "show prints source: {stdout}");
    assert!(stdout.contains("approved"));

    let output = ask_command()
        .arg("$hello")
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask '$hello'");
    assert!(output.status.success(), "approved tool must run keyless");
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello-from-tool"));

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn tool_output_streams_before_the_command_finishes() {
    use std::io::BufRead;
    let home = fake_home("tool-stream");
    seed_approved_tool(
        &home,
        "stream",
        "printf 'streamed-stdout\\n'\nprintf 'streamed-stderr\\n' >&2\nwhile [ ! -f \"$HOME/release\" ]; do sleep 0.01; done\nprintf 'finished\\n'\n",
    );
    let mut child = ask_command()
        .arg("$stream")
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start streaming tool");
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let read_stream = |reader: Box<dyn std::io::Read + Send>,
                       sender: std::sync::mpsc::Sender<String>| {
        std::thread::spawn(move || {
            let mut output = String::new();
            for line in std::io::BufReader::new(reader).lines() {
                let line = line.expect("read command stream");
                if line.starts_with("streamed-") {
                    sender.send(line.clone()).unwrap();
                }
                output.push_str(&line);
                output.push('\n');
            }
            output
        })
    };
    let stdout_reader = read_stream(Box::new(stdout), sender.clone());
    let stderr_reader = read_stream(Box::new(stderr), sender);
    let mut observed = Vec::new();
    for _ in 0..2 {
        if let Ok(line) = receiver.recv_timeout(std::time::Duration::from_secs(2)) {
            observed.push(line);
        } else {
            break;
        }
    }
    let was_running = child.try_wait().unwrap().is_none();
    // Release the tool before asserting, including when the old buffering bug occurs.
    std::fs::write(home.join("release"), "done").unwrap();
    let status = child.wait().unwrap();
    let stdout = stdout_reader.join().unwrap();
    let stderr = stderr_reader.join().unwrap();
    std::fs::remove_dir_all(home).unwrap();
    observed.sort();
    assert_eq!(observed, ["streamed-stderr", "streamed-stdout"]);
    assert!(
        was_running,
        "both streams should arrive before the tool exits"
    );
    assert!(status.success(), "{stderr}");
    assert!(stdout.contains("finished"));
}

#[test]
fn tampered_tool_refuses_to_run_until_reapproved_and_rm_deletes_it() {
    let home = fake_home("tool-tamper");
    let dir = seed_approved_tool(&home, "hello", "echo hello-from-tool\n");
    std::fs::write(dir.join("main.sh"), "echo tampered\n").expect("tamper with script");

    let output = ask_command()
        .arg("$hello")
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask '$hello'");
    assert!(!output.status.success(), "tampered tool must not run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("tool approve"), "error should guide re-approval: {stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("tampered"),
        "the tampered script must not have executed"
    );

    let output = ask_command()
        .args(["tool", "rm", "hello"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool rm");
    assert!(output.status.success());
    assert!(!dir.exists(), "tool rm should delete the directory");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn tool_new_requires_the_api_key() {
    let home = fake_home("tool-new-keyless");

    let output = ask_command()
        .args(["tool", "new", "widget", "do", "something"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool new without key");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("OPENROUTER_ASK_API_KEY"));
    assert!(
        !home.join(".ask").join("tools").join("widget").exists(),
        "nothing may be written without a generated, approved script"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn tool_names_with_path_traversal_are_rejected() {
    let home = fake_home("tool-traversal");
    std::fs::write(home.join("sentinel"), "keep me").expect("write sentinel");

    let output = ask_command()
        .args(["tool", "show", "../config"])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool show ../config");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid tool name"));

    let output = ask_command()
        .args(["tool", "rm", ".."])
        .env("HOME", &home)
        .env_remove("OPENROUTER_ASK_API_KEY")
        .output()
        .expect("run ask tool rm ..");
    assert!(!output.status.success());
    assert!(home.join("sentinel").exists(), "nothing outside tools/ may be touched");

    let _ = std::fs::remove_dir_all(&home);
}
