//! The interactive session owns input dispatch and settings. Conversation
//! updates occur only after an action returns its explicit session effect.

use crate::config::preferences::{
    ModelCommand, apply_model_command, auto_mode_description, parse_auto_toggle,
    parse_model_command, set_auto_mode,
};
use crate::conversation::{ConversationContext, ConversationHistory, HistoryNotice, NoticePolicy};
use crate::execution::{CommandOutcome, ExecutionReport};
use crate::prompt::process_prompt_with_context;
use crate::subprocess::{run_command_with_output, run_utility};
use crate::{
    Theme, interpreter_for_script, is_safe_direct_command, is_script_execution, tools,
    working_directory,
};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::env;

type SessionResult<T> = Result<T, Box<dyn std::error::Error>>;

trait SessionInput {
    fn readline(&mut self, prompt: &str) -> Result<String, ReadlineError>;
    fn remember(&mut self, input: &str);
}

impl SessionInput for DefaultEditor {
    fn readline(&mut self, prompt: &str) -> Result<String, ReadlineError> {
        DefaultEditor::readline(self, prompt)
    }

    fn remember(&mut self, input: &str) {
        let _ = self.add_history_entry(input);
    }
}

struct SessionSettings {
    model: String,
    auto: bool,
}

struct Session {
    settings: SessionSettings,
    history: ConversationHistory,
}

#[derive(Debug)]
enum SessionAction {
    Quit,
    CurrentDirectory,
    ParentDirectory,
    Clear,
    ToggleAuto(bool),
    ShowAuto,
    Model(ModelCommand),
    Finder,
    SavedTool {
        input: String,
        name: String,
        args_tail: String,
    },
    ManageTool(tools::ToolCommand),
    Direct {
        input: String,
        command: String,
    },
    Prompt(String),
}

#[derive(Clone, Copy)]
enum NextPrompt {
    Immediate,
    BlankLine,
}

enum SessionEffect {
    Exit,
    Continue(NextPrompt),
    Clear,
    Record {
        context: ConversationContext,
        notice: NoticePolicy,
        next: NextPrompt,
    },
}

impl SessionEffect {
    fn record_command(prompt: &str, outcome: CommandOutcome, notice: NoticePolicy) -> Self {
        Self::Record {
            context: ConversationContext {
                prompt: prompt.to_string(),
                outcomes: vec![outcome],
            },
            notice,
            next: NextPrompt::Immediate,
        }
    }
}

enum SessionState<O, I> {
    Starting(O),
    OpeningEditor(O),
    Reading(I),
    Dispatching {
        editor: I,
        line: String,
    },
    Handling {
        editor: I,
        action: SessionAction,
    },
    Recording {
        editor: I,
        context: ConversationContext,
        notice: NoticePolicy,
        next: NextPrompt,
    },
    Notifying {
        editor: I,
        notice: HistoryNotice,
        next: NextPrompt,
    },
    Clearing(I),
    Separating(I),
    Interrupted(I),
    Closing(I),
    Complete,
    Failed(Box<dyn std::error::Error>),
}

pub(crate) fn run_interactive_mode(
    initial_model: &str,
    api_key: &str,
    theme: &Theme,
    initial_auto: bool,
) -> SessionResult<()> {
    run_session_with(
        initial_model,
        initial_auto,
        theme,
        || DefaultEditor::new().map_err(Into::into),
        |action, settings, history| {
            handle_action(
                action,
                settings,
                history,
                theme,
                |prompt, model, auto, history| {
                    process_prompt_with_context(prompt, model, api_key, theme, history, None, auto)
                },
            )
        },
    )
    .map(|_| ())
}

