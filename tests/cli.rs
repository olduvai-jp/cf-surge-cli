use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;
use tempfile::TempDir;
use tiny_http::{Header, Response, Server, StatusCode};

#[derive(Clone, Debug)]
struct RequestRecord {
    method: String,
    url: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body: String,
}

#[derive(Clone)]
struct StubResponse {
    status: u16,
    body: String,
    content_type: &'static str,
}

impl StubResponse {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            content_type: "application/json",
        }
    }

    fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            content_type: "text/plain",
        }
    }
}

struct StubServer {
    api_base: String,
    requests: Arc<Mutex<Vec<RequestRecord>>>,
}

impl StubServer {
    fn recorded(&self) -> Vec<RequestRecord> {
        self.requests.lock().unwrap().clone()
    }
}

fn run_cli(
    args: &[&str],
    envs: &[(&str, &str)],
    stdin_text: &str,
    cwd: Option<&Path>,
) -> CliResult {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cfsurge"));
    command.args(args);
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.env_remove("CFSURGE_API_BASE");
    command.env_remove("CFSURGE_TOKEN");
    command.env_remove("CFSURGE_USERNAME");
    command.env_remove("CFSURGE_PASSWORD");
    command.env_remove("CFSURGE_CLI_VERSION");
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn().unwrap();
    if !stdin_text.is_empty() {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_text.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    CliResult {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

struct CliResult {
    code: i32,
    stdout: String,
    stderr: String,
}

fn start_stub_server<F>(handler: F) -> StubServer
where
    F: Fn(&str, &RequestRecord) -> StubResponse + Send + Sync + 'static,
{
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = server.server_addr().to_ip().unwrap();
    let requests = Arc::new(Mutex::new(Vec::<RequestRecord>::new()));
    let shared_requests = Arc::clone(&requests);
    let handler = Arc::new(handler);
    let api_base = format!("http://127.0.0.1:{}", address.port());
    let shared_api_base = api_base.clone();

    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            let record = RequestRecord {
                method: request.method().as_str().to_string(),
                url: request.url().to_string(),
                authorization: request
                    .headers()
                    .iter()
                    .find(|header| {
                        header
                            .field
                            .as_str()
                            .as_str()
                            .eq_ignore_ascii_case("authorization")
                    })
                    .map(|header| header.value.as_str().to_string()),
                content_type: request
                    .headers()
                    .iter()
                    .find(|header| {
                        header
                            .field
                            .as_str()
                            .as_str()
                            .eq_ignore_ascii_case("content-type")
                    })
                    .map(|header| header.value.as_str().to_string()),
                body,
            };
            shared_requests.lock().unwrap().push(record.clone());
            let response_spec = handler(&shared_api_base, &record);
            let response = Response::from_string(response_spec.body)
                .with_status_code(StatusCode(response_spec.status))
                .with_header(
                    Header::from_bytes(&b"content-type"[..], response_spec.content_type.as_bytes())
                        .unwrap(),
                );
            let _ = request.respond(response);
        }
    });

    StubServer { api_base, requests }
}

fn config_path_for_home(home: &Path) -> PathBuf {
    home.join(".config").join("cfsurge").join("config.json")
}

fn site_config_path_for_dir(directory: &Path) -> PathBuf {
    directory.join(".cfsurge.json")
}

fn write_config(home: &Path, config: &str) {
    let config_path = config_path_for_home(home);
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(config_path, config).unwrap();
}

#[cfg(target_os = "macos")]
fn create_fake_security_command(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = root.join("bin");
    let store_path = root.join("fake-keychain-token.txt");
    let log_path = root.join("fake-keychain.log");
    let script_path = bin_dir.join("security");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(
        &script_path,
        r#"#!/bin/sh
cmd="${1:-}"
if [ -n "${FAKE_SECURITY_LOG:-}" ]; then
  printf '%s\n' "$*" >> "${FAKE_SECURITY_LOG}"
fi

if [ "$cmd" = "help" ]; then
  exit 0
fi

if [ -z "${FAKE_SECURITY_STORE:-}" ]; then
  echo "missing FAKE_SECURITY_STORE" >&2
  exit 2
fi

case "$cmd" in
  add-generic-password)
    token=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "-w" ]; then
        shift
        token="${1:-}"
        break
      fi
      shift
    done
    printf '%s' "$token" > "${FAKE_SECURITY_STORE}"
    ;;
  find-generic-password)
    if [ ! -f "${FAKE_SECURITY_STORE}" ]; then
      echo "The specified item could not be found in the keychain" >&2
      exit 44
    fi
    cat "${FAKE_SECURITY_STORE}"
    ;;
  delete-generic-password)
    if [ ! -f "${FAKE_SECURITY_STORE}" ]; then
      echo "The specified item could not be found in the keychain" >&2
      exit 44
    fi
    rm -f "${FAKE_SECURITY_STORE}"
    ;;
  *)
    echo "unsupported fake security command" >&2
    exit 1
    ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).unwrap();
    (bin_dir, store_path, log_path)
}

