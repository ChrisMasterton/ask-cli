//! Positive assessment caching. Local policy is checked before resolving cache
//! context; cache failures never change a fresh assessment's execution decision.

use super::assessment::{API_URL, assess_at};
use super::{Assessment, MIN_CONFIDENCE, MIN_READ_ONLY_PROBABILITY, eligible, request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_POLICY_VERSION: u32 = 1;
pub(super) const CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
pub(super) const MAX_CACHE_ENTRIES: usize = 512;
pub(super) const MAX_CACHE_BYTES: u64 = 512 * 1024;
pub(super) static NEXT_CACHE_WRITE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize)]
pub(super) struct CachedAssessment {
    pub(super) checked_at: u64,
    pub(super) assessment: Assessment,
}

impl CachedAssessment {
    fn valid_at(&self, now: u64) -> bool {
        now.checked_sub(self.checked_at)
            .is_some_and(|age| age < CACHE_TTL_SECONDS)
            && self.assessment.permits_auto()
    }
}

pub(super) type AssessmentCache = BTreeMap<String, CachedAssessment>;

// Exact command bytes, assessment contract, thresholds, and execution context
// retain the existing key material so stored approvals remain compatible.
pub(super) fn cache_key(command: &str, scope: &Value) -> String {
    let material = json!({
        "policy_version": CACHE_POLICY_VERSION,
        "request": request(command),
        "min_probability": MIN_READ_ONLY_PROBABILITY,
        "min_confidence": MIN_CONFIDENCE,
        "scope": scope,
    });
    format!("{:x}", Sha256::digest(material.to_string().as_bytes()))
}

pub(super) struct CacheContext {
    path: PathBuf,
    key: String,
    now: u64,
}

impl CacheContext {
    pub(super) fn new(command: &str, path: &Path, scope: &Value, now: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            key: cache_key(command, scope),
            now,
        }
    }

    fn from_environment(command: &str) -> Option<Self> {
        let (Some(home), Ok(cwd), Ok(now)) = (
            std::env::var_os("HOME"),
            std::env::current_dir(),
            SystemTime::now().duration_since(UNIX_EPOCH),
        ) else {
            return None;
        };
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL").unwrap_or_default();
        let scope = json!({"cwd":cwd.as_os_str().as_bytes(), "path":path.as_bytes(), "shell":shell.as_bytes()});
        Some(Self::new(
            command,
            &Path::new(&home).join(".ask/jev-cache.json"),
            &scope,
            now.as_secs(),
        ))
    }
}

enum FreshSource {
    Uncached,
    Miss {
        context: CacheContext,
        entries: AssessmentCache,
    },
}

enum CacheState<R, F> {
    CheckingPolicy {
        resolve: R,
        fresh: F,
    },
    ResolvingContext {
        resolve: R,
        fresh: F,
    },
    Loading {
        context: CacheContext,
        fresh: F,
    },
    LookingUp {
        context: CacheContext,
        entries: AssessmentCache,
        fresh: F,
    },
    Assessing {
        source: FreshSource,
        fresh: F,
    },
    Considering {
        source: FreshSource,
        assessment: Assessment,
    },
    Evicting {
        path: PathBuf,
        entries: AssessmentCache,
        assessment: Assessment,
    },
    Writing {
        path: PathBuf,
        entries: AssessmentCache,
        assessment: Assessment,
    },
    Complete(Assessment),
    Failed(&'static str),
}

pub(crate) fn assess_cached(command: &str, api_key: &str) -> Result<Assessment, &'static str> {
    assess_with(
        command,
        || CacheContext::from_environment(command),
        || assess_at(command, api_key, API_URL),
    )
}

