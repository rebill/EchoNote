//! Contract tests against a scripted server: request shape, streaming, errors, retries,
//! partial streams, cancel and timeouts for both APIs.

mod mock;

use std::future::Future;
use std::time::{Duration, Instant};

use echonote_llm::{Api, Cancel, Chat, Client, Endpoint, LlmError, Message, Policy, Request};
use mock::{Reply, serve};

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn endpoint(api: Api, base: String) -> Endpoint {
    Endpoint {
        api,
        base_url: base,
        model: "test-model".into(),
        api_key: Some("sk-test-secret".into()),
        headers: Vec::new(),
        proxy: None,
        allow_http: false,
    }
}

fn policy() -> Policy {
    Policy {
        connect_timeout: Duration::from_secs(2),
        idle_timeout: Duration::from_millis(800),
        attempts: 3,
        first_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(50),
    }
}

fn client(api: Api, base: String) -> Client {
    Client::new(endpoint(api, base), policy()).unwrap()
}

fn request() -> Request {
    Request {
        system: "You summarize meetings.".into(),
        messages: vec![Message::user("[00:01] 我：开会")],
        max_tokens: 512,
        temperature: Some(0.2),
    }
}

/// OpenAI chunk events with one content delta each, then a finish reason and `[DONE]`.
fn openai_events(parts: &[&str], finish: &str) -> Vec<String> {
    let mut events: Vec<String> = parts
        .iter()
        .map(|part| {
            let chunk = serde_json::json!({"choices": [{"delta": {"content": part}, "finish_reason": null}]});
            format!("data: {chunk}\n\n")
        })
        .collect();
    let end = serde_json::json!({"choices": [{"delta": {}, "finish_reason": finish}]});
    events.push(format!("data: {end}\n\n"));
    events.push("data: [DONE]\n\n".into());
    events
}

fn anthropic_events(parts: &[&str], stop: &str) -> Vec<String> {
    let mut events = vec![
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n".to_owned(),
        "event: ping\ndata: {\"type\":\"ping\"}\n\n".to_owned(),
    ];
    for part in parts {
        let delta = serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": part}});
        events.push(format!("event: content_block_delta\ndata: {delta}\n\n"));
    }
    let stop = serde_json::json!({"type": "message_delta", "delta": {"stop_reason": stop}});
    events.push(format!("event: message_delta\ndata: {stop}\n\n"));
    events.push("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_owned());
    events
}

fn events(list: &[String]) -> Reply {
    let pieces: Vec<&str> = list.iter().map(String::as_str).collect();
    Reply::events(&pieces)
}

/// Stream `request` and collect the text.
async fn collect(
    client: &Client,
    cancel: &Cancel,
) -> (Result<echonote_llm::Finish, LlmError>, String) {
    let mut text = String::new();
    let result = client
        .stream(&request(), cancel, &mut |chunk| text.push_str(chunk))
        .await;
    (result, text)
}

#[test]
fn openai_requests_stream_and_reassemble_split_events() {
    run(async {
        let stream = openai_events(&["会议", "纪要", "：完成"], "stop").concat();
        // The same events cut at arbitrary byte positions, mid UTF-8 sequence included.
        let bytes = stream.as_bytes();
        let cuts = [
            0,
            7,
            19,
            20,
            55,
            56,
            bytes.len() / 2,
            bytes.len() - 3,
            bytes.len(),
        ];
        let mut pieces = Vec::new();
        for pair in cuts.windows(2) {
            pieces.push((Duration::ZERO, bytes[pair[0]..pair[1]].to_vec()));
        }
        let mut reply = Reply::events(&[]);
        reply.chunks = pieces;
        let server = serve(vec![reply]).await;
        let client = client(Api::OpenAi, server.base());
        let (result, text) = collect(&client, &Cancel::new()).await;
        assert!(!result.unwrap().truncated);
        assert_eq!(text, "会议纪要：完成");
        let seen = &server.requests()[0];
        assert_eq!(
            (seen.method.as_str(), seen.path.as_str()),
            ("POST", "/v1/chat/completions")
        );
        assert_eq!(seen.header("authorization"), Some("Bearer sk-test-secret"));
        let body = seen.json();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "[00:01] 我：开会");
    });
}

