use super::*;
use serde::Deserialize;
use serde_json::{Value, json};

#[test]
fn improvement_decoding_preserves_python_indentation() {
    let reply = "```python\ndef main():\n    for i in range(3):\n\n        print(i)\n```\n";
    let (lang, body) = decode_script(
        reply,
        DecodeMode::Improve {
            name: "hello",
            lang: ToolLang::Python,
        },
    )
    .unwrap();
    assert_eq!(lang, ToolLang::Python);
    assert_eq!(
        body,
        "def main():\n    for i in range(3):\n\n        print(i)\n"
    );
}

#[test]
fn new_tool_decoding_handles_decoration_and_fences() {
    let (lang, body) =
        decode_script("LANG: bash\n#!/bin/bash\necho hi\n", DecodeMode::New).unwrap();
    assert_eq!(lang, ToolLang::Bash);
    assert_eq!(body, "#!/bin/bash\necho hi\n");
    let (lang, body) = decode_script(
        "```\n# LANG: python\nimport sys\n\nif True:\n    print(1)\n```",
        DecodeMode::New,
    )
    .unwrap();
    assert_eq!(lang, ToolLang::Python);
    assert_eq!(body, "import sys\n\nif True:\n    print(1)\n");
    for reply in ["echo hi\n", "LANG: ruby\nputs 1\n", "LANG: bash\n\n"] {
        assert!(decode_script(reply, DecodeMode::New).is_err());
    }
}

#[derive(Deserialize)]
struct Fixture {
    name: String,
    mode: String,
    content: String,
    expected: Value,
}

#[test]
fn decoding_matches_pre_refactor_generation_fixtures() {
    // Captured from the original production generate_draft function with fixed
    // model replies, including its body-before-language validation and fallback.
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/generated-script-decoding.json"
    ))
    .unwrap();
    for fixture in fixtures {
        let mode = match fixture.mode.as_str() {
            "new" => DecodeMode::New,
            "improve-bash" => DecodeMode::Improve {
                name: "hello",
                lang: ToolLang::Bash,
            },
            "improve-python" => DecodeMode::Improve {
                name: "hello",
                lang: ToolLang::Python,
            },
            _ => panic!("unknown fixture mode"),
        };
        let actual = match decode_script(&fixture.content, mode) {
            Ok((lang, body)) => json!({"lang":lang.as_str(), "body":body}),
            Err(error) => json!({"error":error}),
        };
        assert_eq!(actual, fixture.expected, "{}", fixture.name);
    }
}
