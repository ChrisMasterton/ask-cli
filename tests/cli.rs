use std::process::{Command, Stdio};

fn ask_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ask"));
    command.stdin(Stdio::null());
    command
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