fn run_session_with<O, I, H>(
    initial_model: &str,
    initial_auto: bool,
    theme: &Theme,
    open_editor: O,
    mut handle: H,
) -> SessionResult<Session>
where
    O: FnOnce() -> SessionResult<I>,
    I: SessionInput,
    H: FnMut(
        SessionAction,
        &mut SessionSettings,
        &ConversationHistory,
    ) -> SessionResult<SessionEffect>,
{
    let mut session = Session {
        settings: SessionSettings {
            model: initial_model.to_string(),
            auto: initial_auto,
        },
        history: ConversationHistory::default(),
    };
    let mut state = SessionState::Starting(open_editor);
    loop {
        state = match state {
            SessionState::Starting(open) => {
                print_banner(theme, session.settings.auto);
                SessionState::OpeningEditor(open)
            }
            SessionState::OpeningEditor(open) => match open() {
                Ok(editor) => SessionState::Reading(editor),
                Err(error) => SessionState::Failed(error),
            },
            SessionState::Reading(mut editor) => match editor.readline(&session_prompt(theme)) {
                Ok(line) => SessionState::Dispatching { editor, line },
                Err(ReadlineError::Interrupted) => SessionState::Interrupted(editor),
                Err(ReadlineError::Eof) => SessionState::Closing(editor),
                Err(error) => SessionState::Failed(error.into()),
            },
            SessionState::Dispatching { mut editor, line } => {
                let input = line.trim();
                if input.is_empty() {
                    SessionState::Reading(editor)
                } else {
                    // Readline history records every nonempty input before routing,
                    // independently of whether the action belongs in AI context.
                    editor.remember(input);
                    SessionState::Handling {
                        editor,
                        action: route_input(input.to_string()),
                    }
                }
            }
            SessionState::Handling { editor, action } => {
                match handle(action, &mut session.settings, &session.history) {
                    Ok(SessionEffect::Exit) => SessionState::Closing(editor),
                    Ok(SessionEffect::Continue(NextPrompt::Immediate)) => {
                        SessionState::Reading(editor)
                    }
                    Ok(SessionEffect::Continue(NextPrompt::BlankLine)) => {
                        SessionState::Separating(editor)
                    }
                    Ok(SessionEffect::Clear) => SessionState::Clearing(editor),
                    Ok(SessionEffect::Record {
                        context,
                        notice,
                        next,
                    }) => SessionState::Recording {
                        editor,
                        context,
                        notice,
                        next,
                    },
                    Err(error) => SessionState::Failed(error),
                }
            }
            SessionState::Recording {
                editor,
                context,
                notice,
                next,
            } => {
                let notice = session.history.record(context, notice);
                SessionState::Notifying {
                    editor,
                    notice,
                    next,
                }
            }
            SessionState::Notifying {
                editor,
                notice,
                next,
            } => {
                if notice == HistoryNotice::ContextLimitExceeded {
                    println!("{}", theme.helper_text(
                        "Note: Context is being automatically compacted to fit within token limits."
                    ));
                }
                match next {
                    NextPrompt::Immediate => SessionState::Reading(editor),
                    NextPrompt::BlankLine => SessionState::Separating(editor),
                }
            }
            SessionState::Clearing(editor) => {
                session.history.clear();
                print_banner(theme, session.settings.auto);
                SessionState::Reading(editor)
            }
            SessionState::Separating(editor) => {
                println!();
                SessionState::Reading(editor)
            }
            SessionState::Interrupted(editor) => {
                println!("^C");
                SessionState::Reading(editor)
            }
            SessionState::Closing(editor) => {
                println!("Goodbye!");
                drop(editor);
                SessionState::Complete
            }
            SessionState::Complete => return Ok(session),
            SessionState::Failed(error) => return Err(error),
        };
    }
}

fn route_input(input: String) -> SessionAction {
    match input.as_str() {
        "q" | "exit" | "quit" => return SessionAction::Quit,
        "." => return SessionAction::CurrentDirectory,
        ".." => return SessionAction::ParentDirectory,
        "clear" => return SessionAction::Clear,
        "auto" => return SessionAction::ShowAuto,
        _ => {}
    }
    if let Some(enabled) = parse_auto_toggle(&input) {
        return SessionAction::ToggleAuto(enabled);
    }
    if let Some(command) = parse_model_command(&input) {
        return SessionAction::Model(command);
    }
    if input == "finder" {
        return SessionAction::Finder;
    }
    if let Some((name, args_tail)) = tools::parse_tool_invocation(&input) {
        return SessionAction::SavedTool {
            input,
            name,
            args_tail,
        };
    }
    if let Some(command) = tools::parse_tool_command(&input) {
        return SessionAction::ManageTool(command);
    }
    if is_safe_direct_command(&input) {
        let command = if input == "ls" {
            "ls -l".to_string()
        } else if is_script_execution(&input) && !input.contains(' ') {
            match interpreter_for_script(&input) {
                Some(interpreter) => format!("{interpreter} {input}"),
                None => input.clone(),
            }
        } else {
            input.clone()
        };
        return SessionAction::Direct { input, command };
    }
    SessionAction::Prompt(input)
}

