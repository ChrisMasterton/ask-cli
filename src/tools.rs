//! The saved tool library: small reusable bash/python scripts the LLM writes
//! on request, stored under `~/.ask/tools/<name>/` and re-run later with
//! `$name args`. Execution is checksum-gated — the user reviews and approves
//! a script's source, and any change to it forces a re-review.

use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Theme;
mod decoding;
mod integrity;
pub(crate) mod invocation;
mod workflow;

use integrity::{ToolIntegrity, inspect_tool};
use workflow::{ToolWorkflowRequest, run_tool_workflow};

const MANIFEST_FILE: &str = "manifest";
const MAX_CATALOG_TOOLS: usize = 20;
const MAX_CATALOG_DESCRIPTION_BYTES: usize = 100;
const MAX_TOOL_NAME_LEN: usize = 32;

pub(crate) const TOOL_NEW_TEMPLATE: &str = r#"
You are writing a small reusable command-line tool for a user's personal tool library.

**Requirements:**
- The FIRST line of your response must be exactly `LANG: bash` or `LANG: python` — pick the better fit for the job.
- Everything after that first line is the complete script body and nothing else.
- Bash: plain portable bash using only the standard macOS userland.
- Python: Python 3 standard library only — no pip, no third-party imports.
- Arguments arrive as ordinary argv ("$@" / sys.argv). Validate them; on bad usage print a usage line to stderr and exit 1.
- Print results to stdout. Keep the script focused, small, and readable.
- No code fences, no markdown, no commentary before or after the script.

**Tool name:** {name}
**What it must do:** {description}
"#;

pub(crate) const TOOL_IMPROVE_TEMPLATE: &str = r#"
You are improving an existing tool script from a user's personal tool library.

**Requirements:**
- Return the COMPLETE updated script body and nothing else — no code fences, no commentary, no LANG line.
- The language stays {lang}. Standard library / plain shell only — no package management.
- Preserve current behavior except where the improvement instructions say otherwise.

**Tool name:** {name}
**Current script:**
{current_script}

**Improvement instructions:** {instructions}
"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ToolLang {
    Bash,
    Python,
}

impl ToolLang {
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "bash" => Some(Self::Bash),
            "python" => Some(Self::Python),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Python => "python",
        }
    }

    pub(crate) fn script_file(&self) -> &'static str {
        match self {
            Self::Bash => "main.sh",
            Self::Python => "main.py",
        }
    }

    pub(crate) fn interpreter(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Python => "python3",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolManifest {
    pub(crate) description: String,
    pub(crate) lang: ToolLang,
    pub(crate) approved_sha256: Option<String>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum ToolCommand {
    List,
    Show(String),
    Rm(String),
    Approve(String),
    New { name: String, description: String },
    Improve { name: String, instructions: String },
}

pub(crate) fn tools_root() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".ask").join("tools"))
}

// Tool names double as directory names, so validity is also the traversal
// guard: no '/', '.', '~', uppercase, or whitespace can ever appear.
pub(crate) fn is_valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_TOOL_NAME_LEN
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub(crate) fn tool_dir(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

fn script_path(root: &Path, name: &str, lang: ToolLang) -> PathBuf {
    tool_dir(root, name).join(lang.script_file())
}

pub(crate) fn load_manifest(root: &Path, name: &str) -> Result<ToolManifest, String> {
    let dir = tool_dir(root, name);
    if !dir.is_dir() {
        return Err(format!("No tool named '{name}'. See `tool list`."));
    }
    let path = dir.join(MANIFEST_FILE);
    let contents = fs::read_to_string(&path).map_err(|err| {
        format!(
            "Cannot read manifest for '{name}' ({}): {err}",
            path.display()
        )
    })?;

    let mut description = String::new();
    let mut lang = None;
    let mut approved_sha256 = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("description=") {
            description = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("lang=") {
            lang = ToolLang::from_str(value.trim());
        } else if let Some(value) = line.strip_prefix("approved_sha256=") {
            let value = value.trim();
            if !value.is_empty() {
                approved_sha256 = Some(value.to_lowercase());
            }
        }
    }

    let lang = lang.ok_or_else(|| {
        format!("Manifest for '{name}' is missing a valid lang= line (bash or python).")
    })?;
    Ok(ToolManifest {
        description,
        lang,
        approved_sha256,
    })
}

pub(crate) fn save_manifest(root: &Path, name: &str, manifest: &ToolManifest) -> io::Result<()> {
    let dir = tool_dir(root, name);
    fs::create_dir_all(&dir)?;
    let mut contents = format!(
        "description={}\nlang={}\n",
        manifest.description,
        manifest.lang.as_str()
    );
    if let Some(sum) = &manifest.approved_sha256 {
        contents.push_str(&format!("approved_sha256={sum}\n"));
    }
    fs::write(dir.join(MANIFEST_FILE), contents)
}

// The hash covers the interpreter too: swapping `lang=` in a hand-edited
// manifest must invalidate approval just like editing the script would.
pub(crate) fn tool_checksum(lang: ToolLang, script: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lang.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(script);
    format!("{:x}", hasher.finalize())
}

fn tool_names(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_valid_tool_name(name))
        .collect();
    names.sort();
    names
}