fn create_site_directory(root: &Path, relative_dir: &str) -> PathBuf {
    let site_dir = root.join(relative_dir);
    fs::create_dir_all(&site_dir).unwrap();
    fs::write(site_dir.join("index.html"), "<h1>hello</h1>").unwrap();
    site_dir
}

fn home_envs(home: &TempDir) -> [(&str, &str); 3] {
    [
        ("HOME", home.path().to_str().unwrap()),
        ("USERPROFILE", home.path().to_str().unwrap()),
        ("PATH", "/nonexistent"),
    ]
}

#[test]
fn version_prints_development_fallback() {
    let result = run_cli(&["--version"], &[("CFSURGE_CLI_VERSION", "")], "", None);
    assert_eq!(result.code, 0);
    assert_eq!(result.stderr, "");
    assert_eq!(result.stdout, "0.0.0-dev\n");
}

#[test]
fn help_output_includes_version() {
    let result = run_cli(&["--help"], &[], "", None);
    assert_eq!(result.code, 0);
    assert_eq!(result.stderr, "");
    assert!(result.stdout.contains("--version"));
    assert!(
        result
            .stdout
            .contains("[--password <password>] [--new-password <password>] [--token <token>]")
    );
    assert!(
        result
            .stdout
            .contains("passwd [--current-password <password>] [--new-password <password>]")
    );
    assert!(result.stdout.contains("admin users list"));
    assert!(result.stdout.contains("interactive choices: use"));
}

#[test]
fn login_verifies_token_and_writes_config() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/auth/verify") => StubResponse::json(r#"{"actor":"cf-token:test"}"#),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--token",
            "token-login",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert_eq!(result.stdout, "logged in as cf-token:test\n");

    let config = fs::read_to_string(config_path_for_home(home.path())).unwrap();
    assert!(config.contains(&format!(r#""apiBase": "{}""#, server.api_base)));
    assert!(config.contains(r#""tokenStorage": "file""#));
    assert!(config.contains(r#""token": "token-login""#));

    let requests = server.recorded();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].url, "/v1/meta");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].url, "/v1/auth/verify");
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-login")
    );
}

#[test]
fn login_defaults_to_service_session_auth_with_username_password() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/login") => {
                assert_eq!(request.content_type.as_deref(), Some("application/json"));
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "username": "alice",
                        "password": "initial-password",
                    })
                );
                StubResponse::json(
                    r#"{"accessToken":"session-token-1","actor":"alice","username":"alice","role":"user","mustChangePassword":false}"#,
                )
            }
            ("GET", "/v1/projects") => StubResponse::json(r#"{"projects":[]}"#),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let login_result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--username",
            "alice",
            "--password",
            "initial-password",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(login_result.code, 0, "{}", login_result.stderr);
    assert_eq!(login_result.stderr, "");
    assert_eq!(login_result.stdout, "logged in as alice\n");

    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path_for_home(home.path())).unwrap())
            .unwrap();
    assert_eq!(
        config.get("apiBase").and_then(Value::as_str),
        Some(server.api_base.as_str())
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("type"))
            .and_then(Value::as_str),
        Some("service-session")
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("tokenStorage"))
            .and_then(Value::as_str),
        Some("file")
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .and_then(Value::as_str),
        Some("session-token-1")
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("mustChangePassword"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(config.get("token").is_none());

    let list_result = run_cli(&["list"], &home_envs(&home), "", None);
    assert_eq!(list_result.code, 0, "{}", list_result.stderr);
    assert_eq!(list_result.stdout, "no projects\n");
}

#[test]
fn login_rejects_token_mode_when_auth_is_service_session() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |_, _| StubResponse::text(404, "not found"));
    let result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--auth",
            "service-session",
            "--token",
            "token-1",
        ],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .contains("token-based login requires --auth cloudflare-admin")
    );
}

