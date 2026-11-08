use super::*;
use crate::theme::ThemeMode;

struct FakeStartup {
    config: Config,
    saved: Option<Config>,
    key: Option<String>,
    pipe: Option<String>,
    fail_save: bool,
    fail_execute: bool,
    events: Vec<&'static str>,
    action: Option<StartupAction>,
}

impl Default for FakeStartup {
    fn default() -> Self {
        Self {
            config: Config {
                model: Some("vendor/saved".to_string()),
                auto: true,
                ..Config::default()
            },
            saved: None,
            key: Some("test-key".to_string()),
            pipe: None,
            fail_save: false,
            fail_execute: false,
            events: Vec::new(),
            action: None,
        }
    }
}

impl StartupIo for FakeStartup {
    fn load_config(&mut self) -> Config {
        self.events.push("load");
        self.config.clone()
    }

    fn save_config(&mut self, config: &Config) -> StartupResult<()> {
        self.events.push("save");
        if self.fail_save {
            Err("save failed".into())
        } else {
            self.saved = Some(config.clone());
            Ok(())
        }
    }

    fn api_key(&mut self) -> StartupResult<String> {
        self.events.push("key");
        self.key.clone().ok_or_else(|| "key unavailable".into())
    }

    fn piped_input(&mut self) -> Option<String> {
        self.events.push("pipe");
        self.pipe.take()
    }

    fn execute(&mut self, action: StartupAction) -> StartupResult<()> {
        self.events.push("execute");
        self.action = Some(action);
        if self.fail_execute {
            Err("action failed".into())
        } else {
            Ok(())
        }
    }
}

fn start(args: &[&str], io: &mut FakeStartup) -> StartupResult<()> {
    run_startup_with(args.iter().map(|arg| arg.to_string()), io)
}

#[test]
fn local_dispatch_and_help_never_acquire_a_key_or_read_a_pipe() {
    for args in [
        vec!["--theme", "light", "--help"],
        vec!["AUTO", "ON"],
        vec!["model", "reset"],
        vec!["$example", "--help"],
        vec!["tool", "list"],
    ] {
        let mut io = FakeStartup {
            key: None,
            pipe: Some("unused".to_string()),
            ..FakeStartup::default()
        };
        start(&args, &mut io).unwrap();
        assert_eq!(io.events, ["load", "execute"], "{args:?}");
        match io.action.unwrap() {
            StartupAction::Help => assert_eq!(args.last(), Some(&"--help")),
            StartupAction::Local {
                command: LocalCommand::Auto(true),
                ..
            } => assert_eq!(args[0], "AUTO"),
            StartupAction::Local {
                command: LocalCommand::Model(ModelCommand::Reset),
                ..
            } => assert_eq!(args[0], "model"),
            StartupAction::Local {
                command: LocalCommand::SavedTool { name, args_tail },
                ..
            } => {
                assert_eq!(name, "example");
                assert_eq!(args_tail, "'--help'");
            }
            StartupAction::Local {
                command: LocalCommand::ManageTool(tools::ToolCommand::List),
                ..
            } => assert_eq!(args[0], "tool"),
            _ => panic!("unexpected dispatch for {args:?}"),
        }
        assert!(io.saved.is_none());
        assert_eq!(io.pipe.as_deref(), Some("unused"));
    }
}

#[test]
fn parse_and_key_failures_stop_before_dependent_work() {
    let mut io = FakeStartup::default();
    assert_eq!(
        start(&["--theme", "light", "--model"], &mut io)
            .unwrap_err()
            .to_string(),
        "--model requires a value"
    );
    assert_eq!(io.events, ["load"]);
    assert!(io.saved.is_none());
    let mut io = FakeStartup {
        key: None,
        ..FakeStartup::default()
    };
    assert_eq!(
        start(&["--theme", "light", "request"], &mut io)
            .unwrap_err()
            .to_string(),
        "key unavailable"
    );
    // A successfully parsed --theme is saved even when API startup fails.
    assert_eq!(io.events, ["load", "save", "key"]);
    assert_eq!(io.saved.unwrap().theme, ThemeMode::Light);
    assert!(io.action.is_none());
}

