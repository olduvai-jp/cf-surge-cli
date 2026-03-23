use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Clone, Debug)]
struct LoggedRequest {
    method: String,
    url: String,
    authorization: Option<String>,
}

#[derive(Clone)]
struct StubResponse {
    status: u16,
    body: String,
}

struct StubServerGuard {
    api_base: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<LoggedRequest>>>,
}

impl StubServerGuard {
    fn start<F>(handler: F) -> Self
    where
        F: Fn(&Method, &str, &str) -> StubResponse + Send + Sync + 'static,
    {
        let server = Server::http("127.0.0.1:0").expect("server bind");
        let socket = server.server_addr().to_ip().expect("ip socket");
        let api_base = format!("http://{}", socket);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let requests = Arc::new(Mutex::new(Vec::<LoggedRequest>::new()));
        let requests_for_thread = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let thread = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::SeqCst) {
                let mut request = match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(request)) => request,
                    Ok(None) => continue,
                    Err(_) => break,
                };

                let method = request.method().clone();
                let url = request.url().to_string();
                let authorization = request
                    .headers()
                    .iter()
                    .find(|item| item.field.equiv("authorization"))
                    .map(|item| item.value.to_string());
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                requests_for_thread
                    .lock()
                    .expect("requests lock")
                    .push(LoggedRequest {
                        method: method.as_str().to_string(),
                        url: url.clone(),
                        authorization,
                    });

                let response = handler(&method, &url, &body);
                let mut http_response = Response::from_string(response.body)
                    .with_status_code(StatusCode(response.status));
                http_response = http_response.with_header(
                    Header::from_bytes(&b"content-type"[..], &b"application/json"[..])
                        .expect("content-type header"),
                );
                let _ = request.respond(http_response);
            }
        });

        Self {
            api_base,
            stop,
            thread: Some(thread),
            requests,
        }
    }

    fn requests(&self) -> Vec<LoggedRequest> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Drop for StubServerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn config_path_for_home(home: &Path) -> PathBuf {
    home.join(".config").join("cfsurge").join("config.json")
}

fn run_cli(args: &[&str], env: &[(&str, &str)], stdin_text: Option<&str>) -> std::process::Output {
    let mut command = Command::cargo_bin("cfsurge").expect("cargo bin");
    command.args(args);
    command.env_remove("CFSURGE_API_BASE");
    command.env_remove("CFSURGE_TOKEN");
    command.env_remove("CFSURGE_CLI_VERSION");
    for (key, value) in env {
        command.env(key, value);
    }
    if let Some(text) = stdin_text {
        command.write_stdin(text.to_string());
    }
    command.output().expect("run cli")
}

#[test]
fn login_reads_token_from_second_prompt_line() {
    let server = StubServerGuard::start(|method, url, _body| {
        if *method == Method::Get && url == "/v1/meta" {
            return StubResponse {
                status: 200,
                body: r#"{"apiBase":"http://127.0.0.1:1","publicSuffix":"example.test","tokenCreationUrl":null}"#
                    .to_string(),
            };
        }
        if *method == Method::Post && url == "/v1/auth/verify" {
            return StubResponse {
                status: 200,
                body: r#"{"ok":true,"actor":"cf-token:two-step"}"#.to_string(),
            };
        }
        StubResponse {
            status: 404,
            body: "not found".to_string(),
        }
    });

    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let stdin_payload = format!("{}\ntoken-two-step\n", server.api_base);
    let output = run_cli(
        &["login"],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        Some(&stdin_payload),
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("API base URL: "));
    assert!(stdout.contains("Cloudflare API token: "));
    assert!(stdout.contains("logged in as cf-token:two-step"));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].url, "/v1/auth/verify");
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-two-step")
    );
}

