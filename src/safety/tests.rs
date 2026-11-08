use super::assessment::{API_URL, MAX_RESPONSE_BYTES, assess_at};
use super::cache::{
    AssessmentCache, CACHE_TTL_SECONDS, CacheContext, CachedAssessment, MAX_CACHE_BYTES,
    MAX_CACHE_ENTRIES, NEXT_CACHE_WRITE, assess_with, cache_key, read_cache, write_cache,
};
use super::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn allowed() -> Value {
    json!({"type":"choice", "choice":"read_only", "confidence":0.99,
        "probabilities":{"read_only":0.99, "changes_state":0.0, "uncertain":0.01}})
}

struct TempCache(std::path::PathBuf);

impl TempCache {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ask-jev-cache-test-{}-{}",
            std::process::id(),
            NEXT_CACHE_WRITE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> std::path::PathBuf {
        self.0.join("jev-cache.json")
    }
}

impl Drop for TempCache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn positive_assessment() -> Assessment {
    serde_json::from_value(allowed()).unwrap()
}

#[test]
fn cached_assessment_survives_reload_without_another_model_call() {
    use std::os::unix::fs::PermissionsExt;
    let cache = TempCache::new();
    let scope = json!({"cwd":"/private/synthetic/project", "shell":"zsh", "path":"/bin"});
    let command = "echo synthetic-private-marker";
    let first = with_test_cache(command, &cache.path(), &scope, 100, || {
        Ok(positive_assessment())
    })
    .unwrap();
    let second = with_test_cache(command, &cache.path(), &scope, 101, || {
        panic!("cache hit must not call Jev")
    })
    .unwrap();
    assert!(first.permits_auto() && second.permits_auto());
    let persisted = fs::read_to_string(cache.path()).unwrap();
    assert!(!persisted.contains(command));
    assert!(!persisted.contains("synthetic-private-marker"));
    assert!(!persisted.contains("/private/synthetic/project"));
    assert!(persisted.contains("checked_at"));
    assert_eq!(
        fs::metadata(cache.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let stored = read_cache(&cache.path()).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(
        stored
            .keys()
            .all(|key| key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()))
    );
}

#[test]
fn changed_commands_and_execution_context_miss_the_cache() {
    let cache = TempCache::new();
    let scope = json!({"cwd":"/one", "shell":"zsh", "path":"/bin"});
    with_test_cache("ls -la", &cache.path(), &scope, 100, || {
        Ok(positive_assessment())
    })
    .unwrap();
    let calls = std::cell::Cell::new(0);
    for (command, changed_scope) in [
        ("ls -l", scope.clone()),
        ("ls  -la", scope.clone()),
        (
            "ls -la",
            json!({"cwd":"/two", "shell":"zsh", "path":"/bin"}),
        ),
        (
            "ls -la",
            json!({"cwd":"/one", "shell":"bash", "path":"/bin"}),
        ),
        (
            "ls -la",
            json!({"cwd":"/one", "shell":"zsh", "path":"/other/bin"}),
        ),
    ] {
        with_test_cache(command, &cache.path(), &changed_scope, 101, || {
            calls.set(calls.get() + 1);
            Ok(positive_assessment())
        })
        .unwrap();
    }
    assert_eq!(calls.get(), 5);
}

#[test]
fn expired_future_corrupt_and_oversized_entries_require_a_fresh_check() {
    let scope = json!({});
    for now in [99, 100 + CACHE_TTL_SECONDS, 100 + CACHE_TTL_SECONDS + 1] {
        let cache = TempCache::new();
        with_test_cache("ls", &cache.path(), &scope, 100, || {
            Ok(positive_assessment())
        })
        .unwrap();
        let result = with_test_cache("ls", &cache.path(), &scope, now, || {
            Err("fresh check required")
        });
        assert_eq!(result.unwrap_err(), "fresh check required");
    }
    for contents in [
        b"invalid json".to_vec(),
        vec![b' '; MAX_CACHE_BYTES as usize + 1],
    ] {
        let cache = TempCache::new();
        fs::write(cache.path(), contents).unwrap();
        let result = with_test_cache("ls", &cache.path(), &scope, 100, || {
            Err("fresh check required")
        });
        assert_eq!(result.unwrap_err(), "fresh check required");
    }
}

#[test]
fn uncertain_invalid_and_failed_assessments_are_never_cached() {
    for value in [
        json!({"type":"choice", "choice":"uncertain", "confidence":1.0,
            "probabilities":{"read_only":0.0,"changes_state":0.0,"uncertain":1.0}}),
        json!({"type":"choice", "choice":"read_only", "confidence":0.5,
            "probabilities":{"read_only":0.8,"changes_state":0.0,"uncertain":0.2}}),
        json!({"type":"choice", "choice":"read_only", "confidence":1.0,
            "probabilities":{"read_only":1.0}}),
    ] {
        let cache = TempCache::new();
        let result = with_test_cache("ls", &cache.path(), &json!({}), 100, || {
            Ok(serde_json::from_value(value).unwrap())
        })
        .unwrap();
        assert!(!result.permits_auto());
        assert!(!cache.path().exists());
    }
    let cache = TempCache::new();
    assert!(with_test_cache("ls", &cache.path(), &json!({}), 100, || Err("offline")).is_err());
    assert!(!cache.path().exists());
}

#[test]
fn cached_data_cannot_bypass_local_policy_or_current_confidence_checks() {
    let cache = TempCache::new();
    let scope = json!({});
    let mut stored = AssessmentCache::new();
    stored.insert(
        cache_key("rm -rf build", &scope),
        CachedAssessment {
            checked_at: 100,
            assessment: positive_assessment(),
        },
    );
    let mut invalid = positive_assessment();
    invalid.confidence = 0.5;
    stored.insert(
        cache_key("ls", &scope),
        CachedAssessment {
            checked_at: 100,
            assessment: invalid,
        },
    );
    write_cache(&cache.path(), &stored).unwrap();
    assert!(
        with_test_cache("rm -rf build", &cache.path(), &scope, 101, || panic!(
            "locally blocked"
        ))
        .is_err()
    );
    assert_eq!(
        with_test_cache("ls", &cache.path(), &scope, 101, || Err(
            "fresh check required"
        ))
        .unwrap_err(),
        "fresh check required"
    );
}

#[test]
fn cache_capacity_is_bounded_and_write_failures_preserve_fresh_results() {
    let cache = TempCache::new();
    let scope = json!({});
    let stored: AssessmentCache = (0..MAX_CACHE_ENTRIES)
        .map(|i| {
            (
                cache_key(&format!("echo {i}"), &scope),
                CachedAssessment {
                    checked_at: if i == 0 { 99 } else { 100 },
                    assessment: positive_assessment(),
                },
            )
        })
        .collect();
    write_cache(&cache.path(), &stored).unwrap();
    with_test_cache("ls", &cache.path(), &scope, 101, || {
        Ok(positive_assessment())
    })
    .unwrap();
    let loaded = read_cache(&cache.path()).unwrap();
    assert_eq!(loaded.len(), MAX_CACHE_ENTRIES);
    assert!(loaded.contains_key(&cache_key("ls", &scope)));
    assert!(!loaded.contains_key(&cache_key("echo 0", &scope)));
    // A file used as a parent makes writes fail even when tests run as root.
    let impossible_path = cache.path().join("nested-cache.json");
    assert!(
        with_test_cache("ls", &impossible_path, &scope, 101, || Ok(
            positive_assessment()
        ))
        .unwrap()
        .permits_auto()
    );
}

#[test]
fn only_complete_confident_read_only_assessments_permit_auto() {
    let parse = |value| {
        serde_json::from_value::<Assessment>(value)
            .is_ok_and(|assessment| assessment.permits_auto())
    };
    assert!(parse(allowed()));
    for (field, value) in [
        ("type", json!("noul")),
        ("choice", json!("changes_state")),
        ("choice", json!("uncertain")),
        ("confidence", json!(0.98)),
        ("confidence", json!(1.1)),
        ("confidence", json!(null)),
        ("probabilities", json!({"read_only":1.0})),
        (
            "probabilities",
            json!({"read_only":0.99, "changes_state":0.5, "uncertain":0.0}),
        ),
        (
            "probabilities",
            json!({"read_only":0.98, "changes_state":0.0, "uncertain":0.02}),
        ),
    ] {
        let mut value_to_test = allowed();
        value_to_test[field] = value;
        assert!(!parse(value_to_test), "invalid {field} must prompt");
    }
    assert!(!parse(json!({})));
}

#[derive(Deserialize)]
struct Case {
    command: String,
    read_only: bool,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("../../tests/fixtures/command-safety.json")).unwrap()
}