#[test]
fn theme_save_failure_keeps_the_active_override_and_dispatches_normally() {
    let mut io = FakeStartup {
        fail_save: true,
        ..FakeStartup::default()
    };
    start(
        &["--model", "vendor/session", "--theme", "light", "model"],
        &mut io,
    )
    .unwrap();
    assert_eq!(io.events, ["load", "save", "execute"]);
    match io.action.unwrap() {
        StartupAction::Local {
            args,
            command: LocalCommand::Model(ModelCommand::Show),
        } => {
            assert_eq!(args.model, "vendor/session");
            assert_eq!(args.theme, ThemeMode::Light);
            assert!(args.auto);
        }
        _ => panic!("expected model show"),
    }
    assert!(io.saved.is_none());
}

#[test]
fn ai_mode_selection_preserves_empty_prompts_pipe_summary_and_session_settings() {
    for (arguments, pipe, expected) in [
        (vec![], None, AiMode::Interactive),
        (
            vec![""],
            None,
            AiMode::Prompt {
                prompt: String::new(),
                piped_data: None,
            },
        ),
        (
            vec!["request"],
            None,
            AiMode::Prompt {
                prompt: "request".to_string(),
                piped_data: None,
            },
        ),
        (
            vec![],
            Some("data"),
            AiMode::Prompt {
                prompt: "Summarize and explain this data".to_string(),
                piped_data: Some("data".to_string()),
            },
        ),
        (
            vec!["request"],
            Some("data"),
            AiMode::Prompt {
                prompt: "request".to_string(),
                piped_data: Some("data".to_string()),
            },
        ),
    ] {
        let mut io = FakeStartup {
            pipe: pipe.map(str::to_string),
            ..FakeStartup::default()
        };
        start(&arguments, &mut io).unwrap();
        assert_eq!(io.events, ["load", "key", "pipe", "execute"]);
        match io.action.unwrap() {
            StartupAction::Ai {
                args,
                api_key,
                mode,
            } => {
                assert_eq!(mode, expected);
                assert_eq!(api_key, "test-key");
                assert_eq!(args.model, "vendor/saved");
                assert!(args.auto);
            }
            _ => panic!("expected an AI mode"),
        }
    }
}

#[test]
fn dispatch_failures_return_without_falling_through_to_another_mode() {
    for args in [vec!["tool", "list"], vec!["request"]] {
        let mut io = FakeStartup {
            fail_execute: true,
            ..FakeStartup::default()
        };
        assert_eq!(
            start(&args, &mut io).unwrap_err().to_string(),
            "action failed"
        );
        assert_eq!(
            io.events
                .iter()
                .filter(|event| **event == "execute")
                .count(),
            1
        );
        if args[0] == "tool" {
            assert_eq!(io.events, ["load", "execute"]);
        } else {
            assert_eq!(io.events, ["load", "key", "pipe", "execute"]);
        }
    }
}

struct PartialRead(bool);

impl Read for PartialRead {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0 {
            Err(io::Error::other("pipe failed"))
        } else {
            self.0 = true;
            buffer[..3].copy_from_slice(b"ok\xff");
            Ok(3)
        }
    }
}

#[test]
fn piped_input_preserves_the_byte_bound_lossy_decoding_and_partial_reads() {
    assert_eq!(read_piped_input(true, PartialRead(false)), None);
    assert_eq!(read_piped_input(false, &b""[..]), None);
    assert_eq!(read_piped_input(false, PartialRead(true)), None);
    assert_eq!(
        read_piped_input(false, PartialRead(false)).as_deref(),
        Some("ok\u{fffd}")
    );
    let data = vec![b'x'; MAX_PIPE_BYTES + 10];
    assert_eq!(
        read_piped_input(false, data.as_slice()).unwrap().len(),
        MAX_PIPE_BYTES + 1
    );
}
