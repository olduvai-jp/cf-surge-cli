use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

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
