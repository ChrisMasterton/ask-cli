use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn ask_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ask"));
    command.stdin(Stdio::null());
    command
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
