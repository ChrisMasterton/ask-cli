use super::tests::{seed_tool, unique_tools_root};
use super::workflow::run_tool_workflow_with;
use super::*;
use crate::ThemeMode;

#[test]
fn accepted_creation_saves_reviewed_source_and_is_available_to_catalog_and_execution() {
    let (_guard, root) = unique_tools_root("create-workflow");
    let body = "if True:\n    print('created')\n";
    let mut generations = 0;
    let mut reviews = 0;
    run_tool_workflow_with(
        ToolWorkflowRequest::New {
            name: "hello".into(),
            description: "  say hello  ".into(),
        },
        &Theme::from_mode(ThemeMode::Dark),
        || Ok(root.clone()),
        |notice, prompt| {
            generations += 1;
            assert_eq!(notice, "Asking the model to write 'hello'…");
            assert!(prompt.contains("**What it must do:** say hello"));
            Ok(format!("LANG: python\n{body}"))
        },
        |question, _| {
            reviews += 1;
            assert_eq!(question, "Save and approve this script? [y/N]");
            assert!(!tool_dir(&root, "hello").exists());
            Ok(true)
        },
    )
    .unwrap();

    assert_eq!((generations, reviews), (1, 1));
    assert_eq!(
        fs::read_to_string(script_path(&root, "hello", ToolLang::Python)).unwrap(),
        body
    );
    let inspection = inspect_tool(&root, "hello").unwrap();
    assert_eq!(inspection.integrity, ToolIntegrity::Approved);
    assert_eq!(inspection.manifest.description, "say hello");
    assert_eq!(inspection.manifest.lang, ToolLang::Python);
    assert!(catalog_block(&root).contains("$hello"));
    assert!(
        resolve_approved_tool(&root, "hello", "one")
            .unwrap()
            .starts_with("python3 ")
    );
}

#[test]
fn declined_or_failed_reviews_preserve_every_existing_file_and_save_no_draft() {
    for fail_review in [false, true] {
        for request in [
            ToolWorkflowRequest::New {
                name: "fresh".into(),
                description: "say hello".into(),
            },
            ToolWorkflowRequest::Improve {
                name: "hello".into(),
                instructions: "say more".into(),
            },
            ToolWorkflowRequest::Approve("hello".into()),
        ] {
            let (_guard, root) = unique_tools_root("review-no-write");
            seed_tool(&root, "hello", ToolLang::Bash, "echo original\n", true);
            let path = script_path(&root, "hello", ToolLang::Bash);
            let manifest_path = tool_dir(&root, "hello").join(MANIFEST_FILE);
            let before_script = fs::read(&path).unwrap();
            let before_manifest = fs::read(&manifest_path).unwrap();
            let is_approval = matches!(request, ToolWorkflowRequest::Approve(_));
            let mut generations = 0;
            let mut reviews = 0;
            let result = run_tool_workflow_with(
                request,
                &Theme::from_mode(ThemeMode::Dark),
                || Ok(root.clone()),
                |_, _| {
                    generations += 1;
                    Ok("LANG: bash\necho revised\n".into())
                },
                |_, _| {
                    reviews += 1;
                    if fail_review {
                        Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "review closed",
                        ))
                    } else {
                        Ok(false)
                    }
                },
            );
            assert_eq!(result.is_err(), fail_review);
            assert_eq!(generations, usize::from(!is_approval));
            assert_eq!(reviews, 1);
            assert!(!tool_dir(&root, "fresh").exists());
            assert_eq!(fs::read(&path).unwrap(), before_script);
            assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
        }
    }
}