fn handle_action<P>(
    action: SessionAction,
    settings: &mut SessionSettings,
    history: &ConversationHistory,
    theme: &Theme,
    process_prompt: P,
) -> SessionResult<SessionEffect>
where
    P: FnOnce(&str, &str, bool, &[ConversationContext]) -> SessionResult<ExecutionReport>,
{
    Ok(match action {
        SessionAction::Quit => SessionEffect::Exit,
        SessionAction::CurrentDirectory => {
            let cwd = env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            println!(
                "{} {}",
                theme.prompt_text("run>"),
                theme.command_text("pwd")
            );
            println!("{cwd}");
            SessionEffect::record_command(
                "pwd",
                CommandOutcome::succeeded("pwd", cwd),
                NoticePolicy::Silent,
            )
        }
        SessionAction::ParentDirectory => {
            println!(
                "{} {}",
                theme.prompt_text("run>"),
                theme.command_text("cd ..")
            );
            match working_directory::change_to_parent() {
                Ok(()) => {
                    let cwd = env::current_dir()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| "unknown".to_string());
                    println!(
                        "{}",
                        theme.helper_text(&format!("Changed directory to: {cwd}"))
                    );
                    SessionEffect::record_command(
                        "cd ..",
                        CommandOutcome::succeeded("cd ..", format!("Changed to: {cwd}")),
                        NoticePolicy::Silent,
                    )
                }
                Err(error) => {
                    eprintln!("Failed to change directory: {error}");
                    SessionEffect::Continue(NextPrompt::Immediate)
                }
            }
        }
        SessionAction::Clear => {
            // Only a spawn/wait error prevents reset; a nonzero status still resets.
            run_utility("clear", &[])?;
            SessionEffect::Clear
        }
        SessionAction::ToggleAuto(enabled) => {
            match set_auto_mode(enabled, theme) {
                Ok(()) => settings.auto = enabled,
                Err(error) => eprintln!("Error: {error}"),
            }
            SessionEffect::Continue(NextPrompt::Immediate)
        }
        SessionAction::ShowAuto => {
            println!(
                "{}",
                theme.helper_text(auto_mode_description(settings.auto))
            );
            SessionEffect::Continue(NextPrompt::Immediate)
        }
        SessionAction::Model(command) => {
            match apply_model_command(command, theme, &settings.model) {
                Ok(model) => settings.model = model,
                Err(error) => eprintln!("Error: {error}"),
            }
            SessionEffect::Continue(NextPrompt::Immediate)
        }
        SessionAction::Finder => {
            match run_utility("open", &["."]) {
                Ok(_) => println!(
                    "{}",
                    theme.helper_text("Opened Finder at current directory")
                ),
                Err(error) => eprintln!("Failed to open Finder: {error}"),
            }
            SessionEffect::Continue(NextPrompt::Immediate)
        }
        SessionAction::SavedTool {
            input,
            name,
            args_tail,
        } => {
            let resolved = tools::tools_root()
                .ok_or_else(|| "HOME is not set; cannot locate ~/.ask/tools.".to_string())
                .and_then(|root| tools::resolve_approved_tool(&root, &name, &args_tail));
            match resolved {
                Ok(command) => record_direct_command(&input, &command, theme),
                Err(error) => {
                    eprintln!("{error}");
                    SessionEffect::Continue(NextPrompt::Immediate)
                }
            }
        }
        SessionAction::ManageTool(command) => {
            if let Err(error) = tools::handle_tool_command(command, &settings.model, theme) {
                eprintln!("Error: {error}");
            }
            SessionEffect::Continue(NextPrompt::Immediate)
        }
        SessionAction::Direct { input, command } => record_direct_command(&input, &command, theme),
        SessionAction::Prompt(prompt) => {
            match process_prompt(&prompt, &settings.model, settings.auto, history.as_slice()) {
                Ok(report) => SessionEffect::Record {
                    context: ConversationContext {
                        prompt,
                        outcomes: report.outcomes,
                    },
                    notice: NoticePolicy::CheckBudget,
                    next: NextPrompt::BlankLine,
                },
                Err(error) => {
                    eprintln!("Error: {error}");
                    SessionEffect::Continue(NextPrompt::BlankLine)
                }
            }
        }
    })
}

fn record_direct_command(input: &str, command: &str, theme: &Theme) -> SessionEffect {
    println!(
        "{} {}",
        theme.prompt_text("run>"),
        theme.command_text(command)
    );
    let outcome = CommandOutcome::from_result(command, run_command_with_output(command));
    outcome.print_failure();
    SessionEffect::record_command(input, outcome, NoticePolicy::CheckBudget)
}

fn print_banner(theme: &Theme, auto: bool) {
    println!(
        "{}",
        theme.prompt_text(
            "Interactive mode. Commands: 'exit', 'clear', 'finder', 'auto on|off', 'tool', '$name'"
        )
    );
    println!(
        "{}",
        theme.helper_text("Common commands and scripts execute directly without confirmation")
    );
    println!(
        "{}",
        theme.helper_text("Shortcuts: q=quit, .=pwd, ..=cd ..")
    );
    if auto {
        println!("{}", theme.helper_text(auto_mode_description(true)));
    }
    if let Ok(cwd) = env::current_dir() {
        println!("{}", theme.helper_text(&format!("📁 {}", cwd.display())));
    }
    println!();
}

fn session_prompt(theme: &Theme) -> String {
    let cwd_display = if let Ok(cwd) = env::current_dir() {
        if let Ok(home) = env::var("HOME") {
            if cwd.to_string_lossy() == home {
                "~".to_string()
            } else if let Some(relative) = cwd.to_string_lossy().strip_prefix(&format!("{home}/")) {
                format!("~/{}", relative.rsplit('/').next().unwrap_or(relative))
            } else if let Some(name) = cwd.file_name() {
                name.to_string_lossy().to_string()
            } else {
                "/".to_string()
            }
        } else {
            cwd.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string())
        }
    } else {
        "?".to_string()
    };
    format!("{} ", theme.prompt_text(&format!("ask [{cwd_display}]>")))
}

#[cfg(test)]
mod tests;
