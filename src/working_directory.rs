//! Directory changes share one transition boundary. Child reports are applied
//! after output collection, independently of the command's exit status.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CWD_CAPTURE_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) enum DirectorySnapshot {
    Known(PathBuf),
    Unavailable,
}

pub(crate) enum DirectoryChange {
    Changed {
        starting: PathBuf,
        final_dir: PathBuf,
    },
    Unchanged,
    Unavailable,
}

impl DirectorySnapshot {
    pub(crate) fn capture() -> Self {
        match env::current_dir() {
            Ok(path) => Self::Known(path),
            Err(_) => Self::Unavailable,
        }
    }

    pub(crate) fn finish(self) -> DirectoryChange {
        match (self, Self::capture()) {
            (Self::Known(starting), Self::Known(final_dir)) if starting != final_dir => {
                DirectoryChange::Changed {
                    starting,
                    final_dir,
                }
            }
            (Self::Known(_), Self::Known(_)) => DirectoryChange::Unchanged,
            _ => DirectoryChange::Unavailable,
        }
    }

    fn matches(&self, path: &Path) -> bool {
        matches!(self, Self::Known(starting) if starting == path)
    }
}

pub(crate) struct DirectoryCapture {
    starting: DirectorySnapshot,
    path: PathBuf,
}

impl DirectoryCapture {
    pub(crate) fn new() -> Self {
        let starting = DirectorySnapshot::capture();
        let path = env::temp_dir().join(format!(
            "ask-cwd-{}-{}",
            std::process::id(),
            NEXT_CWD_CAPTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        Self { starting, path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn shell_script(&self, command: &str) -> String {
        format!(
            "{command}\nask_command_status=$?\npwd -P > \"$ASK_CWD_CAPTURE\"\nexit $ask_command_status"
        )
    }

    pub(crate) fn synchronize(self) -> io::Result<()> {
        change_directory(DirectoryState::ReadingCapture(self))
    }
}

enum DirectoryCompletion {
    CleanCapture(PathBuf),
    Direct,
}

enum DirectoryState {
    ReadingCapture(DirectoryCapture),
    Comparing {
        capture: DirectoryCapture,
        destination: PathBuf,
    },
    Applying {
        destination: PathBuf,
        after: DirectoryCompletion,
    },
    CleaningCapture(PathBuf),
    Complete,
    Failed(io::Error),
}

pub(crate) fn change_to_parent() -> io::Result<()> {
    change_directory(DirectoryState::Applying {
        destination: PathBuf::from(".."),
        after: DirectoryCompletion::Direct,
    })
}

fn change_directory(mut state: DirectoryState) -> io::Result<()> {
    loop {
        state = match state {
            DirectoryState::ReadingCapture(capture) => match fs::read_to_string(&capture.path) {
                Ok(path) if !path.trim().is_empty() => DirectoryState::Comparing {
                    destination: PathBuf::from(path.trim()),
                    capture,
                },
                _ => DirectoryState::CleaningCapture(capture.path),
            },
            DirectoryState::Comparing {
                capture,
                destination,
            } => {
                if capture.starting.matches(&destination) {
                    DirectoryState::CleaningCapture(capture.path)
                } else {
                    DirectoryState::Applying {
                        destination,
                        after: DirectoryCompletion::CleanCapture(capture.path),
                    }
                }
            }
            DirectoryState::Applying { destination, after } => {
                match env::set_current_dir(destination) {
                    Ok(()) => match after {
                        DirectoryCompletion::CleanCapture(path) => {
                            DirectoryState::CleaningCapture(path)
                        }
                        DirectoryCompletion::Direct => DirectoryState::Complete,
                    },
                    // Preserve the early error: a failed directory change does not
                    // advance to capture-file cleanup.
                    Err(error) => DirectoryState::Failed(error),
                }
            }
            DirectoryState::CleaningCapture(path) => {
                let _ = fs::remove_file(path);
                DirectoryState::Complete
            }
            DirectoryState::Complete => return Ok(()),
            DirectoryState::Failed(error) => return Err(error),
        };
    }
}