#[test]
fn local_policy_blocks_writes_even_with_a_perfect_model_verdict() {
    for case in cases() {
        if !case.read_only {
            assert!(!eligible(&case.command), "must prompt: {}", case.command);
        }
    }
    for command in [
        "file -C -m custom.magic",
        "hostname changed-name",
        "git status\nrm x",
        "ls >result",
        "unknown-program --read-only",
        "echo $SHELL",
        "echo *(e:touch marker:)",
    ] {
        assert!(!eligible(command), "must prompt: {command}");
    }
    for command in ["ls -la", "git status --short", "du -sh .", "rg --files src"] {
        assert!(eligible(command), "should reach Jev: {command}");
    }
}

fn mock_response(
    status: u16,
    body: String,
    declared_length: Option<usize>,
) -> Result<Assessment, &'static str> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut byte = [0];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
        }
        let headers = String::from_utf8(bytes).unwrap();
        assert!(headers.starts_with("POST / HTTP/1.1\r\n"));
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer synthetic-key\r\n")
        );
        assert!(
            headers
                .to_lowercase()
                .contains("content-type: application/json\r\n")
        );
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse().unwrap())
            })
            .unwrap();
        let mut body_bytes = vec![0; length];
        stream.read_exact(&mut body_bytes).unwrap();
        let request: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(request["model"], MODEL);
        assert_eq!(
            request["state"],
            json!({"command":"ls -la","shell":"macOS zsh"})
        );
        write!(
            stream,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            declared_length.unwrap_or(body.len())
        )
        .unwrap();
    });
    let result = assess_at("ls -la", "synthetic-key", &format!("http://{address}"));
    server.join().unwrap();
    result
}