#[test]
fn anthropic_requests_use_messages_and_report_truncation() {
    run(async {
        let server = serve(vec![events(&anthropic_events(
            &["Hello", " world"],
            "max_tokens",
        ))])
        .await;
        let client = client(Api::Anthropic, server.base());
        let (result, text) = collect(&client, &Cancel::new()).await;
        assert!(result.unwrap().truncated);
        assert_eq!(text, "Hello world");
        let seen = &server.requests()[0];
        assert_eq!(seen.path, "/v1/messages");
        assert_eq!(seen.header("x-api-key"), Some("sk-test-secret"));
        assert_eq!(seen.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(seen.header("authorization"), None);
        let body = seen.json();
        assert_eq!(body["system"], "You summarize meetings.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["max_tokens"], 512);
    });
}

#[test]
fn a_server_that_ignores_streaming_still_works() {
    run(async {
        let body = r#"{"choices":[{"message":{"content":"<think>hmm</think>答案"},"finish_reason":"length"}]}"#;
        let server = serve(vec![Reply::json(200, body)]).await;
        let (result, text) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        assert!(result.unwrap().truncated);
        assert_eq!(text, "答案");
    });
}

#[test]
fn think_blocks_split_across_chunks_are_hidden() {
    run(async {
        let parts = [
            "<thi",
            "nk>reasoning ",
            "here</th",
            "ink>",
            "结论",
            "<",
            "b>",
        ];
        let server = serve(vec![events(&openai_events(&parts, "stop"))]).await;
        let (result, text) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        result.unwrap();
        assert_eq!(text, "结论<b>");
    });
}
/// A scripted reply, the error it must produce, and how many requests that takes.
type Case = (Reply, fn(&LlmError) -> bool, usize);

#[test]
fn errors_are_classified_and_only_transient_ones_retried() {
    run(async {
        let cases: Vec<Case> = vec![
            (
                Reply::json(
                    401,
                    r#"{"error":{"message":"Incorrect API key provided: sk-te****","type":"invalid_request_error","code":"invalid_api_key"}}"#,
                ),
                |e| matches!(e, LlmError::Unauthorized { status: 401, .. }),
                1,
            ),
            (
                Reply::json(
                    429,
                    r#"{"error":{"message":"You exceeded your current quota","code":"insufficient_quota"}}"#,
                ),
                |e| matches!(e, LlmError::Quota { .. }),
                1,
            ),
            (
                Reply::json(402, r#"{"error":{"message":"Insufficient Balance"}}"#),
                |e| matches!(e, LlmError::Quota { .. }),
                1,
            ),
            (
                Reply::json(
                    404,
                    r#"{"error":"model \"nope\" not found, try pulling it first"}"#,
                ),
                |e| matches!(e, LlmError::NotFound { detail } if detail.contains("not found")),
                1,
            ),
            (
                Reply::json(
                    400,
                    r#"{"error":{"message":"This model's maximum context length is 8192 tokens","code":"context_length_exceeded"}}"#,
                ),
                |e| matches!(e, LlmError::ContextTooLong { .. }),
                1,
            ),
            (
                Reply::json(
                    413,
                    r#"{"type":"error","error":{"type":"request_too_large","message":"Request exceeds the maximum size"}}"#,
                ),
                |e| matches!(e, LlmError::ContextTooLong { .. }),
                1,
            ),
            (
                Reply::new(400, "text/plain", "bad request"),
                |e| matches!(e, LlmError::Rejected { status: 400, detail } if detail == "bad request"),
                1,
            ),
        ];
        for (reply, expected, requests) in cases {
            let server = serve(vec![reply]).await;
            let (result, _) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
            let error = result.unwrap_err();
            assert!(expected(&error), "{error:?}");
            assert_eq!(server.requests().len(), requests, "{error:?}");
            assert!(!error.to_string().contains("sk-test-secret"));
        }
    });
}

#[test]
fn rate_limits_and_server_errors_are_retried_with_backoff() {
    run(async {
        let ok = openai_events(&["好"], "stop");
        let server = serve(vec![
            Reply::json(
                429,
                r#"{"error":{"message":"Rate limit reached","type":"requests"}}"#,
            )
            .header("retry-after-ms", "20"),
            Reply::json(
                529,
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
            events(&ok),
        ])
        .await;
        let started = Instant::now();
        let (result, text) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        result.unwrap();
        assert_eq!(text, "好");
        assert_eq!(server.requests().len(), 3);
        assert!(started.elapsed() >= Duration::from_millis(30));

        // Out of attempts: the last error is reported.
        let busy = || Reply::json(503, r#"{"error":{"message":"busy"}}"#);
        let server = serve(vec![busy(), busy(), busy()]).await;
        let (result, _) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        assert!(matches!(result, Err(LlmError::Server { status: 503, .. })));
        assert_eq!(server.requests().len(), 3);

        // A refused connection is retried too, then reported.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let (result, _) = collect(&client(Api::OpenAi, base), &Cancel::new()).await;
        assert!(matches!(result, Err(LlmError::Connect)));
    });
}

#[test]
fn a_stream_that_breaks_after_text_is_interrupted_not_retried() {
    run(async {
        let first =
            serde_json::json!({"choices": [{"delta": {"content": "前半"}, "finish_reason": null}]});
        let partial = Reply::events(&[&format!("data: {first}\n\n")]);
        let server = serve(vec![partial, events(&openai_events(&["不该"], "stop"))]).await;
        let (result, text) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        assert!(matches!(result, Err(LlmError::Interrupted)));
        assert_eq!(text, "前半");
        assert_eq!(server.requests().len(), 1);

        // An error event before any text is retried like an error status.
        let overloaded = Reply::events(&[
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        ]);
        let server = serve(vec![
            overloaded,
            events(&anthropic_events(&["OK"], "end_turn")),
        ])
        .await;
        let (result, text) = collect(&client(Api::Anthropic, server.base()), &Cancel::new()).await;
        result.unwrap();
        assert_eq!((text.as_str(), server.requests().len()), ("OK", 2));
    });
}
#[test]
fn cancel_stops_a_running_stream_and_a_silent_server_times_out() {
    run(async {
        let slow = events(&openai_events(&["一", "二", "三", "四"], "stop"))
            .paced(Duration::from_millis(300));
        let server = serve(vec![slow]).await;
        let client = client(Api::OpenAi, server.base());
        let cancel = Cancel::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(450)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let (result, text) = collect(&client, &cancel).await;
        assert!(matches!(result, Err(LlmError::Cancelled)), "{result:?}");
        assert_eq!(text, "一");
        assert!(started.elapsed() < Duration::from_millis(700));

        // Nothing for longer than the idle timeout, on every attempt.
        let silent = || events(&openai_events(&["迟"], "stop")).paced(Duration::from_millis(1_200));
        let server = serve(vec![silent(), silent(), silent()]).await;
        let (result, _) =
            collect(&client_with(Api::OpenAi, server.base(), 2), &Cancel::new()).await;
        assert!(matches!(result, Err(LlmError::Timeout)), "{result:?}");
        assert_eq!(server.requests().len(), 2);
    });
}

fn client_with(api: Api, base: String, attempts: u32) -> Client {
    Client::new(
        endpoint(api, base),
        Policy {
            attempts,
            ..policy()
        },
    )
    .unwrap()
}

#[test]
fn redirects_are_not_followed() {
    run(async {
        let server = serve(vec![
            Reply::new(307, "text/plain", "").header("location", "http://127.0.0.1:9/steal"),
        ])
        .await;
        let (result, _) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        assert!(matches!(result, Err(LlmError::Redirected)));
        assert_eq!(server.requests().len(), 1);
    });
}

#[test]
fn refused_parameters_are_dropped_once() {
    run(async {
        let server = serve(vec![
            Reply::json(400, r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.","code":"unsupported_parameter"}}"#),
            Reply::json(400, r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model.","code":"unsupported_value"}}"#),
            events(&openai_events(&["OK"], "stop")),
        ])
        .await;
        let (result, text) = collect(&client(Api::OpenAi, server.base()), &Cancel::new()).await;
        result.unwrap();
        assert_eq!(text, "OK");
        let seen = server.requests();
        assert_eq!(seen.len(), 3);
        let last = seen[2].json();
        assert_eq!(last["max_completion_tokens"], 512);
        assert!(last.get("max_tokens").is_none() && last.get("temperature").is_none());
    });
}

#[test]
fn models_headers_and_the_connection_test() {
    run(async {
        let server = serve(vec![
            Reply::json(200, r#"{"object":"list","data":[{"id":"qwen3:8b"},{"id":"llama3.2"},{"id":"qwen3:8b"}]}"#),
            events(&openai_events(&["OK"], "stop")),
        ])
        .await;
        let mut local = endpoint(Api::OpenAi, server.base());
        local.api_key = None;
        local.headers = vec![("X-Org".into(), "org-1".into())];
        let client = Client::new(local, policy()).unwrap();
        let models = client.models(&Cancel::new()).await.unwrap();
        assert_eq!(models, ["llama3.2", "qwen3:8b"]);
        client.test(&Cancel::new()).await.unwrap();
        let seen = server.requests();
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("GET", "/v1/models")
        );
        assert_eq!(seen[0].header("authorization"), None);
        assert_eq!(seen[1].header("x-org"), Some("org-1"));
        assert_eq!(seen[1].json()["max_tokens"], 16);

        let mut bad = endpoint(Api::OpenAi, server.base());
        bad.headers = vec![("Host".into(), "evil".into())];
        let client = Client::new(bad, policy()).unwrap();
        assert!(matches!(
            client.test(&Cancel::new()).await,
            Err(LlmError::InvalidHeader)
        ));
        assert_eq!(
            server.requests().len(),
            2,
            "nothing is sent with a bad header"
        );
    });
}

#[test]
fn plain_http_to_another_host_is_refused_before_connecting() {
    let remote = endpoint(Api::OpenAi, "http://10.0.0.8:11434/v1".into());
    assert!(matches!(
        Client::new(remote, policy()),
        Err(LlmError::InsecureUrl)
    ));
    let mut proxied = endpoint(Api::OpenAi, "https://api.example.com/v1".into());
    proxied.proxy = Some("socks5://127.0.0.1:1080".into());
    assert!(matches!(
        Client::new(proxied, policy()),
        Err(LlmError::InvalidProxy)
    ));
}
