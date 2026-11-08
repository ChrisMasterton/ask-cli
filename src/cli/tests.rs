use super::*;

fn parse(values: &[&str], config: Config) -> Result<ArgumentOutcome, Box<dyn std::error::Error>> {
    parse_args(values.iter().map(|value| value.to_string()), config)
}

fn run(values: &[&str], config: Config) -> (Args, Option<Config>) {
    match parse(values, config).unwrap() {
        ArgumentOutcome::Run {
            args,
            theme_preference,
        } => (args, theme_preference),
        ArgumentOutcome::Help => panic!("expected an invocation"),
    }
}

#[test]
fn overrides_follow_argument_order_and_only_theme_requests_persistence() {
    let saved = Config {
        model: Some("vendor/saved".to_string()),
        auto: true,
        ..Config::default()
    };
    let (args, preference) = run(
        &[
            "first",
            "--model",
            "vendor/first",
            "--theme",
            "light",
            "second",
            "--model",
            "vendor/last",
            "--theme",
            "DARK",
        ],
        saved.clone(),
    );
    assert_eq!(
        args,
        Args {
            prompt: Some("first second".to_string()),
            model: "vendor/last".to_string(),
            theme: ThemeMode::Dark,
            auto: true,
        }
    );
    // Even an override equal to the saved theme requests a save. The model
    // override is absent from the saved config, and auto is retained.
    assert_eq!(preference, Some(saved.clone()));
    let (args, preference) = run(&["--model", "vendor/session"], saved);
    assert_eq!(args.model, "vendor/session");
    assert_eq!(preference, None);
}

#[test]
fn terminator_and_tool_invocations_claim_all_remaining_argv() {
    let cases = [
        (
            vec!["--", "--theme", "light", "--help"],
            "--theme light --help",
        ),
        (
            vec!["$echo", "--help", "two words", "", "$HOME"],
            "$echo '--help' 'two words' '' '$HOME'",
        ),
        (
            vec!["--", "$echo", "--theme", "light"],
            "$echo '--theme' 'light'",
        ),
        (
            vec!["$echo 'embedded words'", "a'b"],
            "$echo 'embedded words' 'a'\\''b'",
        ),
    ];
    for (values, prompt) in cases {
        let (args, preference) = run(&values, Config::default());
        assert_eq!(args.prompt.as_deref(), Some(prompt), "{values:?}");
        assert_eq!(preference, None);
    }
}

#[test]
fn help_and_errors_do_not_commit_partial_theme_overrides() {
    // A terminal outcome stops consuming argv, including an invalid later option.
    let mut values = ["--theme", "light", "--help", "--theme", "bad"]
        .into_iter()
        .map(str::to_string);
    assert!(matches!(
        parse_args(&mut values, Config::default()).unwrap(),
        ArgumentOutcome::Help
    ));
    assert_eq!(values.next().as_deref(), Some("--theme"));
    for (values, error) in [
        (
            vec!["--theme", "light", "--model"],
            "--model requires a value",
        ),
        (
            vec!["--theme", "light", "--theme"],
            "--theme requires a value",
        ),
        (
            vec!["--theme", "light", "--theme", " light "],
            "Invalid theme. Use 'light' or 'dark'.",
        ),
    ] {
        assert_eq!(
            parse(&values, Config::default()).unwrap_err().to_string(),
            error
        );
    }
}

#[test]
fn option_values_are_unconditional_and_unknown_options_are_prompt_text() {
    let (args, preference) = run(&["--model", "--help", "--unknown"], Config::default());
    assert_eq!(args.model, "--help");
    assert_eq!(args.prompt.as_deref(), Some("--unknown"));
    assert_eq!(preference, None);
    assert_eq!(
        parse(&["--theme", "--help"], Config::default())
            .unwrap_err()
            .to_string(),
        "Invalid theme. Use 'light' or 'dark'."
    );
}

#[test]
fn empty_prompt_is_distinct_from_no_prompt() {
    let (empty, _) = run(&[""], Config::default());
    let (absent, _) = run(&[], Config::default());
    assert_eq!(empty.prompt.as_deref(), Some(""));
    assert_eq!(absent.prompt, None);
    assert_eq!(absent.model, DEFAULT_MODEL);
}