#[test]
fn version_prints_dev_fallback_when_not_injected() {
    let output = run_cli(&["--version"], &[("CFSURGE_CLI_VERSION", "")], None);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0.0.0-dev\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn version_prints_injected_release_version() {
    let output = run_cli(&["--version"], &[("CFSURGE_CLI_VERSION", "v0.1.0")], None);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "v0.1.0\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn help_output_includes_version_flag() {
    let output = run_cli(&["--help"], &[], None);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--version"));
}

#[test]
fn login_verifies_token_and_writes_config() {
    let server = StubServerGuard::start(|method, url, _body| {
        if *method == Method::Get && url == "/v1/meta" {
            return StubResponse {
                status: 200,
                body: r#"{"apiBase":"http://127.0.0.1:1","publicSuffix":"example.test","tokenCreationUrl":null}"#
                    .to_string(),
            };
        }
        if *method == Method::Post && url == "/v1/auth/verify" {
            return StubResponse {
                status: 200,
                body: r#"{"ok":true,"actor":"cf-token:test"}"#.to_string(),
            };
        }
        StubResponse {
            status: 404,
            body: "not found".to_string(),
        }
    });
    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let output = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--token",
            "token-login",
        ],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        None,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "logged in as cf-token:test\n"
    );

    let config_path = config_path_for_home(temp_home.path());
    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path).expect("read config"))
            .expect("parse config");
    assert_eq!(
        config.get("apiBase").and_then(Value::as_str),
        Some(server.api_base.as_str())
    );
    assert_eq!(
        config.get("tokenStorage").and_then(Value::as_str),
        Some("file")
    );
    assert_eq!(
        config.get("token").and_then(Value::as_str),
        Some("token-login")
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "/v1/meta");
    assert_eq!(requests[0].authorization, None);
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].url, "/v1/auth/verify");
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-login")
    );
}

#[test]
fn login_prompts_for_api_base_when_not_configured() {
    let server = StubServerGuard::start(|method, url, _body| {
        if *method == Method::Get && url == "/v1/meta" {
            return StubResponse {
                status: 200,
                body: r#"{"apiBase":"http://127.0.0.1:1","publicSuffix":"example.test","tokenCreationUrl":null}"#
                    .to_string(),
            };
        }
        if *method == Method::Post && url == "/v1/auth/verify" {
            return StubResponse {
                status: 200,
                body: r#"{"ok":true,"actor":"cf-token:prompted-api"}"#.to_string(),
            };
        }
        StubResponse {
            status: 404,
            body: "not found".to_string(),
        }
    });

    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let input = format!("{}\n", server.api_base);
    let output = run_cli(
        &["login", "--token", "token-from-flag"],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        Some(&input),
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("API base URL: "));
    assert!(stdout.contains("logged in as cf-token:prompted-api"));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url, "/v1/meta");
    assert_eq!(requests[1].url, "/v1/auth/verify");
}

#[test]
fn login_fails_with_clear_error_when_prompted_api_base_is_empty() {
    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let output = run_cli(
        &["login", "--token", "token-from-flag"],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        Some("\n"),
    );

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("API base URL: "));
    assert!(stderr.contains(
        "invalid API base URL: expected absolute http(s) URL like https://api.example.com"
    ));
}

#[test]
fn login_without_token_uses_metadata_token_creation_url() {
    let server = StubServerGuard::start(|method, url, _body| {
        if *method == Method::Get && url == "/v1/meta" {
            return StubResponse {
                status: 200,
                body: r#"{"apiBase":"http://127.0.0.1:1","publicSuffix":"example.test","tokenCreationUrl":"https://dash.cloudflare.com/profile/api-tokens?foo=bar"}"#
                    .to_string(),
            };
        }
        if *method == Method::Post && url == "/v1/auth/verify" {
            return StubResponse {
                status: 200,
                body: r#"{"ok":true,"actor":"cf-token:prompt"}"#.to_string(),
            };
        }
        StubResponse {
            status: 404,
            body: "not found".to_string(),
        }
    });

    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let output = run_cli(
        &["login", "--api-base", &server.api_base],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        Some("token-from-prompt\n"),
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Create a Cloudflare API token if you do not have one:"));
    assert!(stdout.contains("https://dash.cloudflare.com/profile/api-tokens?foo=bar"));
    assert!(stdout.contains("Cloudflare API token: "));
    assert!(stdout.contains("logged in as cf-token:prompt"));
}

#[test]
fn login_without_token_falls_back_to_generic_token_url_when_metadata_fails() {
    let server = StubServerGuard::start(|method, url, _body| {
        if *method == Method::Get && url == "/v1/meta" {
            return StubResponse {
                status: 500,
                body: r#"{"error":"boom"}"#.to_string(),
            };
        }
        if *method == Method::Post && url == "/v1/auth/verify" {
            return StubResponse {
                status: 200,
                body: r#"{"ok":true,"actor":"cf-token:prompt"}"#.to_string(),
            };
        }
        StubResponse {
            status: 404,
            body: "not found".to_string(),
        }
    });

    let temp_home = TempDir::new().expect("temp home");
    let home = temp_home.path().to_string_lossy().to_string();
    let output = run_cli(
        &["login", "--api-base", &server.api_base],
        &[
            ("HOME", &home),
            ("USERPROFILE", &home),
            ("PATH", "/nonexistent"),
        ],
        Some("token-from-prompt\n"),
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("https://dash.cloudflare.com/profile/api-tokens"));
}
