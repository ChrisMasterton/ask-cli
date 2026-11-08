#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
enum Entry {
    MakeInstall,
    ScriptInstall,
    Uninstall,
}

struct Installation {
    root: PathBuf,
}

impl Installation {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ask-install-tests-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir(root.join("shims")).unwrap();
        fs::create_dir(root.join("scripts")).unwrap();
        // Relocate copies of the real entry points, not the production paths.
        // The sudo shim also rejects every path outside this owned fixture.
        for (path, source) in [
            ("Makefile", include_str!("../Makefile")),
            ("install.sh", include_str!("../install.sh")),
            (
                "scripts/install-lifecycle.sh",
                include_str!("../scripts/install-lifecycle.sh"),
            ),
        ] {
            fs::write(
                root.join(path),
                source.replace("/usr/local/bin", root.join("bin").to_str().unwrap()),
            )
            .unwrap();
        }
        for (name, source) in [("cargo", CARGO_SHIM), ("sudo", SUDO_SHIM)] {
            let path = root.join("shims").join(name);
            fs::write(&path, source).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { root }
    }

    fn target(&self) -> PathBuf {
        self.root.join("bin/ask")
    }

    fn backup(&self) -> PathBuf {
        self.root.join("bin/ask.backup")
    }

    fn seed(&self, target: Option<&str>, backup: Option<&str>) {
        for (path, contents) in [(self.target(), target), (self.backup(), backup)] {
            if let Some(contents) = contents {
                fs::write(&path, contents).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
            }
        }
    }