/// Parses the `tool …` management command family. Only inputs whose second
/// token is a known subcommand are claimed; everything else returns None so
/// prompts like "tool to find big files" still reach the LLM. Subcommands
/// that name a tool take exactly one argument — more than one also falls
/// through to the LLM, mirroring `parse_model_command`.
pub(crate) fn parse_tool_command(input: &str) -> Option<ToolCommand> {
    let mut tokens = input.split_whitespace();
    if !tokens.next()?.eq_ignore_ascii_case("tool") {
        return None;
    }
    let Some(sub) = tokens.next() else {
        return Some(ToolCommand::List);
    };
    match sub.to_lowercase().as_str() {
        "list" | "ls" => tokens.next().is_none().then_some(ToolCommand::List),
        "show" => single_arg(tokens).map(ToolCommand::Show),
        "rm" | "remove" => single_arg(tokens).map(ToolCommand::Rm),
        "approve" => single_arg(tokens).map(ToolCommand::Approve),
        "new" => {
            let name = tokens.next().unwrap_or_default().to_string();
            let description = tokens.collect::<Vec<_>>().join(" ");
            Some(ToolCommand::New { name, description })
        }
        "improve" => {
            let name = tokens.next().unwrap_or_default().to_string();
            let instructions = tokens.collect::<Vec<_>>().join(" ");
            Some(ToolCommand::Improve { name, instructions })
        }
        _ => None,
    }
}

// Zero args yields an empty name (the handler prints usage); two or more
// args means the input is plausibly prose, so it falls through to the LLM.
fn single_arg(mut tokens: std::str::SplitWhitespace) -> Option<String> {
    let first = tokens.next().unwrap_or_default().to_string();
    tokens.next().is_none().then_some(first)
}

/// Parses a `$name args` tool invocation. The name must be a full token: the
/// character run after `$` has to end at whitespace or end-of-input, so shell
/// text like `$HOME/x`, `$(pwd)`, or `$hello;rm x` never parses as one.
pub(crate) fn parse_tool_invocation(input: &str) -> Option<(String, String)> {
    let rest = input.trim().strip_prefix('$')?;
    let name_len = rest
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-')
        .count();
    if name_len == 0 {
        return None;
    }
    let (name, tail) = rest.split_at(name_len);
    if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
        return None;
    }
    if !is_valid_tool_name(name) {
        return None;
    }
    Some((name.to_string(), tail.trim().to_string()))
}

/// The tool catalog injected into the prompt so the model can suggest
/// `$name args` lines. Only approved tools are listed — the model must never
/// be encouraged to propose a script the user hasn't reviewed.
pub(crate) fn catalog_block(root: &Path) -> String {
    let mut entries = Vec::new();
    for name in tool_names(root) {
        if entries.len() >= MAX_CATALOG_TOOLS {
            break;
        }
        let Ok(inspection) = inspect_tool(root, &name) else {
            continue;
        };
        if inspection.integrity != ToolIntegrity::Approved {
            continue;
        }
        let manifest = inspection.manifest;
        let (description, truncated) =
            crate::text::truncate_utf8_bytes(&manifest.description, MAX_CATALOG_DESCRIPTION_BYTES);
        let suffix = if truncated { "…" } else { "" };
        entries.push(format!("- ${name} — {description}{suffix}"));
    }
    if entries.is_empty() {
        return String::new();
    }
    format!(
        "**User tool library** — reusable scripts this user has saved:\n{}\nTo run one, output a line of exactly the form `$name args` (the `$` sigil plus the tool name — no interpreter, no path). Only reference tools from this list, and treat `$name` lines as executable commands, not commentary.\n",
        entries.join("\n")
    )
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn require_root() -> Result<PathBuf, String> {
    tools_root().ok_or_else(|| "HOME is not set; cannot locate ~/.ask/tools.".to_string())
}

fn require_name<'a>(name: &'a str, usage: &str) -> Result<&'a str, String> {
    if is_valid_tool_name(name) {
        Ok(name)
    } else if name.is_empty() {
        Err(format!("Usage: {usage}"))
    } else {
        Err(format!(
            "Invalid tool name '{name}' — use lowercase letters, digits, '-' and '_' (max {MAX_TOOL_NAME_LEN} chars)."
        ))
    }
}