#[test]
fn http_contract_and_service_failures() {
    let good = json!({"answers":{"safety":allowed()}}).to_string();
    assert!(mock_response(200, good, None).unwrap().permits_auto());
    assert_eq!(
        mock_response(503, "synthetic private provider error".into(), None).unwrap_err(),
        "Jev safety check unavailable"
    );
    for body in ["{}", "invalid json"] {
        assert_eq!(
            mock_response(200, body.into(), None).unwrap_err(),
            "invalid Jev safety response"
        );
    }
    assert_eq!(
        mock_response(200, " ".repeat(MAX_RESPONSE_BYTES as usize + 2), None).unwrap_err(),
        "oversized Jev safety response"
    );
    assert_eq!(
        mock_response(
            200,
            "synthetic private truncated response".into(),
            Some(100)
        )
        .unwrap_err(),
        "invalid Jev safety response"
    );
}

#[test]
fn cache_key_and_file_format_match_the_pre_refactor_fixture() {
    // Captured from the prior implementation; existing cache entries need no migration.
    let key = "6d91d07fc318e75f271c972226fd8fac38c71bb201ebbe890770e794ddde3762";
    let encoded = r#"{"6d91d07fc318e75f271c972226fd8fac38c71bb201ebbe890770e794ddde3762":{"checked_at":100,"assessment":{"type":"choice","choice":"read_only","confidence":0.99,"probabilities":{"changes_state":0.0,"read_only":0.99,"uncertain":0.01}}}}"#;
    let scope = json!({"cwd":"/synthetic/project","path":"/bin","shell":"/bin/zsh"});
    assert_eq!(cache_key("ls -la", &scope), key);
    let cache = TempCache::new();
    fs::write(cache.path(), encoded).unwrap();
    assert!(
        with_test_cache("ls -la", &cache.path(), &scope, 101, || {
            panic!("the saved fixture must be a cache hit")
        })
        .unwrap()
        .permits_auto()
    );
    assert_eq!(fs::read_to_string(cache.path()).unwrap(), encoded);

    fs::remove_file(cache.path()).unwrap();
    with_test_cache("ls -la", &cache.path(), &scope, 100, || {
        Ok(positive_assessment())
    })
    .unwrap();
    assert_eq!(fs::read_to_string(cache.path()).unwrap(), encoded);
}

#[test]
fn ineligible_commands_stop_before_context_resolution_or_assessment() {
    assert_eq!(
        assess_with(
            "rm -rf build",
            || panic!("blocked commands must not resolve cache context"),
            || panic!("blocked commands must not reach the provider"),
        )
        .unwrap_err(),
        "command requires confirmation"
    );
}

#[test]
fn missing_cache_context_returns_one_fresh_result_or_the_provider_error() {
    for succeeds in [true, false] {
        let mut calls = 0;
        let result = assess_with(
            "ls -la",
            || None,
            || {
                calls += 1;
                if succeeds {
                    Ok(positive_assessment())
                } else {
                    Err("synthetic provider failure")
                }
            },
        );
        assert_eq!(calls, 1);
        if succeeds {
            assert!(result.unwrap().permits_auto());
        } else {
            assert_eq!(result.unwrap_err(), "synthetic provider failure");
        }
    }
}