#[test]
fn login_completes_required_password_change_and_relogin_in_one_flow() {
    let home = TempDir::new().unwrap();
    let login_count = Arc::new(Mutex::new(0usize));
    let login_count_for_server = Arc::clone(&login_count);
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/login") => {
                let mut login_count = login_count_for_server.lock().unwrap();
                *login_count += 1;
                let body: Value = serde_json::from_str(&request.body).unwrap();
                if *login_count == 1 {
                    assert_eq!(
                        body,
                        serde_json::json!({
                            "username": "alice",
                            "password": "temporary-pass",
                        })
                    );
                    return StubResponse::json(
                        r#"{"accessToken":"session-token-temp","actor":"alice","username":"alice","role":"user","mustChangePassword":true}"#,
                    );
                }
                assert_eq!(
                    body,
                    serde_json::json!({
                        "username": "alice",
                        "password": "new-pass",
                    })
                );
                StubResponse::json(
                    r#"{"accessToken":"session-token-final","actor":"alice","username":"alice","role":"user","mustChangePassword":false}"#,
                )
            }
            ("POST", "/v1/auth/change-password") => {
                assert_eq!(
                    request.authorization.as_deref(),
                    Some("Bearer session-token-temp")
                );
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "currentPassword": "temporary-pass",
                        "newPassword": "new-pass",
                    })
                );
                StubResponse::json(r#"{"ok":true}"#)
            }
            ("GET", "/v1/projects") => {
                assert_eq!(
                    request.authorization.as_deref(),
                    Some("Bearer session-token-final")
                );
                StubResponse::json(r#"{"projects":[]}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    let login_result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--username",
            "alice",
            "--password",
            "temporary-pass",
            "--new-password",
            "new-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(login_result.code, 0, "{}", login_result.stderr);
    assert_eq!(login_result.stderr, "");
    assert_eq!(
        login_result.stdout,
        "password updated\nlogged in as alice\n"
    );
    assert_eq!(*login_count.lock().unwrap(), 2);

    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path_for_home(home.path())).unwrap())
            .unwrap();
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .and_then(Value::as_str),
        Some("session-token-final")
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("username"))
            .and_then(Value::as_str),
        Some("alice")
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("mustChangePassword"))
            .and_then(Value::as_bool),
        Some(false)
    );

    let list_result = run_cli(&["list"], &home_envs(&home), "", None);
    assert_eq!(list_result.code, 0, "{}", list_result.stderr);
    assert_eq!(list_result.stdout, "no projects\n");
}

#[test]
fn login_fails_clearly_when_password_change_required_without_new_password_in_non_interactive_mode()
{
    let home = TempDir::new().unwrap();
    let login_count = Arc::new(Mutex::new(0usize));
    let login_count_for_server = Arc::clone(&login_count);
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/login") => {
                *login_count_for_server.lock().unwrap() += 1;
                StubResponse::json(
                    r#"{"accessToken":"session-token-temp","actor":"alice","username":"alice","role":"user","mustChangePassword":true}"#,
                )
            }
            ("POST", "/v1/auth/change-password") => {
                panic!("change-password should not be called without --new-password")
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--username",
            "alice",
            "--password",
            "temporary-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(result.code, 1);
    assert!(result.stderr.contains(
        "password change required for this account. Re-run cfsurge login with --new-password <password>."
    ));
    assert_eq!(*login_count.lock().unwrap(), 1);
    assert!(!config_path_for_home(home.path()).exists());
}

#[test]
fn cloudflare_admin_login_rejects_new_password() {
    let home = TempDir::new().unwrap();
    let result = run_cli(
        &[
            "login",
            "--auth",
            "cloudflare-admin",
            "--api-base",
            "https://api.example.test",
            "--token",
            "token",
            "--new-password",
            "new-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .contains("--new-password is only available with service-session login")
    );
}

#[test]
fn must_change_password_blocks_commands_until_passwd() {
    let home = TempDir::new().unwrap();
    let login_count = Arc::new(Mutex::new(0usize));
    let login_count_for_server = Arc::clone(&login_count);
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/change-password") => {
                assert_eq!(
                    request.authorization.as_deref(),
                    Some("Bearer session-token-2")
                );
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "currentPassword": "temp-pass",
                        "newPassword": "new-pass",
                    })
                );
                StubResponse::json(r#"{"ok":true}"#)
            }
            ("POST", "/v1/auth/login") => {
                *login_count_for_server.lock().unwrap() += 1;
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "username": "bob",
                        "password": "new-pass",
                    })
                );
                StubResponse::json(
                    r#"{"accessToken":"session-token-3","actor":"bob","username":"bob","role":"user","mustChangePassword":false}"#,
                )
            }
            ("GET", "/v1/projects") => {
                assert_eq!(
                    request.authorization.as_deref(),
                    Some("Bearer session-token-3")
                );
                StubResponse::json(r#"{"projects":[]}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"auth\": {{\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"file\",\n    \"accessToken\": \"session-token-2\",\n    \"username\": \"bob\",\n    \"mustChangePassword\": true\n  }}\n}}\n",
            server.api_base
        ),
    );

    let blocked_list = run_cli(&["list"], &home_envs(&home), "", None);
    assert_eq!(blocked_list.code, 1);
    assert!(
        blocked_list
            .stderr
            .contains("password change required. Run cfsurge passwd.")
    );

    let passwd_result = run_cli(
        &[
            "passwd",
            "--current-password",
            "temp-pass",
            "--new-password",
            "new-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(passwd_result.code, 0, "{}", passwd_result.stderr);
    assert_eq!(passwd_result.stdout, "password updated\nlogged in as bob\n");
    assert_eq!(*login_count.lock().unwrap(), 1);

    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path_for_home(home.path())).unwrap())
            .unwrap();
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("mustChangePassword"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .and_then(Value::as_str),
        Some("session-token-3")
    );

    let allowed_list = run_cli(&["list"], &home_envs(&home), "", None);
    assert_eq!(allowed_list.code, 0, "{}", allowed_list.stderr);
    assert_eq!(allowed_list.stdout, "no projects\n");
}