fn require_api_key() -> Result<String, String> {
    env::var("OPENROUTER_ASK_API_KEY")
        .map_err(|_| "Please set the OPENROUTER_ASK_API_KEY environment variable.".to_string())
}

fn print_source(theme: &Theme, name: &str, lang: ToolLang, source: &str) {
    println!(
        "{}",
        theme.helper_text(&format!("--- {name} ({}) ---", lang.as_str()))
    );
    print!("{source}");
    if !source.ends_with('\n') {
        println!();
    }
    println!("{}", theme.helper_text("---"));
}

fn call_llm_content(
    prompt: String,
    model: &str,
    api_key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = crate::api::call_llm(
        vec![json!({"role": "user", "content": prompt})],
        model,
        api_key,
    )?;
    crate::api::response::tool_content_from_api_response(response)
}

pub(crate) fn handle_tool_command(
    command: ToolCommand,
    model: &str,
    theme: &Theme,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ToolCommand::List => {
            let root = require_root()?;
            let tools = tool_names(&root);
            if tools.is_empty() {
                println!(
                    "{}",
                    theme.helper_text(
                        "No saved tools yet. Create one with: tool new <name> <what it should do>"
                    )
                );
                return Ok(());
            }
            for name in tools {
                match inspect_tool(&root, &name) {
                    Ok(inspection) => {
                        let flag = match &inspection.integrity {
                            ToolIntegrity::Approved => String::new(),
                            other => format!("  [{}]", other.description()),
                        };
                        let manifest = inspection.manifest;
                        println!(
                            "  {} ({}) — {}{}",
                            theme.command_text(&format!("${name}")),
                            manifest.lang.as_str(),
                            manifest.description,
                            theme.helper_text(&flag)
                        );
                    }
                    Err(err) => {
                        println!(
                            "  {} — {}",
                            theme.command_text(&format!("${name}")),
                            theme.helper_text(&format!("[broken: {err}]"))
                        );
                    }
                }
            }
        }
        ToolCommand::Show(name) => {
            let name = require_name(&name, "tool show <name>")?;
            let root = require_root()?;
            let inspection = inspect_tool(&root, name)?;
            let manifest = &inspection.manifest;
            let script = script_path(&root, name, manifest.lang);
            let source = fs::read_to_string(&script)
                .map_err(|err| format!("Cannot read script for '{name}': {err}"))?;
            println!(
                "{}",
                theme.helper_text(&format!(
                    "Tool: {name} ({}) — {}",
                    manifest.lang.as_str(),
                    manifest.description
                ))
            );
            println!(
                "{}",
                theme.helper_text(&format!(
                    "Script: {} ({})",
                    script.display(),
                    inspection.integrity.description()
                ))
            );
            println!();
            print_source(theme, name, manifest.lang, &source);
        }
        ToolCommand::Rm(name) => {
            let name = require_name(&name, "tool rm <name>")?;
            let root = require_root()?;
            let dir = tool_dir(&root, name);
            if !dir.is_dir() {
                return Err(format!("No tool named '{name}'. See `tool list`.").into());
            }
            fs::remove_dir_all(&dir)?;
            println!(
                "{}",
                theme.helper_text(&format!("Removed tool '{name}' ({}).", dir.display()))
            );
        }
        ToolCommand::Approve(name) => {
            run_tool_workflow(ToolWorkflowRequest::Approve(name), model, theme)?;
        }
        ToolCommand::New { name, description } => {
            run_tool_workflow(ToolWorkflowRequest::New { name, description }, model, theme)?;
        }
        ToolCommand::Improve { name, instructions } => {
            run_tool_workflow(
                ToolWorkflowRequest::Improve { name, instructions },
                model,
                theme,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(super) struct TempDirGuard(PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(super) fn unique_tools_root(name: &str) -> (TempDirGuard, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!("ask-tools-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp tools root");
        (TempDirGuard(path.clone()), path)
    }

    pub(super) fn seed_tool(root: &Path, name: &str, lang: ToolLang, script: &str, approved: bool) {
        let manifest = ToolManifest {
            description: format!("test tool {name}"),
            lang,
            approved_sha256: approved.then(|| tool_checksum(lang, script.as_bytes())),
        };
        save_manifest(root, name, &manifest).expect("save manifest");
        fs::write(tool_dir(root, name).join(lang.script_file()), script).expect("write script");
    }

    pub(super) fn prepared_command(
        root: &Path,
        name: &str,
        args_tail: &str,
    ) -> Result<String, String> {
        invocation::invoke_with(
            name,
            args_tail,
            invocation::InvocationOrigin::User,
            || Ok(root.to_path_buf()),
            |_| {},
            |_| Ok(String::new()),
        )
        .map(|execution| execution.command)
    }

    #[test]
    fn tool_names_reject_traversal_and_odd_characters() {
        for good in ["backup", "du2", "my-tool_2", "2nd", "a"] {
            assert!(is_valid_tool_name(good), "{good} should be valid");
        }
        let too_long = "a".repeat(33);
        for bad in [
            "",
            "../x",
            "a/b",
            ".hidden",
            "Backup",
            "a b",
            "-lead",
            "_lead",
            "~x",
            too_long.as_str(),
        ] {
            assert!(!is_valid_tool_name(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn tool_command_parser_claims_only_real_subcommands() {
        assert_eq!(parse_tool_command("tool"), Some(ToolCommand::List));
        assert_eq!(parse_tool_command("tool list"), Some(ToolCommand::List));
        assert_eq!(parse_tool_command("Tool ls"), Some(ToolCommand::List));
        assert_eq!(
            parse_tool_command("tool show backup"),
            Some(ToolCommand::Show("backup".to_string()))
        );
        assert_eq!(
            parse_tool_command("tool show"),
            Some(ToolCommand::Show(String::new()))
        );
        assert_eq!(
            parse_tool_command("tool rm backup"),
            Some(ToolCommand::Rm("backup".to_string()))
        );
        assert_eq!(
            parse_tool_command("tool new backup back up my documents"),
            Some(ToolCommand::New {
                name: "backup".to_string(),
                description: "back up my documents".to_string(),
            })
        );
        assert_eq!(
            parse_tool_command("tool improve backup add a dry-run flag"),
            Some(ToolCommand::Improve {
                name: "backup".to_string(),
                instructions: "add a dry-run flag".to_string(),
            })
        );
        // Prose that merely starts with "tool" must reach the LLM.
        assert_eq!(parse_tool_command("tool to find big files"), None);
        assert_eq!(parse_tool_command("tool listing please"), None);
        assert_eq!(parse_tool_command("tool show me the weather"), None);
        assert_eq!(parse_tool_command("tools list"), None);
    }

    #[test]
    fn tool_invocation_parser_requires_a_clean_name_token() {
        assert_eq!(
            parse_tool_invocation("$backup"),
            Some(("backup".to_string(), String::new()))
        );
        assert_eq!(
            parse_tool_invocation("$backup ~/docs \"a b\""),
            Some(("backup".to_string(), "~/docs \"a b\"".to_string()))
        );
        for not_an_invocation in [
            "$",
            "$ x",
            "$(pwd)",
            "$HOME/x",
            "$Bad",
            "$hello;rm x",
            "echo $backup",
        ] {
            assert_eq!(
                parse_tool_invocation(not_an_invocation),
                None,
                "{not_an_invocation:?} must not parse as a tool invocation"
            );
        }
    }

    #[test]
    fn manifest_round_trips_and_ignores_unknown_keys() {
        let (_guard, root) = unique_tools_root("manifest");
        let manifest = ToolManifest {
            description: "does = things".to_string(),
            lang: ToolLang::Python,
            approved_sha256: Some("ab".repeat(32)),
        };
        save_manifest(&root, "demo", &manifest).expect("save");

        let path = tool_dir(&root, "demo").join(MANIFEST_FILE);
        let mut contents = fs::read_to_string(&path).expect("read manifest");
        contents.push_str("future_key=whatever\n");
        fs::write(&path, contents).expect("append unknown key");

        let loaded = load_manifest(&root, "demo").expect("load");
        assert_eq!(loaded.description, "does = things");
        assert_eq!(loaded.lang, ToolLang::Python);
        assert_eq!(loaded.approved_sha256, manifest.approved_sha256);

        fs::write(&path, "description=x\n").expect("drop lang line");
        assert!(
            load_manifest(&root, "demo").is_err(),
            "missing lang must fail"
        );
    }

    #[test]
    fn integrity_inspection_rechecks_edits_and_language_changes() {
        let (_guard, root) = unique_tools_root("checksum");
        seed_tool(&root, "ok", ToolLang::Bash, "echo hi\n", true);
        assert_eq!(
            inspect_tool(&root, "ok").unwrap().integrity,
            ToolIntegrity::Approved
        );

        seed_tool(&root, "pending", ToolLang::Bash, "echo hi\n", false);
        assert_eq!(
            inspect_tool(&root, "pending").unwrap().integrity,
            ToolIntegrity::Unapproved
        );

        fs::write(tool_dir(&root, "ok").join("main.sh"), "echo tampered\n").expect("tamper");
        assert_eq!(
            inspect_tool(&root, "ok").unwrap().integrity,
            ToolIntegrity::Mismatch
        );

        // Swapping lang= in the manifest must not survive either: the hash
        // covers the interpreter, and the script file for the new lang is
        // missing anyway.
        seed_tool(&root, "swapped", ToolLang::Bash, "echo hi\n", true);
        let manifest_path = tool_dir(&root, "swapped").join(MANIFEST_FILE);
        let contents = fs::read_to_string(&manifest_path)
            .expect("read manifest")
            .replace("lang=bash", "lang=python");
        fs::write(&manifest_path, contents).expect("swap lang");
        fs::write(tool_dir(&root, "swapped").join("main.py"), "echo hi\n").expect("plant script");
        assert_eq!(
            inspect_tool(&root, "swapped").unwrap().integrity,
            ToolIntegrity::Mismatch
        );

        assert!(inspect_tool(&root, "missing").is_err());
        fs::remove_file(tool_dir(&root, "pending").join("main.sh")).unwrap();
        let broken = inspect_tool(&root, "pending").unwrap();
        assert_eq!(broken.manifest.lang, ToolLang::Bash);
        assert!(matches!(broken.integrity, ToolIntegrity::Broken(_)));
    }

    #[test]
    fn invocation_refuses_everything_but_approved_tools() {
        let (_guard, root) = unique_tools_root("resolve");
        seed_tool(&root, "greet", ToolLang::Python, "print('hi')\n", true);

        let resolved = prepared_command(&root, "greet", "  a b  ").expect("resolve");
        let script = tool_dir(&root, "greet").join("main.py");
        assert_eq!(
            resolved,
            format!("python3 '{}' a b", script.display()),
            "interpreter + quoted path + verbatim tail"
        );
        let no_args = prepared_command(&root, "greet", "").expect("resolve without args");
        assert!(!no_args.ends_with(' '));

        seed_tool(&root, "pending", ToolLang::Bash, "echo hi\n", false);
        let err = prepared_command(&root, "pending", "").unwrap_err();
        assert!(
            err.contains("tool approve"),
            "unapproved error should guide: {err}"
        );

        fs::write(tool_dir(&root, "greet").join("main.py"), "print('evil')\n").expect("tamper");
        let err = prepared_command(&root, "greet", "").unwrap_err();
        assert!(
            err.contains("changed since"),
            "mismatch error should explain: {err}"
        );

        assert!(prepared_command(&root, "nope", "").is_err());
        assert!(prepared_command(&root, "../etc", "").is_err());
    }

    #[test]
    fn invocation_quotes_script_paths_containing_spaces() {
        let (_guard, base) = unique_tools_root("space");
        let root = base.join("with space");
        fs::create_dir_all(&root).expect("create spaced root");
        seed_tool(&root, "hi", ToolLang::Bash, "echo hi\n", true);
        let resolved = prepared_command(&root, "hi", "").expect("resolve");
        let script = tool_dir(&root, "hi").join("main.sh");
        assert_eq!(resolved, format!("bash '{}'", script.display()));
    }

    #[test]
    fn catalog_lists_only_approved_tools_and_truncates_descriptions() {
        let (_guard, root) = unique_tools_root("catalog");
        assert_eq!(catalog_block(&root), "", "empty library yields no catalog");

        seed_tool(&root, "ready", ToolLang::Bash, "echo hi\n", true);
        seed_tool(&root, "pending", ToolLang::Bash, "echo hi\n", false);
        let long_description = "x".repeat(200);
        let manifest = ToolManifest {
            description: long_description,
            lang: ToolLang::Bash,
            approved_sha256: Some(tool_checksum(ToolLang::Bash, b"echo hi\n")),
        };
        save_manifest(&root, "wordy", &manifest).expect("save");
        fs::write(tool_dir(&root, "wordy").join("main.sh"), "echo hi\n").expect("write");

        let catalog = catalog_block(&root);
        assert!(catalog.contains("$ready"));
        assert!(catalog.contains("$wordy"));
        assert!(!catalog.contains("$pending"), "unapproved tools stay out");
        assert!(catalog.contains("…"), "long descriptions are truncated");
        assert!(!catalog.contains(&"x".repeat(101)));
    }
}

#[cfg(test)]
mod workflow_tests;
