//! Creation, improvement, and reapproval share review and persistence phases.
//! Only an accepted review reaches script writes or checksum recording.

use super::decoding::{DecodeMode, decode_script};
use super::{
    TOOL_IMPROVE_TEMPLATE, TOOL_NEW_TEMPLATE, ToolLang, ToolManifest, call_llm_content,
    load_manifest, print_source, require_api_key, require_name, require_root, save_manifest,
    script_path, tool_checksum, tool_dir,
};
use crate::Theme;
use crate::confirmation::confirm_tool_approval;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(super) enum ToolWorkflowRequest {
    New { name: String, description: String },
    Improve { name: String, instructions: String },
    Approve(String),
}

struct ToolTarget {
    root: PathBuf,
    name: String,
}

enum ToolGeneration {
    New {
        description: String,
    },
    Improve {
        lang: ToolLang,
        current: String,
        instructions: String,
    },
}

enum DraftKind {
    New { description: String },
    Improve,
}

struct ToolDraft {
    lang: ToolLang,
    body: String,
    kind: DraftKind,
}

enum ToolReview {
    Draft(ToolDraft),
    Existing { lang: ToolLang, source: String },
}

enum ToolWorkflowState {
    Validating(ToolWorkflowRequest),
    Generating(ToolTarget, ToolGeneration),
    Reviewing(ToolTarget, ToolReview),
    WritingScript(ToolTarget, ToolDraft),
    RecordingApproval {
        target: ToolTarget,
        source: ApprovalSource,
        success_message: String,
    },
    Complete(String),
    Discarded(&'static str),
    Failed(Box<dyn std::error::Error>),
}

pub(super) fn run_tool_workflow(
    request: ToolWorkflowRequest,
    model: &str,
    theme: &Theme,
) -> ToolResult<()> {
    run_tool_workflow_with(
        request,
        theme,
        require_root,
        |notice, prompt| {
            let api_key = require_api_key()?;
            println!("{}", theme.helper_text(notice));
            call_llm_content(prompt, model, &api_key)
        },
        confirm_tool_approval,
    )
}

pub(super) fn run_tool_workflow_with<R, G, C>(
    request: ToolWorkflowRequest,
    theme: &Theme,
    mut locate_root: R,
    mut generate: G,
    mut confirm: C,
) -> ToolResult<()>
where
    R: FnMut() -> Result<PathBuf, String>,
    G: FnMut(&str, String) -> ToolResult<String>,
    C: FnMut(&str, &Theme) -> io::Result<bool>,
{
    let mut state = ToolWorkflowState::Validating(request);
    loop {
        state = match state {
            ToolWorkflowState::Validating(request) => {
                prepare_request(request, &mut locate_root).unwrap_or_else(ToolWorkflowState::Failed)
            }
            ToolWorkflowState::Generating(target, generation) => {
                match generate_draft(&target.name, generation, &mut generate) {
                    Ok(draft) => ToolWorkflowState::Reviewing(target, ToolReview::Draft(draft)),
                    Err(error) => ToolWorkflowState::Failed(error),
                }
            }
            ToolWorkflowState::Reviewing(target, review) => {
                let (lang, source, question, discarded) = match &review {
                    ToolReview::Draft(draft) => match draft.kind {
                        DraftKind::New { .. } => (
                            draft.lang,
                            draft.body.as_str(),
                            "Save and approve this script? [y/N]".to_string(),
                            "Discarded — nothing was saved.",
                        ),
                        DraftKind::Improve => (
                            draft.lang,
                            draft.body.as_str(),
                            "Apply and approve this new version? [y/N]".to_string(),
                            "Discarded — the existing tool is unchanged.",
                        ),
                    },
                    ToolReview::Existing { lang, source } => (
                        *lang,
                        source.as_str(),
                        format!("Approve this script for '${}'? [y/N]", target.name),
                        "Not approved.",
                    ),
                };
                print_source(theme, &target.name, lang, source);
                match confirm(&question, theme) {
                    Ok(false) => ToolWorkflowState::Discarded(discarded),
                    Err(error) => ToolWorkflowState::Failed(error.into()),
                    Ok(true) => match review {
                        ToolReview::Draft(draft) => ToolWorkflowState::WritingScript(target, draft),
                        ToolReview::Existing { .. } => {
                            let success_message =
                                format!("Approved. Run it with: ${} <args>", target.name);
                            ToolWorkflowState::RecordingApproval {
                                target,
                                source: ApprovalSource::Current,
                                success_message,
                            }
                        }
                    },
                }
            }
            ToolWorkflowState::WritingScript(target, draft) => match write_draft(&target, draft) {
                Ok((source, success_message)) => ToolWorkflowState::RecordingApproval {
                    target,
                    source,
                    success_message,
                },
                Err(error) => ToolWorkflowState::Failed(error),
            },
            ToolWorkflowState::RecordingApproval {
                target,
                source,
                success_message,
            } => match record_approval(&target.root, &target.name, source) {
                Ok(()) => ToolWorkflowState::Complete(success_message),
                Err(error) => ToolWorkflowState::Failed(error),
            },
            ToolWorkflowState::Complete(message) => {
                println!("{}", theme.helper_text(&message));
                return Ok(());
            }
            ToolWorkflowState::Discarded(message) => {
                println!("{message}");
                return Ok(());
            }
            ToolWorkflowState::Failed(error) => return Err(error),
        };
    }
}

fn prepare_request<R>(
    request: ToolWorkflowRequest,
    locate_root: &mut R,
) -> ToolResult<ToolWorkflowState>
where
    R: FnMut() -> Result<PathBuf, String>,
{
    match request {
        ToolWorkflowRequest::New { name, description } => {
            require_name(&name, "tool new <name> <what it should do>")?;
            let description = description.trim().to_string();
            if description.is_empty() {
                return Err("Usage: tool new <name> <what it should do>".into());
            }
            let root = locate_root()?;
            if tool_dir(&root, &name).exists() {
                return Err(format!(
                    "Tool '{name}' already exists — use `tool improve {name} <instructions>` or `tool rm {name}` first."
                ).into());
            }
            Ok(ToolWorkflowState::Generating(
                ToolTarget { root, name },
                ToolGeneration::New { description },
            ))
        }
        ToolWorkflowRequest::Improve { name, instructions } => {
            require_name(&name, "tool improve <name> <instructions>")?;
            let instructions = instructions.trim().to_string();
            if instructions.is_empty() {
                return Err("Usage: tool improve <name> <instructions>".into());
            }
            let root = locate_root()?;
            let manifest = load_manifest(&root, &name)?;
            let current = read_source(&root, &name, manifest.lang)?;
            Ok(ToolWorkflowState::Generating(
                ToolTarget { root, name },
                ToolGeneration::Improve {
                    lang: manifest.lang,
                    current,
                    instructions,
                },
            ))
        }
        ToolWorkflowRequest::Approve(name) => {
            require_name(&name, "tool approve <name>")?;
            let root = locate_root()?;
            let manifest = load_manifest(&root, &name)?;
            let source = read_source(&root, &name, manifest.lang)?;
            Ok(ToolWorkflowState::Reviewing(
                ToolTarget { root, name },
                ToolReview::Existing {
                    lang: manifest.lang,
                    source,
                },
            ))
        }
    }
}

fn read_source(root: &Path, name: &str, lang: ToolLang) -> Result<String, String> {
    fs::read_to_string(script_path(root, name, lang))
        .map_err(|error| format!("Cannot read script for '{name}': {error}"))
}

fn generate_draft<G>(
    name: &str,
    generation: ToolGeneration,
    generate: &mut G,
) -> ToolResult<ToolDraft>
where
    G: FnMut(&str, String) -> ToolResult<String>,
{
    match generation {
        ToolGeneration::New { description } => {
            let prompt = TOOL_NEW_TEMPLATE
                .replace("{name}", name)
                .replace("{description}", &description);
            let content = generate(&format!("Asking the model to write '{name}'…"), prompt)?;
            let (lang, body) = decode_script(&content, DecodeMode::New)?;
            Ok(ToolDraft {
                lang,
                body,
                kind: DraftKind::New { description },
            })
        }
        ToolGeneration::Improve {
            lang,
            current,
            instructions,
        } => {
            let prompt = TOOL_IMPROVE_TEMPLATE
                .replace("{name}", name)
                .replace("{lang}", lang.as_str())
                .replace("{current_script}", &current)
                .replace("{instructions}", &instructions);
            let content = generate(&format!("Asking the model to improve '{name}'…"), prompt)?;
            let (lang, body) = decode_script(&content, DecodeMode::Improve { name, lang })?;
            Ok(ToolDraft {
                lang,
                body,
                kind: DraftKind::Improve,
            })
        }
    }
}

fn write_draft(target: &ToolTarget, draft: ToolDraft) -> ToolResult<(ApprovalSource, String)> {
    let path = script_path(&target.root, &target.name, draft.lang);
    match draft.kind {
        DraftKind::New { description } => {
            fs::create_dir_all(tool_dir(&target.root, &target.name))?;
            fs::write(&path, &draft.body)?;
            Ok((
                ApprovalSource::Created {
                    manifest: ToolManifest {
                        description,
                        lang: draft.lang,
                        approved_sha256: None,
                    },
                    script: draft.body.into_bytes(),
                },
                format!(
                    "Saved {} — run it with: ${} <args>",
                    path.display(),
                    target.name
                ),
            ))
        }
        DraftKind::Improve => {
            fs::write(&path, &draft.body)?;
            Ok((
                ApprovalSource::Current,
                format!("Updated. Run it with: ${} <args>", target.name),
            ))
        }
    }
}

enum ApprovalSource {
    Current,
    Created {
        manifest: ToolManifest,
        script: Vec<u8>,
    },
}

#[derive(Clone, Copy)]
enum ApprovalOrigin {
    Existing,
    Created,
}

enum ApprovalState {
    LoadingCurrent,
    ReadingCurrent(ToolManifest),
    Hashing {
        manifest: ToolManifest,
        script: Vec<u8>,
        origin: ApprovalOrigin,
    },
    Saving {
        manifest: ToolManifest,
        origin: ApprovalOrigin,
    },
    Approved,
    Failed(Box<dyn std::error::Error>),
}

fn record_approval(root: &Path, name: &str, source: ApprovalSource) -> ToolResult<()> {
    let mut state = match source {
        ApprovalSource::Current => ApprovalState::LoadingCurrent,
        ApprovalSource::Created { manifest, script } => ApprovalState::Hashing {
            manifest,
            script,
            origin: ApprovalOrigin::Created,
        },
    };
    loop {
        state = match state {
            ApprovalState::LoadingCurrent => match load_manifest(root, name) {
                Ok(manifest) => ApprovalState::ReadingCurrent(manifest),
                Err(error) => ApprovalState::Failed(error.into()),
            },
            ApprovalState::ReadingCurrent(manifest) => {
                match fs::read(script_path(root, name, manifest.lang)) {
                    Ok(script) => ApprovalState::Hashing {
                        manifest,
                        script,
                        origin: ApprovalOrigin::Existing,
                    },
                    Err(error) => ApprovalState::Failed(
                        format!("Cannot read script for '{name}': {error}").into(),
                    ),
                }
            }
            ApprovalState::Hashing {
                mut manifest,
                script,
                origin,
            } => {
                manifest.approved_sha256 = Some(tool_checksum(manifest.lang, &script));
                ApprovalState::Saving { manifest, origin }
            }
            ApprovalState::Saving { manifest, origin } => {
                match save_manifest(root, name, &manifest) {
                    Ok(()) => ApprovalState::Approved,
                    Err(error) => ApprovalState::Failed(match origin {
                        ApprovalOrigin::Existing => {
                            format!("Could not save manifest for '{name}': {error}").into()
                        }
                        ApprovalOrigin::Created => error.into(),
                    }),
                }
            }
            ApprovalState::Approved => return Ok(()),
            ApprovalState::Failed(error) => return Err(error),
        };
    }
}