#[test]
fn failed_generation_and_invalid_new_drafts_do_not_reach_review_or_persistence() {
    for content in [None, Some("echo missing-header\n"), Some("LANG: bash\n\n")] {
        let (_guard, root) = unique_tools_root("generation-failure");
        let result = run_tool_workflow_with(
            ToolWorkflowRequest::New {
                name: "hello".into(),
                description: "say hello".into(),
            },
            &Theme::from_mode(ThemeMode::Dark),
            || Ok(root.clone()),
            |_, _| match content {
                Some(body) => Ok(body.into()),
                None => Err("synthetic generation failure".into()),
            },
            |_, _| panic!("failed generation must not reach review"),
        );
        assert!(result.is_err());
        assert!(!tool_dir(&root, "hello").exists());
    }
}

#[test]
fn invalid_requests_stop_before_generation_and_review() {
    let (_guard, root) = unique_tools_root("request-guards");
    seed_tool(&root, "exists", ToolLang::Bash, "echo keep\n", true);
    for request in [
        ToolWorkflowRequest::New {
            name: "../bad".into(),
            description: "say hello".into(),
        },
        ToolWorkflowRequest::New {
            name: "hello".into(),
            description: "   ".into(),
        },
        ToolWorkflowRequest::New {
            name: "exists".into(),
            description: "replace it".into(),
        },
        ToolWorkflowRequest::Improve {
            name: "exists".into(),
            instructions: " ".into(),
        },
        ToolWorkflowRequest::Improve {
            name: "missing".into(),
            instructions: "fix it".into(),
        },
        ToolWorkflowRequest::Approve("missing".into()),
    ] {
        assert!(
            run_tool_workflow_with(
                request,
                &Theme::from_mode(ThemeMode::Dark),
                || Ok(root.clone()),
                |_, _| panic!("invalid requests must not call the model"),
                |_, _| panic!("invalid requests must not reach review"),
            )
            .is_err()
        );
    }
    assert_eq!(
        fs::read_to_string(script_path(&root, "exists", ToolLang::Bash)).unwrap(),
        "echo keep\n"
    );
}

#[test]
fn improvement_keeps_language_description_indentation_and_supported_reply_formats() {
    let body = "if True:\n    print('revised')\n";
    for content in [
        body.to_string(),
        format!("```python\n{body}```"),
        format!("LANG: python\n{body}"),
    ] {
        let (_guard, root) = unique_tools_root("improve-workflow");
        seed_tool(
            &root,
            "hello",
            ToolLang::Python,
            "print('original')\n",
            true,
        );
        let before = load_manifest(&root, "hello").unwrap();
        run_tool_workflow_with(
            ToolWorkflowRequest::Improve {
                name: "hello".into(),
                instructions: "say revised".into(),
            },
            &Theme::from_mode(ThemeMode::Dark),
            || Ok(root.clone()),
            |_, prompt| {
                assert!(prompt.contains("print('original')"));
                assert!(prompt.contains("**Improvement instructions:** say revised"));
                Ok(content.clone())
            },
            |question, _| {
                assert_eq!(question, "Apply and approve this new version? [y/N]");
                assert_eq!(
                    fs::read_to_string(script_path(&root, "hello", ToolLang::Python)).unwrap(),
                    "print('original')\n"
                );
                Ok(true)
            },
        )
        .unwrap();
        let after = inspect_tool(&root, "hello").unwrap();
        assert_eq!(after.integrity, ToolIntegrity::Approved);
        assert_eq!(after.manifest.description, before.description);
        assert_eq!(after.manifest.lang, before.lang);
        assert_ne!(after.manifest.approved_sha256, before.approved_sha256);
        assert_eq!(
            fs::read_to_string(script_path(&root, "hello", ToolLang::Python)).unwrap(),
            body
        );
    }
}