#[cfg(target_os = "macos")]
#[test]
fn passwd_relogs_keychain_backed_service_session_token() {
    let home = TempDir::new().unwrap();
    let fake_security_root = TempDir::new().unwrap();
    let (bin_dir, store_path, log_path) = create_fake_security_command(fake_security_root.path());
    let path_value = match env::var("PATH") {
        Ok(existing) if !existing.is_empty() => format!("{}:{existing}", bin_dir.display()),
        _ => bin_dir.display().to_string(),
    };
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/change-password") => StubResponse::json(r#"{"ok":true}"#),
            ("POST", "/v1/auth/login") => {
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "username": "bob",
                        "password": "new-pass",
                    })
                );
                StubResponse::json(
                    r#"{"accessToken":"session-token-keychain-new","actor":"bob","username":"bob","role":"user","mustChangePassword":false}"#,
                )
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"auth\": {{\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"keychain\",\n    \"username\": \"bob\",\n    \"mustChangePassword\": true\n  }}\n}}\n",
            server.api_base
        ),
    );
    fs::write(&store_path, "session-token-keychain").unwrap();

    let result = run_cli(
        &[
            "passwd",
            "--current-password",
            "temp-pass",
            "--new-password",
            "new-pass",
        ],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("USERPROFILE", home.path().to_str().unwrap()),
            ("PATH", &path_value),
            ("FAKE_SECURITY_STORE", store_path.to_str().unwrap()),
            ("FAKE_SECURITY_LOG", log_path.to_str().unwrap()),
        ],
        "",
        None,
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "password updated\nlogged in as bob\n");
    assert_eq!(result.stderr, "");
    assert_eq!(
        fs::read_to_string(&store_path).unwrap(),
        "session-token-keychain-new"
    );

    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path_for_home(home.path())).unwrap())
            .unwrap();
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("mustChangePassword"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        config
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .is_none()
    );

    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("find-generic-password"));
    assert!(log.contains("add-generic-password"));
}