#[test]
fn over_capacity_cache_files_require_fresh_assessment_even_for_a_matching_key() {
    let cache = TempCache::new();
    let scope = json!({});
    let mut entries: AssessmentCache = (0..MAX_CACHE_ENTRIES)
        .map(|index| {
            (
                format!("{index:064x}"),
                CachedAssessment {
                    checked_at: 100,
                    assessment: positive_assessment(),
                },
            )
        })
        .collect();
    entries.insert(
        cache_key("ls -la", &scope),
        CachedAssessment {
            checked_at: 100,
            assessment: positive_assessment(),
        },
    );
    write_cache(&cache.path(), &entries).unwrap();
    assert!(read_cache(&cache.path()).is_none());
    assert_eq!(
        with_test_cache("ls -la", &cache.path(), &scope, 101, || Err(
            "fresh check required"
        ))
        .unwrap_err(),
        "fresh check required"
    );
}

#[test]
fn failed_cache_publication_cleans_only_its_temporary_file() {
    let cache = TempCache::new();
    // A directory cannot be replaced by the temporary cache file.
    fs::create_dir(cache.path()).unwrap();
    fs::write(cache.path().join("sentinel"), b"keep directory").unwrap();
    let unrelated = cache.0.join(".jev-cache-unrelated.tmp");
    fs::write(&unrelated, b"keep unrelated write").unwrap();
    let entries = AssessmentCache::new();
    assert!(write_cache(&cache.path(), &entries).is_err());
    assert_eq!(
        fs::read(cache.path().join("sentinel")).unwrap(),
        b"keep directory"
    );
    assert_eq!(fs::read(&unrelated).unwrap(), b"keep unrelated write");
    assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 2);
}

#[test]
fn oversized_commands_fail_before_contacting_the_provider() {
    for command in [
        "x".repeat(MAX_COMMAND_BYTES + 1),
        "é".repeat(MAX_COMMAND_BYTES / 2 + 1),
    ] {
        assert_eq!(
            assess_at(&command, "synthetic-key", "http://127.0.0.1:9").unwrap_err(),
            "command too long to assess"
        );
    }
}

#[test]
fn decoded_uncertainty_and_provider_failure_keep_distinct_batch_fallbacks() {
    use crate::execution::AutoExecution;

    let mut auto = AutoExecution::new(true, false);
    let mut uncertain = allowed();
    uncertain["choice"] = json!("uncertain");
    assert!(!auto.permits_with("ls -la", |command| {
        assess_with(
            command,
            || None,
            || {
                mock_response(
                    200,
                    json!({"answers":{"safety":uncertain}}).to_string(),
                    None,
                )
            },
        )
        .map(|assessment| assessment.permits_auto())
    }));
    assert!(auto.permits_with("pwd", |_| Ok(true)));

    assert!(!auto.permits_with("ls -la", |command| {
        assess_with(command, || None, || mock_response(200, "{}".into(), None))
            .map(|assessment| assessment.permits_auto())
    }));
    assert!(!auto.permits_with("pwd", |_| panic!(
        "provider failure disables later assessment"
    )));
}

#[test]
#[ignore = "live OpenRouter classification only; never executes the fixture commands"]
fn live_jev_safety_evaluation() {
    let key = std::env::var("OPENROUTER_ASK_API_KEY").expect("set OPENROUTER_ASK_API_KEY");
    let mut accepted_reads = 0;
    let mut total_reads = 0;
    for case in cases() {
        let start = std::time::Instant::now();
        let assessment = assess_at(&case.command, &key, API_URL).expect("live Jev request failed");
        eprintln!(
            "{} => {} p(read_only)={:.2} confidence={:.2} {:?}",
            case.command,
            assessment.choice,
            assessment.probabilities["read_only"],
            assessment.confidence,
            start.elapsed()
        );
        if case.read_only {
            total_reads += 1;
            accepted_reads += usize::from(assessment.permits_auto());
        } else {
            assert!(
                !assessment.permits_auto(),
                "unsafe false approval: {}",
                case.command
            );
        }
    }
    eprintln!("Read-only auto approvals: {accepted_reads}/{total_reads}; unsafe approvals: 0");
    assert!(
        accepted_reads > 0,
        "the check must have some utility on known read-only commands"
    );
}

fn with_test_cache<F>(
    command: &str,
    path: &Path,
    scope: &Value,
    now: u64,
    fresh: F,
) -> Result<Assessment, &'static str>
where
    F: FnOnce() -> Result<Assessment, &'static str>,
{
    assess_with(
        command,
        || Some(CacheContext::new(command, path, scope, now)),
        fresh,
    )
}
