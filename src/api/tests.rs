use super::*;
use response::{commands_from_api_response, tool_content_from_api_response};
use std::io::{Read, Write};
use std::net::TcpListener;

fn respond(
    status: u16,
    body: &[u8],
    declared_length: Option<usize>,
) -> (ApiResult<ApiResponse>, Value) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
    let body = body.to_vec();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut headers = Vec::new();
        let mut byte = [0];
        while !headers.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
        }
        let headers = String::from_utf8(headers).unwrap().to_lowercase();
        assert!(headers.starts_with("post /chat/completions http/1.1\r\n"));
        assert!(headers.contains("authorization: bearer synthetic-key\r\n"));
        assert!(headers.contains("content-type: application/json\r\n"));
        let length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .map(|length| length.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let mut request = vec![0; length];
        stream.read_exact(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            declared_length.unwrap_or(body.len())
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        serde_json::from_slice::<Value>(&request).unwrap()
    });
    let messages = vec![
        json!({"role":"system","content":"prior context"}),
        json!({"role":"user","content":"request 🚀"}),
    ];
    let result = call_llm_at(messages, "vendor/model", "synthetic-key", &endpoint);
    (result, server.join().unwrap())
}

#[test]
fn chat_request_keeps_the_wire_contract_and_decodes_the_first_choice() {
    let body = json!({"choices":[
        {"message":{"content":" # explanation\n mkdir app && cd app \n"}},
        {"message":{"content":"ignored second choice"}},
    ], "usage":{"total_tokens":10}})
    .to_string();
    let (response, request) = respond(200, body.as_bytes(), None);
    assert_eq!(
        request,
        json!({"model":"vendor/model", "messages":[
            {"role":"system","content":"prior context"}, {"role":"user","content":"request 🚀"}
        ]})
    );
    assert_eq!(
        commands_from_api_response(response.unwrap()).unwrap(),
        ["# explanation", "mkdir app && cd app"]
    );
}

#[test]
fn http_error_bodies_keep_their_existing_error_text_and_read_failure_fallback() {
    let (result, _) = respond(429, br#"{"error":"quota fixture"}"#, None);
    assert_eq!(
        result.unwrap_err().to_string(),
        "API error 429: {\"error\":\"quota fixture\"}"
    );
    for (body, length, expected) in [
        (&b"\xff"[..], None, "API error 503: \u{fffd}"),
        (&b"partial"[..], Some(100), "API error 503: "),
    ] {
        let (result, _) = respond(503, body, length);
        assert_eq!(result.unwrap_err().to_string(), expected);
    }
}

#[test]
fn invalid_json_schema_and_truncated_success_bodies_fail_before_content_parsing() {
    for (body, length) in [
        (&b"{"[..], None),
        (&b"{}"[..], None),
        (&br#"{"choices":[{"message":{"content":null}}]}"#[..], None),
        (&br#"{"choices":[]}"#[..], Some(100)),
    ] {
        let (result, _) = respond(200, body, length);
        let error = result.unwrap_err().to_string();
        assert!(!error.starts_with("Network error:"));
        assert!(!error.starts_with("API error"));
        assert!(!error.starts_with("No command returned"));
    }
    let (result, _) = respond(200, br#"{"choices":[]}"#, None);
    assert_eq!(
        commands_from_api_response(result.unwrap())
            .unwrap_err()
            .to_string(),
        "No command returned from the model."
    );
}

#[test]
fn unavailable_endpoint_stays_a_network_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = call_llm_at(Vec::new(), "vendor/model", "synthetic-key", &endpoint).unwrap_err();
    assert!(error.to_string().starts_with("Network error: "));
}

#[test]
fn raw_tool_response_uses_the_same_request_and_preserves_indentation() {
    let content = "\nLANG: python\n```python\nif True:\n    print('fixture')\n```\n";
    let body = json!({"choices":[{"message":{"content":content}}]}).to_string();
    let (response, _) = respond(200, body.as_bytes(), None);
    assert_eq!(
        tool_content_from_api_response(response.unwrap()).unwrap(),
        content.trim()
    );
}