#[test]
fn passwd_auto_relogin_failure_clears_session_and_errors() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/auth/change-password") => StubResponse::json(r#"{"ok":true}"#),
            ("POST", "/v1/auth/login") => StubResponse::text(401, "invalid credentials"),
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"auth\": {{\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"file\",\n    \"accessToken\": \"session-token-4\",\n    \"username\": \"bob\",\n    \"mustChangePassword\": true\n  }}\n}}\n",
            server.api_base
        ),
    );

    let result = run_cli(
        &[
            "passwd",
            "--current-password",
            "temp-pass",
            "--new-password",
            "new-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(result.code, 1);
    assert!(result.stderr.contains(
        "password updated, but automatic re-login failed. Run cfsurge login with your new password."
    ));

    let config: Value =
        serde_json::from_str(&fs::read_to_string(config_path_for_home(home.path())).unwrap())
            .unwrap();
    assert_eq!(
        config
            .get("auth")
            .and_then(|auth| auth.get("mustChangePassword"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        config
            .get("auth")
            .and_then(|auth| auth.get("accessToken"))
            .is_none()
    );
}

#[test]
fn passwd_fails_when_stored_service_session_username_is_missing() {
    let home = TempDir::new().unwrap();
    write_config(
        home.path(),
        "{\n  \"apiBase\": \"https://api.example.test\",\n  \"auth\": {\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"file\",\n    \"accessToken\": \"session-token-missing-user\",\n    \"mustChangePassword\": true\n  }\n}\n",
    );

    let result = run_cli(
        &[
            "passwd",
            "--current-password",
            "temp-pass",
            "--new-password",
            "new-pass",
        ],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .contains("stored service-session username is missing. Run cfsurge login.")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn login_defaults_to_file_storage_even_when_keychain_is_available() {
    let home = TempDir::new().unwrap();
    let fake_security_root = TempDir::new().unwrap();
    let (bin_dir, store_path, log_path) = create_fake_security_command(fake_security_root.path());
    let path_value = match env::var("PATH") {
        Ok(existing) if !existing.is_empty() => format!("{}:{existing}", bin_dir.display()),
        _ => bin_dir.display().to_string(),
    };
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/auth/verify") => {
                StubResponse::json(r#"{"actor":"cf-token:file-default"}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--token",
            "token-file-default",
        ],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("USERPROFILE", home.path().to_str().unwrap()),
            ("PATH", &path_value),
            ("FAKE_SECURITY_STORE", store_path.to_str().unwrap()),
            ("FAKE_SECURITY_LOG", log_path.to_str().unwrap()),
        ],
        "",
        None,
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert_eq!(result.stdout, "logged in as cf-token:file-default\n");

    let config = fs::read_to_string(config_path_for_home(home.path())).unwrap();
    assert!(config.contains(r#""tokenStorage": "file""#));
    assert!(config.contains(r#""token": "token-file-default""#));
    assert!(!store_path.exists());
    assert!(!log_path.exists());
}

#[test]
fn login_fails_when_keychain_storage_is_unavailable() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/auth/verify") => {
                StubResponse::json(r#"{"actor":"cf-token:keychain-unavailable"}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--token",
            "token-keychain",
            "--token-storage",
            "keychain",
        ],
        &home_envs(&home),
        "",
        None,
    );

    assert_eq!(result.code, 1, "{}", result.stderr);
    assert!(
        result.stderr.contains(
            "macOS Keychain is unavailable. Run cfsurge login with --token-storage file."
        )
    );
    assert!(!config_path_for_home(home.path()).exists());
}

#[cfg(target_os = "macos")]
#[test]
fn login_stores_token_in_keychain_when_explicitly_requested_and_list_reads_it() {
    let home = TempDir::new().unwrap();
    let fake_security_root = TempDir::new().unwrap();
    let (bin_dir, store_path, log_path) = create_fake_security_command(fake_security_root.path());
    let path_value = match env::var("PATH") {
        Ok(existing) if !existing.is_empty() => format!("{}:{existing}", bin_dir.display()),
        _ => bin_dir.display().to_string(),
    };
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/auth/verify") => StubResponse::json(r#"{"actor":"cf-token:keychain"}"#),
            ("GET", "/v1/projects") => StubResponse::json(
                r#"{"projects":[{"slug":"site-a","visibility":"public","servedUrl":"https://site-a.example.test","activeDeploymentId":"dep-1","updatedAt":"2026-03-24T00:00:00.000Z","updatedBy":"cf-token:keychain"}]}"#,
            ),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let login_result = run_cli(
        &[
            "login",
            "--api-base",
            &server.api_base,
            "--token",
            "token-keychain",
            "--token-storage",
            "keychain",
        ],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("USERPROFILE", home.path().to_str().unwrap()),
            ("PATH", &path_value),
            ("FAKE_SECURITY_STORE", store_path.to_str().unwrap()),
            ("FAKE_SECURITY_LOG", log_path.to_str().unwrap()),
        ],
        "",
        None,
    );

    assert_eq!(login_result.code, 0, "{}", login_result.stderr);
    assert_eq!(login_result.stderr, "");
    assert_eq!(login_result.stdout, "logged in as cf-token:keychain\n");

    let config = fs::read_to_string(config_path_for_home(home.path())).unwrap();
    assert!(config.contains(r#""tokenStorage": "keychain""#));
    assert!(!config.contains(r#""token":"#));
    assert_eq!(fs::read_to_string(&store_path).unwrap(), "token-keychain");

    let list_result = run_cli(
        &["list"],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("USERPROFILE", home.path().to_str().unwrap()),
            ("PATH", &path_value),
            ("FAKE_SECURITY_STORE", store_path.to_str().unwrap()),
            ("FAKE_SECURITY_LOG", log_path.to_str().unwrap()),
        ],
        "",
        None,
    );

    assert_eq!(list_result.code, 0, "{}", list_result.stderr);
    assert_eq!(list_result.stderr, "");
    assert!(
        list_result.stdout.contains(
            "site-a\tpublic\thttps://site-a.example.test\tdep-1\t2026-03-24T00:00:00.000Z\tcf-token:keychain"
        )
    );

    let requests = server.recorded();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-keychain")
    );
    assert_eq!(
        requests[2].authorization.as_deref(),
        Some("Bearer token-keychain")
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("add-generic-password"));
    assert!(log.contains("find-generic-password"));
}

#[test]
fn login_prompts_for_api_base_when_unset() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/auth/verify") => {
                StubResponse::json(r#"{"actor":"cf-token:prompted-api"}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });
    let input = format!("{}\n", server.api_base);
    let result = run_cli(
        &["login", "--token", "token-from-flag"],
        &home_envs(&home),
        &input,
        None,
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert!(result.stdout.contains("API base URL: "));
    assert!(result.stdout.contains("logged in as cf-token:prompted-api"));

    let config = fs::read_to_string(config_path_for_home(home.path())).unwrap();
    assert!(config.contains(&format!(r#""apiBase": "{}""#, server.api_base)));
    assert!(config.contains(r#""tokenStorage": "file""#));
    assert!(config.contains(r#""token": "token-from-flag""#));
}

#[test]
fn init_prompts_for_api_base_and_writes_site_config() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &["init", "--slug", "my-site", "--publish-dir", "."],
        &home_envs(&home),
        &format!("{}\n", server.api_base),
        Some(project.path()),
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert!(result.stdout.contains("API base URL: "));
    assert!(result.stdout.contains("saved .cfsurge.json"));
    assert!(
        result
            .stdout
            .contains("public URL preview: https://my-site.example.test")
    );

    let site_config = fs::read_to_string(site_config_path_for_dir(project.path())).unwrap();
    assert!(site_config.contains(r#""slug": "my-site""#));
    assert!(site_config.contains(r#""publishDir": ".""#));
    assert!(site_config.contains(r#""visibility": "public""#));
}

#[test]
fn init_stores_unlisted_visibility_and_prints_preview() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","unlistedHost":"u.example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "init",
            "--api-base",
            &server.api_base,
            "--slug",
            "private-site",
            "--publish-dir",
            "public",
            "--visibility",
            "unlisted",
        ],
        &home_envs(&home),
        "",
        Some(project.path()),
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert!(
        result
            .stdout
            .contains("unlisted URL preview: https://u.example.test/<share-token>/")
    );
    let site_config = fs::read_to_string(site_config_path_for_dir(project.path())).unwrap();
    assert!(site_config.contains(r#""visibility": "unlisted""#));
}

#[test]
fn init_still_saves_site_config_when_metadata_is_unavailable() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::text(503, r#"{"error":"unavailable"}"#),
            _ => StubResponse::text(404, "not found"),
        }
    });

    let result = run_cli(
        &[
            "init",
            "--api-base",
            &server.api_base,
            "--slug",
            "my-site",
            "--publish-dir",
            "public",
        ],
        &home_envs(&home),
        "\n",
        Some(project.path()),
    );

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stderr, "");
    assert!(result.stdout.contains("saved .cfsurge.json"));
    assert!(!result.stdout.contains("public URL preview"));

    let site_config = fs::read_to_string(site_config_path_for_dir(project.path())).unwrap();
    assert!(site_config.contains(r#""slug": "my-site""#));
    assert!(site_config.contains(r#""publishDir": "public""#));
    assert!(site_config.contains(r#""visibility": "public""#));
}

#[test]
fn init_fails_with_clear_error_when_prompted_api_base_is_invalid() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    let result = run_cli(
        &["init", "--slug", "my-site", "--publish-dir", "."],
        &home_envs(&home),
        "not-a-url\n",
        Some(project.path()),
    );

    assert_eq!(result.code, 1);
    assert!(result.stdout.contains("API base URL: "));
    assert!(result.stderr.contains(
        "invalid API base URL: expected absolute http(s) URL like https://api.example.com"
    ));
}

#[test]
fn publish_uses_site_config_defaults() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/projects/site-default/deployments/prepare") => {
                StubResponse::json(&format!(
                    r#"{{"deploymentId":"dep-1","publicUrl":"https://site-default.example.test","uploadUrls":[{{"path":"index.html","url":"{}/v1/projects/site-default/deployments/dep-1/files/index.html"}}]}}"#,
                    api_base
                ))
            }
            ("PUT", "/v1/projects/site-default/deployments/dep-1/files/index.html") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            ("POST", "/v1/projects/site-default/deployments/dep-1/activate") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-publish\"\n}}\n",
            server.api_base
        ),
    );
    create_site_directory(project.path(), "public");
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"site-default\",\n  \"publishDir\": \"public\",\n  \"visibility\": \"public\"\n}\n",
    )
    .unwrap();

    let result = run_cli(&["publish"], &home_envs(&home), "", Some(project.path()));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout,
        "published site-default -> https://site-default.example.test\n"
    );

    let requests = server.recorded();
    assert_eq!(
        requests[0].url,
        "/v1/projects/site-default/deployments/prepare"
    );
    assert!(requests[0].body.contains(r#""visibility":"public""#));
    assert_eq!(
        requests[1].content_type.as_deref(),
        Some("text/html; charset=utf-8")
    );
}

#[test]
fn publish_explicit_args_override_site_config() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("POST", "/v1/projects/slug-override/deployments/prepare") => {
                StubResponse::json(&format!(
                    r#"{{"deploymentId":"dep-2","publicUrl":"https://slug-override.example.test","uploadUrls":[{{"path":"index.html","url":"{}/v1/projects/slug-override/deployments/dep-2/files/index.html"}}]}}"#,
                    api_base
                ))
            }
            ("PUT", "/v1/projects/slug-override/deployments/dep-2/files/index.html") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            ("POST", "/v1/projects/slug-override/deployments/dep-2/activate") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-publish\"\n}}\n",
            server.api_base
        ),
    );
    create_site_directory(project.path(), "config-dir");
    let explicit_dir = create_site_directory(project.path(), "explicit-dir");
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"config-slug\",\n  \"publishDir\": \"config-dir\",\n  \"visibility\": \"public\"\n}\n",
    )
    .unwrap();

    let result = run_cli(
        &[
            "publish",
            explicit_dir.to_str().unwrap(),
            "--slug",
            "slug-override",
        ],
        &home_envs(&home),
        "",
        Some(project.path()),
    );
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout,
        "published slug-override -> https://slug-override.example.test\n"
    );

    let requests = server.recorded();
    assert_eq!(
        requests[0].url,
        "/v1/projects/slug-override/deployments/prepare"
    );
    assert!(requests[0].body.contains(r#""visibility":"public""#));
}

#[test]
fn publish_unlisted_fails_when_meta_lacks_unlisted_host() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-publish\"\n}}\n",
            server.api_base
        ),
    );
    create_site_directory(project.path(), "public");
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"private-site\",\n  \"publishDir\": \"public\",\n  \"visibility\": \"unlisted\"\n}\n",
    )
    .unwrap();

    let result = run_cli(&["publish"], &home_envs(&home), "", Some(project.path()));
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .contains("unlisted publish is not supported by this server")
    );
}

#[test]
fn publish_unlisted_sends_visibility_and_prints_served_url() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_stub_server(move |api_base, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/meta") => StubResponse::json(&format!(
                r#"{{"apiBase":"{}","publicSuffix":"example.test","unlistedHost":"u.example.test","tokenCreationUrl":null}}"#,
                api_base
            )),
            ("POST", "/v1/projects/private-site/deployments/prepare") => {
                StubResponse::json(&format!(
                    r#"{{"deploymentId":"dep-unlisted","servedUrl":"https://u.example.test/abc123","uploadUrls":[{{"path":"index.html","url":"{}/v1/projects/private-site/deployments/dep-unlisted/files/index.html"}}]}}"#,
                    api_base
                ))
            }
            ("PUT", "/v1/projects/private-site/deployments/dep-unlisted/files/index.html") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            ("POST", "/v1/projects/private-site/deployments/dep-unlisted/activate") => {
                StubResponse::json(r#"{"ok":true}"#)
            }
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-publish\"\n}}\n",
            server.api_base
        ),
    );
    create_site_directory(project.path(), "public");
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"private-site\",\n  \"publishDir\": \"public\",\n  \"visibility\": \"unlisted\"\n}\n",
    )
    .unwrap();

    let result = run_cli(&["publish"], &home_envs(&home), "", Some(project.path()));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout,
        "published private-site -> https://u.example.test/abc123\n"
    );

    let requests = server.recorded();
    assert_eq!(requests[0].url, "/v1/meta");
    assert_eq!(
        requests[1].url,
        "/v1/projects/private-site/deployments/prepare"
    );
    assert!(requests[1].body.contains(r#""visibility":"unlisted""#));
}

#[test]
fn remove_uses_site_config_slug_by_default() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server =
        start_stub_server(
            |_, request| match (request.method.as_str(), request.url.as_str()) {
                ("DELETE", "/v1/projects/config-slug") => StubResponse::json(r#"{"ok":true}"#),
                _ => StubResponse::text(404, "not found"),
            },
        );
    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-remove\"\n}}\n",
            server.api_base
        ),
    );
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"config-slug\",\n  \"publishDir\": \".\"\n}\n",
    )
    .unwrap();

    let result = run_cli(&["remove"], &home_envs(&home), "", Some(project.path()));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "removed config-slug\n");

    let requests = server.recorded();
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer token-remove")
    );
}

#[test]
fn remove_explicit_slug_overrides_site_config_slug() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server =
        start_stub_server(
            |_, request| match (request.method.as_str(), request.url.as_str()) {
                ("DELETE", "/v1/projects/arg-slug") => StubResponse::json(r#"{"ok":true}"#),
                _ => StubResponse::text(404, "not found"),
            },
        );
    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-remove\"\n}}\n",
            server.api_base
        ),
    );
    fs::write(
        site_config_path_for_dir(project.path()),
        "{\n  \"slug\": \"config-slug\",\n  \"publishDir\": \".\"\n}\n",
    )
    .unwrap();

    let result = run_cli(
        &["remove", "arg-slug"],
        &home_envs(&home),
        "",
        Some(project.path()),
    );
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "removed arg-slug\n");

    let requests = server.recorded();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, "/v1/projects/arg-slug");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer token-remove")
    );
}

