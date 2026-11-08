//! Fresh filesystem inspection is the authority for tool integrity. Listing,
//! catalog generation, and execution consume the same inspected manifest.

use super::{ToolManifest, load_manifest, script_path, tool_checksum};
use std::fs;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ToolIntegrity {
    Approved,
    Unapproved,
    Mismatch,
    Broken(String),
}

impl ToolIntegrity {
    pub(super) fn description(&self) -> &str {
        match self {
            Self::Approved => "approved",
            Self::Unapproved => "NOT approved — run `tool approve <name>` after review",
            Self::Mismatch => "EDITED since approval — re-approve after review",
            Self::Broken(reason) => reason,
        }
    }
}

#[derive(Debug)]
pub(super) struct ToolInspection {
    pub(super) manifest: ToolManifest,
    pub(super) integrity: ToolIntegrity,
}

impl ToolInspection {
    pub(super) fn into_approved_manifest(self, name: &str) -> Result<ToolManifest, String> {
        match self.integrity {
            ToolIntegrity::Approved => Ok(self.manifest),
            ToolIntegrity::Unapproved => Err(format!(
                "Tool '{name}' has not been approved yet — review it with `tool show {name}`, then `tool approve {name}`."
            )),
            ToolIntegrity::Mismatch => Err(format!(
                "The script for '{name}' changed since it was approved — refusing to run. Review it with `tool show {name}`, then `tool approve {name}`."
            )),
            ToolIntegrity::Broken(reason) => Err(reason),
        }
    }
}

enum IntegrityState {
    LoadingManifest,
    ReadingScript(ToolManifest),
    Verifying {
        manifest: ToolManifest,
        script: Vec<u8>,
    },
    Inspected(ToolInspection),
    Failed(String),
}

pub(super) fn inspect_tool(root: &Path, name: &str) -> Result<ToolInspection, String> {
    let mut state = IntegrityState::LoadingManifest;
    loop {
        state = match state {
            IntegrityState::LoadingManifest => match load_manifest(root, name) {
                Ok(manifest) => IntegrityState::ReadingScript(manifest),
                Err(reason) => IntegrityState::Failed(reason),
            },
            IntegrityState::ReadingScript(manifest) => {
                match fs::read(script_path(root, name, manifest.lang)) {
                    Ok(script) => IntegrityState::Verifying { manifest, script },
                    Err(error) => IntegrityState::Inspected(ToolInspection {
                        manifest,
                        integrity: ToolIntegrity::Broken(format!(
                            "Cannot read script for '{name}': {error}"
                        )),
                    }),
                }
            }
            IntegrityState::Verifying { manifest, script } => {
                let integrity = match &manifest.approved_sha256 {
                    None => ToolIntegrity::Unapproved,
                    Some(approved) if *approved == tool_checksum(manifest.lang, &script) => {
                        ToolIntegrity::Approved
                    }
                    Some(_) => ToolIntegrity::Mismatch,
                };
                IntegrityState::Inspected(ToolInspection {
                    manifest,
                    integrity,
                })
            }
            IntegrityState::Inspected(inspection) => return Ok(inspection),
            IntegrityState::Failed(reason) => return Err(reason),
        };
    }
}
