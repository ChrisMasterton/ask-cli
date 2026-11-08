use super::*;
use crate::tools::tests::{seed_tool, unique_tools_root};
use crate::tools::{ToolLang, tool_dir};
use std::cell::RefCell;
use std::fs;

#[test]
fn model_argument_rejections_precede_root_lookup_preview_and_execution() {
    for args in [
        "; echo x",
        "a && b",
        "$(printf x)",
        "a > b",
        "a\nb",
        "`printf x`",
    ] {
        let error = invoke_with(
            "hello",
            args,
            InvocationOrigin::Model,
            || panic!("rejected arguments must not locate HOME"),
            |_| panic!("rejected arguments must not preview"),
            |_| panic!("rejected arguments must not execute"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            format!("Refusing to run '$hello' with shell metacharacters in its arguments: {args}")
        );
    }
}

#[test]
fn approved_invocations_prepare_preview_and_execute_once_with_origin_specific_arguments() {
    let (_guard, base) = unique_tools_root("invocation-order");
    let root = base.join("folder with space and ' quote");
    seed_tool(&root, "hello", ToolLang::Python, "print('fixture')\n", true);
    for (origin, args) in [
        (
            InvocationOrigin::User,
            "  'two words' $HOME; echo user-tail  ",
        ),
        (InvocationOrigin::Model, "  'two words' --flag  "),
    ] {
        let events = RefCell::new(Vec::new());
        let execution = invoke_with(
            "hello",
            args,
            origin,
            || {
                events.borrow_mut().push("root");
                Ok(root.clone())
            },
            |_| events.borrow_mut().push("preview"),
            |_| {
                events.borrow_mut().push("execute");
                Ok("fixture output".into())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["root", "preview", "execute"]);
        assert_eq!(
            execution.command,
            format!(
                "python3 {} {}",
                shell_quote(&root.join("hello/main.py").to_string_lossy()),
                args.trim()
            )
        );
        assert_eq!(execution.result.unwrap(), "fixture output");
    }
}

#[test]
fn missing_invalid_unapproved_changed_and_broken_tools_never_preview_or_execute() {
    let (_guard, root) = unique_tools_root("invocation-rejected");
    seed_tool(&root, "pending", ToolLang::Bash, "echo pending\n", false);
    seed_tool(&root, "changed", ToolLang::Bash, "echo old\n", true);
    fs::write(tool_dir(&root, "changed").join("main.sh"), "echo edited\n").unwrap();
    seed_tool(&root, "broken", ToolLang::Bash, "echo broken\n", true);
    fs::remove_file(tool_dir(&root, "broken").join("main.sh")).unwrap();
    for (name, message) in [
        ("missing", "No tool named"),
        ("../outside", "Invalid tool name"),
        ("pending", "has not been approved"),
        ("changed", "changed since"),
        ("broken", "Cannot read script"),
    ] {
        let error = invoke_with(
            name,
            "",
            InvocationOrigin::User,
            || Ok(root.clone()),
            |_| panic!("rejected {name} must not preview"),
            |_| panic!("rejected {name} must not execute"),
        )
        .unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    let error = invoke_with(
        "hello",
        "",
        InvocationOrigin::User,
        || Err("HOME is not set; cannot locate ~/.ask/tools.".into()),
        |_| panic!("missing HOME must not preview"),
        |_| panic!("missing HOME must not execute"),
    )
    .unwrap_err();
    assert_eq!(error, "HOME is not set; cannot locate ~/.ask/tools.");
}

#[test]
fn execution_failure_retains_the_prepared_command_and_differs_from_rejection() {
    let (_guard, root) = unique_tools_root("invocation-execution-error");
    seed_tool(&root, "hello", ToolLang::Bash, "echo fixture\n", true);
    let mut previews = Vec::new();
    let mut executions = Vec::new();
    let execution = invoke_with(
        "hello",
        "",
        InvocationOrigin::User,
        || Ok(root.clone()),
        |command| previews.push(command.to_string()),
        |command| {
            executions.push(command.to_string());
            Err("partial output\nCommand exited with status exit status: 7".into())
        },
    )
    .expect("a prepared invocation returns its execution result");
    assert_eq!(
        previews.as_slice(),
        std::slice::from_ref(&execution.command)
    );
    assert_eq!(executions, previews);
    assert_eq!(
        execution.result.unwrap_err().to_string(),
        "partial output\nCommand exited with status exit status: 7"
    );
}

#[test]
fn each_invocation_inspects_the_current_script_again() {
    let (_guard, root) = unique_tools_root("invocation-freshness");
    seed_tool(&root, "hello", ToolLang::Bash, "echo before\n", true);
    let mut executions = 0;
    let mut invoke = || {
        invoke_with(
            "hello",
            "",
            InvocationOrigin::User,
            || Ok(root.clone()),
            |_| {},
            |_| {
                executions += 1;
                Ok(String::new())
            },
        )
    };
    invoke().unwrap().result.unwrap();
    fs::write(tool_dir(&root, "hello").join("main.sh"), "echo edited\n").unwrap();
    assert!(invoke().unwrap_err().contains("changed since"));
    assert_eq!(executions, 1);
}