#[test]
fn remove_reserves_api_host_first_label_from_configured_api_base() {
    let home = TempDir::new().unwrap();
    write_config(
        home.path(),
        "{\n  \"apiBase\": \"https://manage.example.test\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-remove\"\n}\n",
    );

    let result = run_cli(&["remove", "manage"], &home_envs(&home), "", None);
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    assert!(
        result
            .stderr
            .contains("invalid slug: manage (reserved_slug)")
    );
}

#[test]
fn remove_reserves_unlisted_host_label() {
    let home = TempDir::new().unwrap();
    write_config(
        home.path(),
        "{\n  \"apiBase\": \"https://api.example.test\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-remove\"\n}\n",
    );

    let result = run_cli(&["remove", "u"], &home_envs(&home), "", None);
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    assert!(result.stderr.contains("invalid slug: u (reserved_slug)"));
}

#[test]
fn list_prints_visibility_and_fallbacks() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(|_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/projects") => StubResponse::json(
                r#"{"projects":[{"slug":"public-site","visibility":"public","servedUrl":"https://public-site.example.test","publicUrl":"https://public-site.example.test","hostname":"https://public-site.example.test","activeDeploymentId":"dep-1","updatedAt":"2026-03-23T00:00:00.000Z","updatedBy":"cf-token:a"},{"slug":"legacy","publicUrl":"https://legacy.example.test","hostname":"https://legacy.example.test","activeDeploymentId":null,"updatedAt":null,"updatedBy":null}]}"#,
            ),
            _ => StubResponse::text(404, "not found"),
        }
    });
    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-list\"\n}}\n",
            server.api_base
        ),
    );

    let result = run_cli(&["list"], &home_envs(&home), "", None);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout,
        "public-site\tpublic\thttps://public-site.example.test\tdep-1\t2026-03-23T00:00:00.000Z\tcf-token:a\nlegacy\tpublic\thttps://legacy.example.test\t-\t-\t-\n"
    );
}