#[test]
fn empty_or_language_changing_improvements_never_reach_review_or_change_the_tool() {
    for content in [
        "LANG: python\nprint('wrong language')\n",
        " \n\n",
        "```bash\n```",
    ] {
        let (_guard, root) = unique_tools_root("invalid-improvement");
        seed_tool(&root, "hello", ToolLang::Bash, "echo original\n", true);
        let manifest_path = tool_dir(&root, "hello").join(MANIFEST_FILE);
        let before_manifest = fs::read(&manifest_path).unwrap();
        let result = run_tool_workflow_with(
            ToolWorkflowRequest::Improve {
                name: "hello".into(),
                instructions: "fix it".into(),
            },
            &Theme::from_mode(ThemeMode::Dark),
            || Ok(root.clone()),
            |_, _| Ok(content.into()),
            |_, _| panic!("invalid improvement must not reach review"),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
        assert_eq!(
            fs::read_to_string(script_path(&root, "hello", ToolLang::Bash)).unwrap(),
            "echo original\n"
        );
    }
}

#[test]
fn reapproval_records_current_script_bytes_without_generation() {
    let (_guard, root) = unique_tools_root("reapprove-workflow");
    seed_tool(&root, "hello", ToolLang::Bash, "echo original\n", true);
    fs::write(script_path(&root, "hello", ToolLang::Bash), "echo edited\n").unwrap();
    assert_eq!(
        inspect_tool(&root, "hello").unwrap().integrity,
        ToolIntegrity::Mismatch
    );
    run_tool_workflow_with(
        ToolWorkflowRequest::Approve("hello".into()),
        &Theme::from_mode(ThemeMode::Dark),
        || Ok(root.clone()),
        |_, _| panic!("approval must not call the model"),
        |question, _| {
            assert_eq!(question, "Approve this script for '$hello'? [y/N]");
            Ok(true)
        },
    )
    .unwrap();
    let after = inspect_tool(&root, "hello").unwrap();
    assert_eq!(after.integrity, ToolIntegrity::Approved);
    assert_eq!(
        after.manifest.approved_sha256.unwrap(),
        tool_checksum(ToolLang::Bash, b"echo edited\n")
    );
}

#[test]
fn partial_save_failures_leave_written_scripts_unexecutable() {
    for improve in [false, true] {
        let (_guard, root) = unique_tools_root("partial-save");
        if improve {
            seed_tool(&root, "hello", ToolLang::Bash, "echo original\n", true);
        }
        let request = if improve {
            ToolWorkflowRequest::Improve {
                name: "hello".into(),
                instructions: "fix it".into(),
            }
        } else {
            ToolWorkflowRequest::New {
                name: "hello".into(),
                description: "say hello".into(),
            }
        };
        let result = run_tool_workflow_with(
            request,
            &Theme::from_mode(ThemeMode::Dark),
            || Ok(root.clone()),
            |_, _| Ok("LANG: bash\necho revised\n".into()),
            |_, _| {
                let manifest_path = tool_dir(&root, "hello").join(MANIFEST_FILE);
                if improve {
                    fs::remove_file(&manifest_path).unwrap();
                }
                fs::create_dir_all(manifest_path).unwrap();
                Ok(true)
            },
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(script_path(&root, "hello", ToolLang::Bash)).unwrap(),
            "echo revised\n"
        );
        assert!(resolve_approved_tool(&root, "hello", "").is_err());
        assert!(!catalog_block(&root).contains("$hello"));
    }
}

#[test]
fn failed_script_writes_never_update_the_approval_manifest() {
    let (_guard, root) = unique_tools_root("script-write-failure");
    seed_tool(&root, "hello", ToolLang::Bash, "echo original\n", true);
    let manifest_path = tool_dir(&root, "hello").join(MANIFEST_FILE);
    let before_manifest = fs::read(&manifest_path).unwrap();
    let result = run_tool_workflow_with(
        ToolWorkflowRequest::Improve {
            name: "hello".into(),
            instructions: "fix it".into(),
        },
        &Theme::from_mode(ThemeMode::Dark),
        || Ok(root.clone()),
        |_, _| Ok("echo revised\n".into()),
        |_, _| {
            let path = script_path(&root, "hello", ToolLang::Bash);
            fs::remove_file(&path).unwrap();
            fs::create_dir(path).unwrap();
            Ok(true)
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert!(matches!(
        inspect_tool(&root, "hello").unwrap().integrity,
        ToolIntegrity::Broken(_)
    ));
}