pub(super) fn assess_with<R, F>(
    command: &str,
    resolve: R,
    fresh: F,
) -> Result<Assessment, &'static str>
where
    R: FnOnce() -> Option<CacheContext>,
    F: FnOnce() -> Result<Assessment, &'static str>,
{
    let mut state = CacheState::CheckingPolicy { resolve, fresh };
    loop {
        state = match state {
            CacheState::CheckingPolicy { resolve, fresh } => {
                if eligible(command) {
                    CacheState::ResolvingContext { resolve, fresh }
                } else {
                    CacheState::Failed("command requires confirmation")
                }
            }
            CacheState::ResolvingContext { resolve, fresh } => match resolve() {
                Some(context) => CacheState::Loading { context, fresh },
                None => CacheState::Assessing {
                    source: FreshSource::Uncached,
                    fresh,
                },
            },
            CacheState::Loading { context, fresh } => {
                let entries = read_cache(&context.path).unwrap_or_default();
                CacheState::LookingUp {
                    context,
                    entries,
                    fresh,
                }
            }
            CacheState::LookingUp {
                context,
                mut entries,
                fresh,
            } => {
                entries.retain(|_, entry| entry.valid_at(context.now));
                match entries.get(&context.key) {
                    Some(entry) => CacheState::Complete(entry.assessment.clone()),
                    None => CacheState::Assessing {
                        source: FreshSource::Miss { context, entries },
                        fresh,
                    },
                }
            }
            CacheState::Assessing { source, fresh } => match fresh() {
                Ok(assessment) => CacheState::Considering { source, assessment },
                Err(reason) => CacheState::Failed(reason),
            },
            CacheState::Considering { source, assessment } => match source {
                FreshSource::Miss {
                    context,
                    mut entries,
                } if assessment.permits_auto() => {
                    entries.insert(
                        context.key,
                        CachedAssessment {
                            checked_at: context.now,
                            assessment: assessment.clone(),
                        },
                    );
                    CacheState::Evicting {
                        path: context.path,
                        entries,
                        assessment,
                    }
                }
                _ => CacheState::Complete(assessment),
            },
            CacheState::Evicting {
                path,
                mut entries,
                assessment,
            } => {
                if entries.len() > MAX_CACHE_ENTRIES {
                    let oldest = entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.checked_at)
                        .map(|(key, _)| key.clone())
                        .unwrap();
                    entries.remove(&oldest);
                    CacheState::Evicting {
                        path,
                        entries,
                        assessment,
                    }
                } else {
                    CacheState::Writing {
                        path,
                        entries,
                        assessment,
                    }
                }
            }
            CacheState::Writing {
                path,
                entries,
                assessment,
            } => {
                // Cache persistence is an optimization, never an execution decision.
                let _ = write_cache(&path, &entries);
                CacheState::Complete(assessment)
            }
            CacheState::Complete(assessment) => return Ok(assessment),
            CacheState::Failed(reason) => return Err(reason),
        };
    }
}

pub(super) fn read_cache(path: &Path) -> Option<AssessmentCache> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    let cache: AssessmentCache = serde_json::from_slice(&bytes).ok()?;
    (cache.len() <= MAX_CACHE_ENTRIES).then_some(cache)
}

struct TemporaryCacheFile {
    path: PathBuf,
    file: File,
}

enum CacheWriteState {
    CreatingDirectory,
    CreatingTemporary(PathBuf),
    Serializing(TemporaryCacheFile),
    Flushing(TemporaryCacheFile),
    Publishing(TemporaryCacheFile),
    Cleaning {
        temporary: TemporaryCacheFile,
        result: io::Result<()>,
    },
    Complete(io::Result<()>),
}

pub(super) fn write_cache(path: &Path, entries: &AssessmentCache) -> io::Result<()> {
    let mut state = CacheWriteState::CreatingDirectory;
    loop {
        state = match state {
            CacheWriteState::CreatingDirectory => {
                let result = (|| {
                    let parent = path
                        .parent()
                        .ok_or_else(|| io::Error::other("cache directory missing"))?;
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(parent)?;
                    Ok(parent.join(format!(
                        ".jev-cache-{}-{}.tmp",
                        std::process::id(),
                        NEXT_CACHE_WRITE.fetch_add(1, Ordering::Relaxed)
                    )))
                })();
                match result {
                    Ok(temporary) => CacheWriteState::CreatingTemporary(temporary),
                    Err(error) => CacheWriteState::Complete(Err(error)),
                }
            }
            CacheWriteState::CreatingTemporary(temporary) => match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
            {
                Ok(file) => CacheWriteState::Serializing(TemporaryCacheFile {
                    path: temporary,
                    file,
                }),
                // No file was created, so there is nothing owned by this call to remove.
                Err(error) => CacheWriteState::Complete(Err(error)),
            },
            CacheWriteState::Serializing(mut temporary) => {
                match serde_json::to_writer(&mut temporary.file, entries) {
                    Ok(()) => CacheWriteState::Flushing(temporary),
                    Err(error) => CacheWriteState::Cleaning {
                        temporary,
                        result: Err(error.into()),
                    },
                }
            }
            CacheWriteState::Flushing(mut temporary) => match temporary.file.flush() {
                Ok(()) => CacheWriteState::Publishing(temporary),
                Err(error) => CacheWriteState::Cleaning {
                    temporary,
                    result: Err(error),
                },
            },
            CacheWriteState::Publishing(temporary) => {
                let result = fs::rename(&temporary.path, path);
                CacheWriteState::Cleaning { temporary, result }
            }
            CacheWriteState::Cleaning { temporary, result } => {
                // Concurrent writers can lose entries, but never expose a partial file.
                let _ = fs::remove_file(&temporary.path);
                CacheWriteState::Complete(result)
            }
            CacheWriteState::Complete(result) => return result,
        };
    }
}
