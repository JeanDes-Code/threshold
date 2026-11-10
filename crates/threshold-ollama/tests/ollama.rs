use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use threshold_core::{Gm, GmError, Prompt, proposal_schema};
use threshold_ollama::{OllamaConfig, OllamaGm};

fn prompt() -> Prompt {
    Prompt {
        system: "system-".into(),
        stable: "stable-".into(),
        scene: "scene".into(),
        recent: "recent".into(),
    }
}

fn serve_response(body: String) -> (String, Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| position + 4)
            {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
        }
        sender.send(String::from_utf8(request).unwrap()).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    (format!("http://{address}"), receiver, handle)
}

fn request_body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn default_config_matches_threshold() {
    let config = OllamaConfig::default();
    assert_eq!(config.base_url, "http://127.0.0.1:11434");
    assert_eq!(config.model, "gemma4:26b-a4b-it-qat");
    assert_eq!(config.num_ctx, 16_384);
    assert_eq!(config.keep_alive, "30m");
    assert_eq!(config.timeout_s, 60);
}

#[test]
fn config_changes_model_tag() {
    let config = OllamaConfig::from_toml("model = \"qwen3.8:27b\"").unwrap();
    assert_eq!(config.model, "qwen3.8:27b");
    assert_eq!(config.base_url, OllamaConfig::default().base_url);
}

#[test]
fn unreachable_server_fails_fast() {
    let gm = OllamaGm::new(OllamaConfig {
        base_url: "http://127.0.0.1:9".into(),
        timeout_s: 2,
        ..OllamaConfig::default()
    });
    let started = Instant::now();
    let result = gm.propose(&prompt(), &proposal_schema());
    assert!(matches!(result, Err(GmError::Unreachable(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
#[ignore = "needs a live Ollama"]
fn missing_model_reports_status() {
    let gm = OllamaGm::new(OllamaConfig {
        model: "no-such-model:1b".into(),
        ..OllamaConfig::default()
    });
    let result = gm.propose(&prompt(), &proposal_schema());
    assert!(
        matches!(result, Err(GmError::BadStatus(404, _))),
        "{result:?}"
    );
}

#[test]
fn propose_sends_schema_and_concatenated_prompt() {
    let response = json!({
        "message": { "role": "assistant", "content": "{\"action\":\"wait\",\"verdict\":\"allowed\"}" },
        "prompt_eval_count": 123,
        "eval_count": 45,
        "eval_duration": 2_000_000_000_u64,
        "prompt_eval_duration": 500_000_000_u64,
        "total_duration": 1_500_000_000_u64,
        "done_reason": "length"
    })
    .to_string();
    let (base_url, request, server) = serve_response(response);
    let gm = OllamaGm::new(OllamaConfig {
        base_url,
        num_ctx: 8192,
        keep_alive: "12m".into(),
        ..OllamaConfig::default()
    });

    let raw = gm.propose(&prompt(), &proposal_schema()).unwrap();
    server.join().unwrap();
    assert!(raw.contains("\"action\":\"wait\""));

    let request = request.recv().unwrap();
    let body = request_body(&request);
    assert!(request.starts_with("POST /api/chat "));
    assert_eq!(body["model"], "gemma4:26b-a4b-it-qat");
    assert_eq!(body["stream"], false);
    assert_eq!(body["think"], false);
    assert_eq!(body["format"], proposal_schema());
    assert_eq!(body["options"]["num_ctx"], 8192);
    assert_eq!(body["keep_alive"], "12m");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "system-stable-scene");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "recent");
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);

    let metrics = gm.last_metrics().unwrap();
    assert_eq!(metrics.prompt_tokens, 123);
    assert_eq!(metrics.eval_tokens, 45);
    assert_eq!(metrics.eval_duration_ns, 2_000_000_000);
    assert_eq!(metrics.prompt_eval_duration_ns, 500_000_000);
    assert_eq!(metrics.total_ms, 1500);
    assert_eq!(metrics.done_reason.as_deref(), Some("length"));
    assert_eq!(gm.last_done_reason().as_deref(), Some("length"));
}

#[test]
fn narrate_streams_ndjson_and_returns_full_text() {
    let response = [
        json!({ "message": { "content": "The door " } }),
        json!({ "message": { "content": "opens." } }),
        json!({
            "message": { "content": "" },
            "done": true,
            "prompt_eval_count": 200,
            "eval_count": 8,
            "total_duration": 750_000_000_u64
        }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let (base_url, request, server) = serve_response(response);
    let gm = OllamaGm::new(OllamaConfig {
        base_url,
        ..OllamaConfig::default()
    });
    let mut chunks = Vec::new();

    let narration = gm
        .narrate(&prompt(), &mut |chunk| chunks.push(chunk.to_owned()))
        .unwrap();
    server.join().unwrap();
    assert_eq!(narration, "The door opens.");
    assert_eq!(chunks, ["The door ", "opens.", ""]);

    let body = request_body(&request.recv().unwrap());
    assert_eq!(body["stream"], true);
    assert_eq!(body["think"], false);
    assert!(body.get("format").is_none());
    assert_eq!(body["messages"][0]["content"], "system-stable-scene");
    assert_eq!(body["messages"][1]["content"], "recent");
    let metrics = gm.last_metrics().unwrap();
    assert_eq!(metrics.prompt_tokens, 200);
    assert_eq!(metrics.eval_tokens, 8);
    assert_eq!(metrics.total_ms, 750);
}

#[test]
fn narrate_maps_a_mid_stream_error_line() {
    let response = [
        json!({ "message": { "content": "The door " } }),
        json!({ "error": "model runner has unexpectedly stopped" }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let (base_url, _request, server) = serve_response(response);
    let gm = OllamaGm::new(OllamaConfig {
        base_url,
        ..OllamaConfig::default()
    });
    let result = gm.narrate(&prompt(), &mut |_| {});
    server.join().unwrap();
    assert_eq!(
        result,
        Err(GmError::Aborted(
            "model runner has unexpectedly stopped".into()
        ))
    );
}

#[test]
fn propose_maps_an_error_body() {
    let (base_url, _request, server) =
        serve_response(json!({ "error": "out of memory" }).to_string());
    let gm = OllamaGm::new(OllamaConfig {
        base_url,
        ..OllamaConfig::default()
    });
    let result = gm.propose(&prompt(), &proposal_schema());
    server.join().unwrap();
    assert_eq!(result, Err(GmError::Aborted("out of memory".into())));
}

#[test]
fn silent_server_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Hold the connection open, never answer.
        thread::sleep(Duration::from_secs(3));
        drop(stream);
    });
    let gm = OllamaGm::new(OllamaConfig {
        base_url: format!("http://{address}"),
        timeout_s: 1,
        ..OllamaConfig::default()
    });
    let started = Instant::now();
    let result = gm.narrate(&prompt(), &mut |_| {});
    assert_eq!(result, Err(GmError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(3));
    server.join().unwrap();
}