    fn run(&self, entry: Entry, fail: &str, version: &str, args: &[&str]) -> Output {
        fs::write(self.root.join("trace"), "").unwrap();
        let mut command = match entry {
            Entry::MakeInstall => {
                let mut command = Command::new("make");
                command.arg("install");
                command
            }
            Entry::ScriptInstall => {
                let mut command = Command::new("bash");
                command.arg("install.sh");
                command
            }
            Entry::Uninstall => {
                let mut command = Command::new("make");
                command.arg("uninstall");
                command
            }
        };
        command
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("shims").display()),
            )
            .env("ASK_INSTALL_TEST_ROOT", &self.root)
            .env("ASK_INSTALL_TEST_FAIL", fail)
            .env("ASK_INSTALL_TEST_VERSION", version)
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .env_remove("MAKELEVEL")
            .output()
            .unwrap()
    }

    fn trace(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("trace"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const CARGO_SHIM: &str = r#"#!/bin/bash
printf 'cargo' >> "$ASK_INSTALL_TEST_ROOT/trace"
printf '|%s' "$@" >> "$ASK_INSTALL_TEST_ROOT/trace"
printf '\n' >> "$ASK_INSTALL_TEST_ROOT/trace"
if [[ "$ASK_INSTALL_TEST_FAIL" == build ]]; then
    printf 'fixture failure: build\n' >&2
    exit 23
fi
mkdir -p target/release
printf '%s\n' "$ASK_INSTALL_TEST_VERSION" > target/release/ask
chmod 640 target/release/ask
"#;

const SUDO_SHIM: &str = r#"#!/bin/bash
printf 'sudo' >> "$ASK_INSTALL_TEST_ROOT/trace"
printf '|%s' "$@" >> "$ASK_INSTALL_TEST_ROOT/trace"
printf '\n' >> "$ASK_INSTALL_TEST_ROOT/trace"
for argument in "$@"; do
    case "$argument" in
        cp|chmod|rm|mv|+x|-f|target/release/ask|"$ASK_INSTALL_TEST_ROOT"/*) ;;
        *) printf 'Refusing fixture operation outside scratch paths\n' >&2; exit 99 ;;
    esac
done
case "$1" in
    cp) if [[ "$2" == target/release/ask ]]; then stage=copy; else stage=backup; fi ;;
    chmod) stage=permissions ;;
    rm) stage=remove ;;
    mv) stage=restore ;;
    *) exit 99 ;;
esac
if [[ "$ASK_INSTALL_TEST_FAIL" == "$stage" ]]; then
    printf 'fixture failure: %s\n' "$stage" >&2
    exit 23
fi
exec "$@"
"#;

#[test]
fn both_install_entry_points_build_copy_and_enable_the_binary() {
    for entry in [Entry::MakeInstall, Entry::ScriptInstall] {
        let installation = Installation::new();
        let output = installation.run(entry, "", "release-one", &[]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(installation.target()).unwrap(),
            "release-one\n"
        );
        assert_eq!(
            fs::metadata(installation.target())
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0o111
        );
        assert!(!installation.backup().exists());
        assert_eq!(
            installation.trace(),
            [
                "cargo|build|--release".to_string(),
                format!(
                    "sudo|cp|target/release/ask|{}",
                    installation.target().display()
                ),
                format!("sudo|chmod|+x|{}", installation.target().display()),
            ]
        );
    }
}

#[test]
fn repeated_upgrades_overwrite_one_backup_and_uninstall_restores_it_once() {
    for entry in [Entry::MakeInstall, Entry::ScriptInstall] {
        let installation = Installation::new();
        installation.seed(Some("original\n"), Some("older backup\n"));
        assert!(
            installation
                .run(entry, "", "release-one", &[])
                .status
                .success()
        );
        assert_eq!(
            fs::read_to_string(installation.backup()).unwrap(),
            "original\n"
        );
        assert!(
            installation
                .run(entry, "", "release-two", &[])
                .status
                .success()
        );
        assert_eq!(
            fs::read_to_string(installation.backup()).unwrap(),
            "release-one\n"
        );
        assert!(
            installation
                .run(Entry::Uninstall, "", "", &[])
                .status
                .success()
        );
        assert_eq!(
            fs::read_to_string(installation.target()).unwrap(),
            "release-one\n"
        );
        assert!(!installation.backup().exists());
        assert!(
            installation
                .run(Entry::Uninstall, "", "", &[])
                .status
                .success()
        );
        assert!(!installation.target().exists());
    }
}

#[test]
fn uninstall_handles_absence_and_restores_a_backup_even_without_an_installed_binary() {
    for target in [None, Some("installed\n")] {
        for backup in [None, Some("backup\n")] {
            let installation = Installation::new();
            installation.seed(target, backup);
            let output = installation.run(Entry::Uninstall, "", "", &[]);
            assert!(output.status.success());
            assert_eq!(
                fs::read_to_string(installation.target()).ok().as_deref(),
                backup
            );
            assert!(!installation.backup().exists());
            assert!(
                installation
                    .trace()
                    .iter()
                    .all(|line| !line.starts_with("cargo"))
            );
            if backup.is_some() {
                assert_eq!(
                    fs::metadata(installation.target())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o640
                );
            }
        }
    }
}

#[test]
fn install_failures_stop_later_commands_and_preserve_completed_effects() {
    for entry in [Entry::MakeInstall, Entry::ScriptInstall] {
        for (stage, attempted, target, backup) in [
            ("build", 1, "original\n", "old backup\n"),
            ("backup", 2, "original\n", "old backup\n"),
            ("copy", 3, "original\n", "original\n"),
            ("permissions", 4, "new release\n", "original\n"),
        ] {
            let installation = Installation::new();
            installation.seed(Some("original\n"), Some("old backup\n"));
            let output = installation.run(entry, stage, "new release", &[]);
            assert_eq!(
                output.status.code(),
                Some(if matches!(entry, Entry::ScriptInstall) {
                    23
                } else {
                    2
                })
            );
            assert_eq!(installation.trace().len(), attempted);
            assert_eq!(fs::read_to_string(installation.target()).unwrap(), target);
            assert_eq!(fs::read_to_string(installation.backup()).unwrap(), backup);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.contains("✅"),
                "failed {stage} claimed success: {stdout}"
            );
        }
    }
}

#[test]
fn removal_and_restore_failures_do_not_recreate_or_discard_files() {
    for stage in ["remove", "restore"] {
        let installation = Installation::new();
        installation.seed(Some("installed\n"), Some("backup\n"));
        let output = installation.run(Entry::Uninstall, stage, "", &[]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(installation.target().exists(), stage == "remove");
        assert_eq!(
            fs::read_to_string(installation.backup()).unwrap(),
            "backup\n"
        );
        assert_eq!(
            installation.trace().len(),
            if stage == "remove" { 1 } else { 2 }
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("✅"));
    }
}

#[test]
fn make_builds_shared_release_once_and_silent_mode_hides_command_echoes() {
    let installation = Installation::new();
    let output = installation.run(Entry::MakeInstall, "", "release", &["--silent", "release"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("cargo build"));
    assert!(!stdout.contains("sudo cp"));
    assert!(!stdout.contains("sudo chmod"));
    assert!(stdout.contains("✅ Installed successfully!"));
    assert_eq!(
        installation
            .trace()
            .iter()
            .filter(|line| line.starts_with("cargo"))
            .count(),
        1
    );
    let output = installation.run(Entry::Uninstall, "", "", &["-s"]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sudo rm"));
}

#[test]
fn make_dry_run_has_no_effects_and_standalone_arguments_remain_ignored() {
    let installation = Installation::new();
    let output = installation.run(Entry::MakeInstall, "", "release", &["--just-print"]);
    assert!(output.status.success());
    assert!(installation.trace().is_empty());
    assert!(!installation.target().exists());
    assert!(!installation.root.join("target").exists());
    let output = installation.run(
        Entry::ScriptInstall,
        "",
        "release",
        &["uninstall", "--ignored"],
    );
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(installation.target()).unwrap(),
        "release\n"
    );
}

#[test]
fn explicit_make_ignore_errors_keeps_the_previous_recipe_continuations() {
    for stage in ["backup", "copy", "permissions"] {
        let installation = Installation::new();
        installation.seed(Some("original\n"), Some("backup\n"));
        let output = installation.run(Entry::MakeInstall, stage, "release", &["-i"]);
        assert!(output.status.success());
        assert_eq!(installation.trace().len(), 4);
        assert_eq!(
            fs::read_to_string(installation.target()).unwrap(),
            if stage == "copy" {
                "original\n"
            } else {
                "release\n"
            }
        );
        assert_eq!(
            fs::read_to_string(installation.backup()).unwrap(),
            if stage == "backup" {
                "backup\n"
            } else {
                "original\n"
            }
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("Error 23 (ignored)"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("✅ Installed successfully!"));
    }
    for stage in ["remove", "restore"] {
        let installation = Installation::new();
        installation.seed(Some("installed\n"), Some("backup\n"));
        let output = installation.run(Entry::Uninstall, stage, "", &["--ignore-errors"]);
        assert!(output.status.success());
        assert_eq!(installation.trace().len(), 2);
        assert_eq!(
            fs::read_to_string(installation.target()).ok().as_deref(),
            if stage == "remove" {
                Some("backup\n")
            } else {
                None
            }
        );
        assert_eq!(installation.backup().exists(), stage == "restore");
        assert!(String::from_utf8_lossy(&output.stderr).contains("Error 23 (ignored)"));
    }
    let installation = Installation::new();
    installation.seed(Some("installed\n"), Some("backup\n"));
    let output = installation.run(Entry::MakeInstall, "copy", "release", &["-is"]);
    assert!(output.status.success());
    assert_eq!(installation.trace().len(), 4);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fixture failure: copy"));
    assert!(!stderr.contains("(ignored)"));
}