#[test]
fn admin_users_commands_call_expected_endpoints() {
    let home = TempDir::new().unwrap();
    let server = start_stub_server(|_, request| {
        match (request.method.as_str(), request.url.as_str()) {
            ("GET", "/v1/admin/users") => StubResponse::json(
                r#"{"users":[{"username":"alice","role":"admin","status":"active","mustChangePassword":false},{"username":"bob","role":"user","status":"disabled","mustChangePassword":true}]}"#,
            ),
            ("POST", "/v1/admin/users") => {
                let body: Value = serde_json::from_str(&request.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({
                        "username": "charlie",
                        "role": "user",
                        "temporaryPassword": "temp-created",
                    })
                );
                StubResponse::json(r#"{"username":"charlie","temporaryPassword":"temp-created"}"#)
            }
            ("POST", "/v1/admin/users/charlie/reset-password") => {
                StubResponse::json(r#"{"temporaryPassword":"temp-reset"}"#)
            }
            ("POST", "/v1/admin/users/charlie/disable") => StubResponse::json(r#"{"ok":true}"#),
            ("POST", "/v1/admin/users/charlie/enable") => StubResponse::json(r#"{"ok":true}"#),
            _ => StubResponse::text(404, "not found"),
        }
    });

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"auth\": {{\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"file\",\n    \"accessToken\": \"admin-session-token\",\n    \"username\": \"alice\",\n    \"role\": \"admin\",\n    \"mustChangePassword\": false\n  }}\n}}\n",
            server.api_base
        ),
    );

    let list_result = run_cli(&["admin", "users", "list"], &home_envs(&home), "", None);
    assert_eq!(list_result.code, 0, "{}", list_result.stderr);
    assert_eq!(
        list_result.stdout,
        "alice\tadmin\tactive\tno\nbob\tuser\tdisabled\tyes\n"
    );

    let create_result = run_cli(
        &[
            "admin",
            "users",
            "create",
            "--username",
            "charlie",
            "--temporary-password",
            "temp-created",
        ],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(create_result.code, 0, "{}", create_result.stderr);
    assert_eq!(
        create_result.stdout,
        "created user charlie\ntemporary password: temp-created\n"
    );

    let reset_result = run_cli(
        &["admin", "users", "reset-password", "charlie"],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(reset_result.code, 0, "{}", reset_result.stderr);
    assert_eq!(
        reset_result.stdout,
        "reset password for charlie\ntemporary password: temp-reset\n"
    );

    let disable_result = run_cli(
        &["admin", "users", "disable", "charlie"],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(disable_result.code, 0, "{}", disable_result.stderr);
    assert_eq!(disable_result.stdout, "disabled user charlie\n");

    let enable_result = run_cli(
        &["admin", "users", "enable", "charlie"],
        &home_envs(&home),
        "",
        None,
    );
    assert_eq!(enable_result.code, 0, "{}", enable_result.stderr);
    assert_eq!(enable_result.stdout, "enabled user charlie\n");

    let requests = server.recorded();
    assert!(requests.len() >= 5);
    for request in requests {
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer admin-session-token")
        );
    }
}

#[test]
fn logout_revokes_service_session_before_clearing_local_state() {
    let home = TempDir::new().unwrap();
    let server =
        start_stub_server(
            |_, request| match (request.method.as_str(), request.url.as_str()) {
                ("POST", "/v1/auth/logout") => StubResponse::json(r#"{"ok":true}"#),
                _ => StubResponse::text(404, "not found"),
            },
        );

    write_config(
        home.path(),
        &format!(
            "{{\n  \"apiBase\": \"{}\",\n  \"auth\": {{\n    \"type\": \"service-session\",\n    \"tokenStorage\": \"file\",\n    \"accessToken\": \"session-token-logout\"\n  }}\n}}\n",
            server.api_base
        ),
    );

    let result = run_cli(&["logout"], &home_envs(&home), "", None);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "logged out\n");

    let requests = server.recorded();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].url, "/v1/auth/logout");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer session-token-logout")
    );
    assert!(!config_path_for_home(home.path()).exists());
}

#[test]
fn logout_removes_config_file() {
    let home = TempDir::new().unwrap();
    write_config(
        home.path(),
        "{\n  \"apiBase\": \"https://api.example.test\",\n  \"tokenStorage\": \"file\",\n  \"token\": \"token-logout\"\n}\n",
    );

    let result = run_cli(&["logout"], &home_envs(&home), "", None);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "logged out\n");
    assert!(!config_path_for_home(home.path()).exists());
}
