use crossterm::ExecutableCommand;
use crossterm::cursor::MoveUp;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};
use crossterm::terminal::{
    Clear, ClearType, disable_raw_mode, enable_raw_mode, is_raw_mode_enabled,
};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use urlencoding::encode;
use walkdir::WalkDir;

const SITE_CONFIG_FILE_NAME: &str = ".cfsurge.json";
const GENERIC_TOKEN_DASHBOARD_URL: &str = "https://dash.cloudflare.com/profile/api-tokens";
const API_BASE_PROMPT_GUIDANCE: &str = "Enter API base URL (for example: https://api.example.com)";
const USERNAME_PROMPT_GUIDANCE: &str = "Use your issued username from the admin.";
const DEV_CLI_VERSION: &str = "0.0.0-dev";
const KEYCHAIN_SERVICE: &str = "cfsurge";
const KEYCHAIN_ACCOUNT: &str = "api-token";
const DEFAULT_VISIBILITY: Visibility = Visibility::Public;
const MAX_SLUG_LENGTH: usize = 63;
const FALLBACK_API_RESERVED_LABEL: &str = "api";
const ALWAYS_RESERVED_LABEL: &str = "www";
const UNLISTED_FALLBACK_LABEL: &str = "u";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
    api_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<StoredAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_storage: Option<TokenStorage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum TokenStorage {
    Keychain,
    File,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum AuthType {
    CloudflareAdmin,
    ServiceSession,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredAuth {
    #[serde(rename = "type")]
    auth_type: AuthType,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_storage: Option<TokenStorage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    must_change_password: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SiteConfig {
    slug: String,
    publish_dir: String,
    visibility: Visibility,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Public,
    Unlisted,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unlisted => "unlisted",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiMetadata {
    api_base: Option<String>,
    public_suffix: Option<String>,
    unlisted_host: Option<String>,
    token_creation_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareResponse {
    deployment_id: String,
    served_url: Option<String>,
    public_url: Option<String>,
    upload_urls: Vec<UploadUrl>,
}

#[derive(Clone, Debug, Deserialize)]
struct UploadUrl {
    path: String,
    url: String,
}

struct SelectOption<T> {
    value: T,
    label: &'static str,
    description: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: String,
    sha256: String,
    size: u64,
    content_type: String,
}

#[derive(Debug, Deserialize)]
struct AuthVerifyResponse {
    actor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceLoginResponse {
    access_token: Option<String>,
    actor: Option<String>,
    username: Option<String>,
    role: Option<String>,
    must_change_password: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AdminUsersPayload {
    users: Option<Vec<AdminUserRecord>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserRecord {
    username: Option<String>,
    role: Option<String>,
    status: Option<String>,
    must_change_password: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserMutationPayload {
    username: Option<String>,
    temporary_password: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectsPayload {
    projects: Vec<ProjectRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRecord {
    slug: String,
    hostname: Option<String>,
    visibility: Option<String>,
    served_url: Option<String>,
    public_url: Option<String>,
    active_deployment_id: Option<String>,
    updated_at: Option<String>,
    updated_by: Option<String>,
}

pub fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    run_with_args(args)
}

fn run_with_args(args: Vec<String>) -> Result<(), String> {
    let command = args.first().map(String::as_str);
    match command {
        Some("login") => login(&args[1..]),
        Some("init") => init(&args[1..]),
        Some("publish") => publish(&args[1..]),
        Some("list") => list_projects(),
        Some("remove") => remove_project(&args[1..]),
        Some("passwd") => change_password(&args[1..]),
        Some("admin") => admin(&args[1..]),
        Some("logout") => logout(),
        Some("--version") => {
            print_version();
            Ok(())
        }
        Some("--help") | None => {
            print_help();
            Ok(())
        }
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn login(args: &[String]) -> Result<(), String> {
    let stored_config = read_stored_config_if_exists()?;
    let api_base = resolve_api_base(
        args,
        stored_config
            .as_ref()
            .map(|config| config.api_base.as_str()),
    )?;
    let token_from_flag = read_flag(args, "--token");
    let token_from_env = env::var("CFSURGE_TOKEN")
        .ok()
        .and_then(|value| read_string(&value));
    let mode = resolve_login_mode(read_flag(args, "--auth"), &token_from_flag, &token_from_env)?;
    let requested_token_storage = parse_token_storage_input(read_flag(args, "--token-storage"))?;
    let token_storage_preference = resolve_token_storage_preference(requested_token_storage)?;
    let client = Client::new();
    if mode == AuthType::CloudflareAdmin {
        let metadata = fetch_api_metadata(&api_base);
        print_token_creation_hint_if_needed(
            token_from_flag.as_deref(),
            token_from_env.as_deref(),
            metadata.as_ref(),
        )?;
        let token = match token_from_flag.or(token_from_env) {
            Some(token) => token,
            None => prompt("Cloudflare API token")?,
        };
        let response = client
            .post(format!("{api_base}/v1/auth/verify"))
            .headers(auth_headers(&token)?)
            .send()
            .map_err(format_http_error)?;
        if !response.status().is_success() {
            return Err(format!(
                "login failed: {}",
                response.text().unwrap_or_default()
            ));
        }
        let verify_result = response
            .json::<AuthVerifyResponse>()
            .map_err(format_http_error)?;
        let token_storage = persist_token(&token, token_storage_preference)?;
        write_config_file(&StoredConfig {
            api_base,
            auth: Some(StoredAuth {
                auth_type: AuthType::CloudflareAdmin,
                token_storage: Some(token_storage),
                access_token: if token_storage == TokenStorage::File {
                    Some(token.clone())
                } else {
                    None
                },
                actor: read_string_opt(verify_result.actor.as_ref()),
                username: None,
                role: None,
                must_change_password: None,
            }),
            token_storage: Some(token_storage),
            token: if token_storage == TokenStorage::File {
                Some(token)
            } else {
                None
            },
        })?;
        let actor = read_string_opt(verify_result.actor.as_ref())
            .unwrap_or_else(|| "cloudflare-admin".into());
        println!("logged in as {actor}");
        return Ok(());
    }

    if token_from_flag.is_some() || token_from_env.is_some() {
        return Err("token-based login requires --auth cloudflare-admin".into());
    }

    let username = resolve_username(args)?;
    let password = resolve_password(args)?;
    let response = client
        .post(format!("{api_base}/v1/auth/login"))
        .header(CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "login failed: {}",
            response.text().unwrap_or_default()
        ));
    }
    let login_result = response
        .json::<ServiceLoginResponse>()
        .map_err(format_http_error)?;
    let access_token = read_string_opt(login_result.access_token.as_ref())
        .ok_or_else(|| "login failed: missing accessToken".to_string())?;
    let token_storage = persist_token(&access_token, token_storage_preference)?;
    write_config_file(&StoredConfig {
        api_base,
        auth: Some(StoredAuth {
            auth_type: AuthType::ServiceSession,
            token_storage: Some(token_storage),
            access_token: if token_storage == TokenStorage::File {
                Some(access_token)
            } else {
                None
            },
            actor: read_string_opt(login_result.actor.as_ref()),
            username: read_string_opt(login_result.username.as_ref())
                .or_else(|| Some(username.clone())),
            role: read_string_opt(login_result.role.as_ref()),
            must_change_password: Some(login_result.must_change_password == Some(true)),
        }),
        token_storage: None,
        token: None,
    })?;
    let actor = read_string_opt(login_result.actor.as_ref())
        .or_else(|| read_string_opt(login_result.username.as_ref()))
        .unwrap_or(username);
    println!("logged in as {actor}");
    if login_result.must_change_password == Some(true) {
        println!("password change required. Run cfsurge passwd");
    }
    Ok(())
}

fn init(args: &[String]) -> Result<(), String> {
    let stored_config = read_stored_config_if_exists()?;
    let api_base = resolve_api_base(
        args,
        stored_config
            .as_ref()
            .map(|config| config.api_base.as_str()),
    )?;
    let metadata = fetch_api_metadata(&api_base);
    let existing = read_site_config_if_exists()?;
    let reserved_labels = build_reserved_labels(Some(api_base.as_str()), metadata.as_ref());

    let slug = resolve_slug(
        args,
        existing.as_ref().map(|item| item.slug.as_str()),
        &reserved_labels,
    )?;
    let publish_dir = resolve_publish_dir(
        args,
        existing
            .as_ref()
            .map(|item| item.publish_dir.as_str())
            .unwrap_or("."),
    )?;
    let visibility = resolve_visibility(
        args,
        existing
            .as_ref()
            .map(|item| item.visibility)
            .unwrap_or(DEFAULT_VISIBILITY),
    )?;

    write_site_config(&SiteConfig {
        slug: slug.clone(),
        publish_dir: publish_dir.clone(),
        visibility,
    })?;

    println!("saved {SITE_CONFIG_FILE_NAME}");
    if visibility == Visibility::Public {
        if let Some(public_suffix) = metadata
            .as_ref()
            .and_then(|item| read_string_opt(item.public_suffix.as_ref()))
        {
            println!("public URL preview: https://{slug}.{public_suffix}");
        }
    } else if let Some(unlisted_host) = metadata
        .as_ref()
        .and_then(|item| read_string_opt(item.unlisted_host.as_ref()))
    {
        println!("unlisted URL preview: https://{unlisted_host}/<share-token>/");
    }

    Ok(())
}

fn publish(args: &[String]) -> Result<(), String> {
    let site_config = read_site_config_if_exists()?;
    let slug =
        read_flag(args, "--slug").or_else(|| site_config.as_ref().map(|item| item.slug.clone()));
    let directory = read_positional_arg(args)
        .or_else(|| site_config.as_ref().map(|item| item.publish_dir.clone()));
    let visibility = site_config
        .as_ref()
        .map(|item| item.visibility)
        .unwrap_or(DEFAULT_VISIBILITY);

    let slug = slug.ok_or_else(|| {
        format!(
            "usage: cfsurge publish [dir] [--slug <slug>] (or configure {SITE_CONFIG_FILE_NAME} via cfsurge init)"
        )
    })?;
    let directory = directory.ok_or_else(|| {
        format!(
            "usage: cfsurge publish [dir] [--slug <slug>] (or configure {SITE_CONFIG_FILE_NAME} via cfsurge init)"
        )
    })?;

    let config = read_config(ReadConfigOptions::default())?;
    let metadata = if visibility == Visibility::Unlisted {
        fetch_api_metadata(&config.api_base)
    } else {
        None
    };
    let reserved_labels = build_reserved_labels(Some(config.api_base.as_str()), metadata.as_ref());
    assert_valid_slug(&slug, &reserved_labels)?;
    if visibility == Visibility::Unlisted {
        assert_unlisted_publish_supported(metadata.as_ref())?;
    }

    let absolute_dir = fs::canonicalize(&directory).map_err(|error| error.to_string())?;
    let files = collect_files(&absolute_dir)?;
    if files.is_empty() {
        return Err("publish target has no files".into());
    }

    let client = Client::new();
    let prepare_response = client
        .post(format!(
            "{}/v1/projects/{}/deployments/prepare",
            config.api_base,
            encode(&slug)
        ))
        .headers(auth_headers(&config.token)?)
        .json(&serde_json::json!({
            "files": files,
            "visibility": visibility.as_str(),
        }))
        .send()
        .map_err(format_http_error)?;

    if !prepare_response.status().is_success() {
        return Err(format!(
            "prepare failed: {}",
            prepare_response.text().unwrap_or_default()
        ));
    }

    let prepared = prepare_response
        .json::<PrepareResponse>()
        .map_err(format_http_error)?;

    for upload in &prepared.upload_urls {
        let file = files
            .iter()
            .find(|entry| entry.path == upload.path)
            .ok_or_else(|| format!("missing file descriptor for {}", upload.path))?;
        let body = fs::read(absolute_dir.join(&upload.path)).map_err(|error| error.to_string())?;
        let mut request = client.put(&upload.url);
        request = request.header(CONTENT_TYPE, &file.content_type);
        if should_attach_api_auth(&upload.url, &config.api_base) {
            request = request.header(AUTHORIZATION, format!("Bearer {}", config.token));
        }
        let upload_response = request.body(body).send().map_err(format_http_error)?;
        if !upload_response.status().is_success() {
            return Err(format!(
                "upload failed for {}: {}",
                upload.path,
                upload_response.text().unwrap_or_default()
            ));
        }
    }

    let activate_response = client
        .post(format!(
            "{}/v1/projects/{}/deployments/{}/activate",
            config.api_base,
            encode(&slug),
            encode(&prepared.deployment_id)
        ))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;

    if !activate_response.status().is_success() {
        return Err(format!(
            "activate failed: {}",
            activate_response.text().unwrap_or_default()
        ));
    }

    let served_url = read_string_opt(prepared.served_url.as_ref())
        .or_else(|| read_string_opt(prepared.public_url.as_ref()))
        .ok_or_else(|| "prepare failed: missing servedUrl/publicUrl in response".to_string())?;
    println!("published {slug} -> {served_url}");
    Ok(())
}

fn list_projects() -> Result<(), String> {
    let config = read_config(ReadConfigOptions::default())?;
    let client = Client::new();
    let response = client
        .get(format!("{}/v1/projects", config.api_base))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;

    if !response.status().is_success() {
        return Err(format!(
            "list failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    let payload = response
        .json::<ProjectsPayload>()
        .map_err(format_http_error)?;
    if payload.projects.is_empty() {
        println!("no projects");
        return Ok(());
    }

    for project in payload.projects {
        let visibility =
            normalize_visibility(project.visibility.as_deref()).unwrap_or(DEFAULT_VISIBILITY);
        let served_url = read_string_opt(project.served_url.as_ref())
            .or_else(|| read_string_opt(project.public_url.as_ref()))
            .or_else(|| read_string_opt(project.hostname.as_ref()))
            .unwrap_or_else(|| "-".into());
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            project.slug,
            visibility.as_str(),
            served_url,
            project.active_deployment_id.unwrap_or_else(|| "-".into()),
            project.updated_at.unwrap_or_else(|| "-".into()),
            project.updated_by.unwrap_or_else(|| "-".into())
        );
    }

    Ok(())
}

fn remove_project(args: &[String]) -> Result<(), String> {
    let site_config = read_site_config_if_exists()?;
    let slug =
        read_positional_arg(args).or_else(|| site_config.as_ref().map(|item| item.slug.clone()));
    let slug = slug.ok_or_else(|| {
        format!(
            "usage: cfsurge remove [slug] (or configure {SITE_CONFIG_FILE_NAME} via cfsurge init)"
        )
    })?;
    let config = read_config(ReadConfigOptions::default())?;
    let reserved_labels = build_reserved_labels(Some(config.api_base.as_str()), None);
    assert_valid_slug(&slug, &reserved_labels)?;

    let client = Client::new();
    let response = client
        .delete(format!("{}/v1/projects/{}", config.api_base, encode(&slug)))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;

    if !response.status().is_success() {
        return Err(format!(
            "remove failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    println!("removed {slug}");
    Ok(())
}

fn change_password(args: &[String]) -> Result<(), String> {
    let config = read_config(ReadConfigOptions {
        allow_must_change_password: true,
    })?;
    if config.auth_type != AuthType::ServiceSession {
        return Err("passwd is only available for service-session login.".into());
    }

    let current_password = match read_flag(args, "--current-password") {
        Some(value) => value,
        None => prompt("Current password")?,
    };
    let new_password = match read_flag(args, "--new-password") {
        Some(value) => value,
        None => prompt("New password")?,
    };
    if new_password.trim().is_empty() {
        return Err("new password cannot be empty".into());
    }

    let client = Client::new();
    let response = client
        .post(format!("{}/v1/auth/change-password", config.api_base))
        .headers(auth_headers(&config.token)?)
        .header(CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({
            "currentPassword": current_password,
            "newPassword": new_password,
        }))
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "passwd failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    update_stored_must_change_password(false)?;
    println!("password updated");
    Ok(())
}

fn admin(args: &[String]) -> Result<(), String> {
    let target = args.first().map(String::as_str);
    let action = args.get(1).map(String::as_str);
    if target != Some("users") {
        return Err(
            "usage: cfsurge admin users <list|create|reset-password|disable|enable> [...]".into(),
        );
    }
    let rest = if args.len() > 2 { &args[2..] } else { &[] };
    match action {
        Some("list") => admin_users_list(),
        Some("create") => admin_users_create(rest),
        Some("reset-password") => admin_users_reset_password(rest),
        Some("disable") => admin_users_toggle_status(rest, "disable"),
        Some("enable") => admin_users_toggle_status(rest, "enable"),
        _ => Err(
            "usage: cfsurge admin users <list|create|reset-password|disable|enable> [...]".into(),
        ),
    }
}

fn admin_users_list() -> Result<(), String> {
    let config = read_config(ReadConfigOptions::default())?;
    let client = Client::new();
    let response = client
        .get(format!("{}/v1/admin/users", config.api_base))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "admin users list failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    let payload = response
        .json::<AdminUsersPayload>()
        .map_err(format_http_error)?;
    let users = payload.users.unwrap_or_default();
    if users.is_empty() {
        println!("no users");
        return Ok(());
    }

    for user in users {
        println!(
            "{}\t{}\t{}\t{}",
            read_string_opt(user.username.as_ref()).unwrap_or_else(|| "-".into()),
            read_string_opt(user.role.as_ref()).unwrap_or_else(|| "-".into()),
            read_string_opt(user.status.as_ref()).unwrap_or_else(|| "-".into()),
            if user.must_change_password == Some(true) {
                "yes"
            } else {
                "no"
            }
        );
    }
    Ok(())
}

fn admin_users_create(args: &[String]) -> Result<(), String> {
    let config = read_config(ReadConfigOptions::default())?;
    let username = resolve_admin_username(args, "--username", "Username")?;
    let role = parse_user_role(
        read_flag(args, "--role")
            .unwrap_or_else(|| "user".to_string())
            .as_str(),
    )?;
    let temporary_password =
        read_flag(args, "--temporary-password").and_then(|item| read_string(&item));
    let mut body = serde_json::json!({
        "username": username.clone(),
        "role": role,
    });
    if let Some(value) = temporary_password {
        body["temporaryPassword"] = serde_json::Value::String(value);
    }

    let client = Client::new();
    let response = client
        .post(format!("{}/v1/admin/users", config.api_base))
        .headers(auth_headers(&config.token)?)
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "admin users create failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    let payload = response
        .json::<AdminUserMutationPayload>()
        .unwrap_or_default();
    let created_user = read_string_opt(payload.username.as_ref()).unwrap_or(username);
    println!("created user {created_user}");
    if let Some(issued_password) = read_string_opt(payload.temporary_password.as_ref()) {
        println!("temporary password: {issued_password}");
    }
    Ok(())
}

fn admin_users_reset_password(args: &[String]) -> Result<(), String> {
    let config = read_config(ReadConfigOptions::default())?;
    let username = resolve_admin_positional_username(
        args,
        "usage: cfsurge admin users reset-password <username>",
    )?;

    let client = Client::new();
    let response = client
        .post(format!(
            "{}/v1/admin/users/{}/reset-password",
            config.api_base,
            encode(&username)
        ))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "admin users reset-password failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    let payload = response
        .json::<AdminUserMutationPayload>()
        .unwrap_or_default();
    println!("reset password for {username}");
    if let Some(temporary_password) = read_string_opt(payload.temporary_password.as_ref()) {
        println!("temporary password: {temporary_password}");
    }
    Ok(())
}

fn admin_users_toggle_status(args: &[String], action: &str) -> Result<(), String> {
    let config = read_config(ReadConfigOptions::default())?;
    let username = resolve_admin_positional_username(
        args,
        &format!("usage: cfsurge admin users {action} <username>"),
    )?;

    let client = Client::new();
    let response = client
        .post(format!(
            "{}/v1/admin/users/{}/{}",
            config.api_base,
            encode(&username),
            action
        ))
        .headers(auth_headers(&config.token)?)
        .send()
        .map_err(format_http_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "admin users {action} failed: {}",
            response.text().unwrap_or_default()
        ));
    }

    println!("{action}d user {username}");
    Ok(())
}

fn logout() -> Result<(), String> {
    if let Some(stored) = read_stored_config_if_exists()? {
        let stored_api_base = match env::var("CFSURGE_API_BASE")
            .ok()
            .and_then(|value| read_string(&value))
        {
            Some(value) => normalize_api_base(&value)?,
            None => stored.api_base.clone(),
        };
        if resolve_stored_auth_type(&stored) == AuthType::ServiceSession
            && let Some(token) = read_stored_token(&stored)?
            && let Ok(headers) = auth_headers(&token)
        {
            let client = Client::new();
            let _ = client
                .post(format!("{stored_api_base}/v1/auth/logout"))
                .headers(headers)
                .send();
        }
    }

    let config_path = config_path()?;
    let _ = fs::remove_file(config_path);

    if can_use_mac_keychain()
        && let Err(error) = delete_token_from_mac_keychain()
    {
        eprintln!(
            "warning: failed to clear token from macOS Keychain ({})",
            error
        );
    }

    println!("logged out");
    Ok(())
}

fn read_config(options: ReadConfigOptions) -> Result<CliConfig, String> {
    let stored = read_stored_config()?;
    let api_base = env::var("CFSURGE_API_BASE")
        .ok()
        .and_then(|value| read_string(&value))
        .unwrap_or(stored.api_base.clone());
    let token = read_stored_token(&stored)?
        .ok_or_else(|| "missing API token. Run cfsurge login.".to_string())?;
    let auth_type = resolve_stored_auth_type(&stored);
    let must_change_password = stored.auth.as_ref().is_some_and(|auth| {
        auth.auth_type == AuthType::ServiceSession && auth.must_change_password == Some(true)
    });
    if must_change_password && !options.allow_must_change_password {
        return Err("password change required. Run cfsurge passwd.".into());
    }
    Ok(CliConfig {
        api_base,
        token,
        auth_type,
    })
}

#[derive(Clone, Debug)]
struct CliConfig {
    api_base: String,
    token: String,
    auth_type: AuthType,
}

#[derive(Clone, Copy, Debug, Default)]
struct ReadConfigOptions {
    allow_must_change_password: bool,
}

fn read_stored_config() -> Result<StoredConfig, String> {
    read_stored_config_if_exists()?.ok_or_else(|| "not logged in. Run cfsurge login.".into())
}

fn read_stored_config_if_exists() -> Result<Option<StoredConfig>, String> {
    let path = config_path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    parse_stored_config(&raw, &path).map(Some)
}

fn parse_stored_config(raw: &str, path: &Path) -> Result<StoredConfig, String> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| format!("invalid config file: {}", path.display()))?;
    let api_base = value
        .get("apiBase")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| format!("invalid config file: missing apiBase in {}", path.display()))?;
    let auth = if value.get("auth").is_some() {
        parse_stored_auth(value.get("auth"), path)?
    } else {
        None
    };
    let token_storage = match value.get("tokenStorage").and_then(|item| item.as_str()) {
        Some("keychain") => Some(TokenStorage::Keychain),
        Some("file") => Some(TokenStorage::File),
        Some(_) => {
            return Err(format!(
                "invalid config file: unsupported tokenStorage in {}",
                path.display()
            ));
        }
        None => None,
    };
    let token = value
        .get("token")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned);
    let token_storage = token_storage.or_else(|| token.as_ref().map(|_| TokenStorage::File));
    Ok(StoredConfig {
        api_base: api_base.to_string(),
        auth,
        token_storage,
        token,
    })
}

fn parse_stored_auth(
    value: Option<&serde_json::Value>,
    path: &Path,
) -> Result<Option<StoredAuth>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(auth) = value.as_object() else {
        return Err(format!(
            "invalid config file: unsupported auth in {}",
            path.display()
        ));
    };

    let Some(auth_type_raw) = auth.get("type").and_then(|item| item.as_str()) else {
        return Err(format!(
            "invalid config file: unsupported auth.type in {}",
            path.display()
        ));
    };
    let auth_type = match auth_type_raw.trim() {
        "cloudflare-admin" => AuthType::CloudflareAdmin,
        "service-session" => AuthType::ServiceSession,
        _ => {
            return Err(format!(
                "invalid config file: unsupported auth.type in {}",
                path.display()
            ));
        }
    };
    let token_storage = match auth.get("tokenStorage").and_then(|item| item.as_str()) {
        Some("keychain") => Some(TokenStorage::Keychain),
        Some("file") => Some(TokenStorage::File),
        Some(_) => {
            return Err(format!(
                "invalid config file: unsupported auth.tokenStorage in {}",
                path.display()
            ));
        }
        None => None,
    };
    let access_token = auth
        .get("accessToken")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned);
    let token_storage = token_storage.or_else(|| access_token.as_ref().map(|_| TokenStorage::File));
    let must_change_password = if auth.contains_key("mustChangePassword") {
        Some(
            auth.get("mustChangePassword")
                .and_then(|item| item.as_bool())
                .unwrap_or(false),
        )
    } else {
        None
    };

    Ok(Some(StoredAuth {
        auth_type,
        token_storage,
        access_token,
        actor: auth
            .get("actor")
            .and_then(|item| item.as_str())
            .and_then(read_string),
        username: auth
            .get("username")
            .and_then(|item| item.as_str())
            .and_then(read_string),
        role: auth
            .get("role")
            .and_then(|item| item.as_str())
            .and_then(read_string),
        must_change_password,
    }))
}

fn read_site_config_if_exists() -> Result<Option<SiteConfig>, String> {
    let path = site_config_path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| format!("invalid site config file: {}", path.display()))?;
    let slug = value
        .get("slug")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| format!("invalid site config file: {}", path.display()))?;
    let publish_dir = value
        .get("publishDir")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| format!("invalid site config file: {}", path.display()))?;
    let raw_visibility = value.get("visibility").and_then(|item| item.as_str());
    let visibility = match raw_visibility {
        Some(item) => normalize_visibility(Some(item))
            .ok_or_else(|| format!("invalid site config file: {}", path.display()))?,
        None => DEFAULT_VISIBILITY,
    };
    Ok(Some(SiteConfig {
        slug: slug.to_string(),
        publish_dir: publish_dir.to_string(),
        visibility,
    }))
}

fn write_site_config(config: &SiteConfig) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    fs::write(site_config_path()?, append_newline(bytes)).map_err(|error| error.to_string())
}

fn read_stored_token(config: &StoredConfig) -> Result<Option<String>, String> {
    if let Some(token) = env::var("CFSURGE_TOKEN")
        .ok()
        .and_then(|value| read_string(&value))
    {
        return Ok(Some(token));
    }

    if let Some(auth) = &config.auth {
        return read_token_from_storage(auth.token_storage, auth.access_token.clone())
            .and_then(|value| {
                value.ok_or_else(|| "missing API token. Run cfsurge login.".to_string())
            })
            .map(Some);
    }

    read_token_from_storage(config.token_storage, config.token.clone())
}

fn read_token_from_storage(
    token_storage: Option<TokenStorage>,
    inline_token: Option<String>,
) -> Result<Option<String>, String> {
    match token_storage {
        Some(TokenStorage::Keychain) => {
            if !can_use_mac_keychain() {
                if let Some(token) = inline_token {
                    return Ok(Some(token));
                }
                return Err(
                    "stored token requires macOS Keychain, but it is unavailable. Run cfsurge login again.".into(),
                );
            }
            match read_token_from_mac_keychain() {
                Ok(token) => Ok(Some(token)),
                Err(error) => {
                    if let Some(token) = inline_token {
                        Ok(Some(token))
                    } else {
                        Err(format!(
                            "failed to read token from macOS Keychain ({}). Run cfsurge login.",
                            error
                        ))
                    }
                }
            }
        }
        Some(TokenStorage::File) => inline_token
            .map(Some)
            .ok_or_else(|| "config file token is missing. Run cfsurge login.".into()),
        None => Ok(None),
    }
}

fn persist_token(token: &str, token_storage: TokenStorage) -> Result<TokenStorage, String> {
    match token_storage {
        TokenStorage::File => Ok(TokenStorage::File),
        TokenStorage::Keychain => {
            if !can_use_mac_keychain() {
                return Err(
                    "macOS Keychain is unavailable. Run cfsurge login with --token-storage file."
                        .into(),
                );
            }
            write_token_to_mac_keychain(token)?;
            Ok(TokenStorage::Keychain)
        }
    }
}

fn write_config_file(config: &StoredConfig) -> Result<(), String> {
    let config_dir = config_dir()?;
    fs::create_dir_all(&config_dir).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700));
    }

    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    fs::write(config_path()?, append_newline(bytes)).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(config_path()?.as_path(), fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn update_stored_must_change_password(value: bool) -> Result<(), String> {
    let Some(mut stored) = read_stored_config_if_exists()? else {
        return Ok(());
    };
    let Some(mut auth) = stored.auth else {
        return Ok(());
    };
    if auth.auth_type != AuthType::ServiceSession {
        return Ok(());
    }
    auth.must_change_password = Some(value);
    stored.auth = Some(auth);
    write_config_file(&stored)
}

fn can_use_mac_keychain() -> bool {
    cfg!(target_os = "macos") && Command::new("security").arg("help").output().is_ok()
}

fn write_token_to_mac_keychain(token: &str) -> Result<(), String> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
            token,
            "-U",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn read_token_from_mac_keychain() -> Result<String, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err("empty token in keychain".into());
    }
    Ok(token)
}

fn delete_token_from_mac_keychain() -> Result<(), String> {
    let output = Command::new("security")
        .args([
            "delete-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            KEYCHAIN_SERVICE,
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr.contains("could not be found")
        || stderr.contains("The specified item could not be found in the keychain")
    {
        return Ok(());
    }
    Err(stderr.trim().to_string())
}

fn collect_files(root_dir: &Path) -> Result<Vec<FileEntry>, String> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root_dir) {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.file_type().is_file() {
            continue;
        }
        let absolute_path = entry.into_path();
        let relative_path = absolute_path
            .strip_prefix(root_dir)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let buffer = fs::read(&absolute_path).map_err(|error| error.to_string())?;
        let size = fs::metadata(&absolute_path)
            .map_err(|error| error.to_string())?
            .len();
        let sha256 = hex_digest(&buffer);
        files.push(FileEntry {
            path: relative_path.clone(),
            sha256,
            size,
            content_type: guess_content_type(&relative_path).into(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn guess_content_type(file_path: &str) -> &'static str {
    let lowercase = file_path.to_ascii_lowercase();
    if lowercase.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if lowercase.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if lowercase.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if lowercase.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lowercase.ends_with(".svg") {
        "image/svg+xml"
    } else if lowercase.ends_with(".png") {
        "image/png"
    } else if lowercase.ends_with(".jpg") || lowercase.ends_with(".jpeg") {
        "image/jpeg"
    } else if lowercase.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn auth_headers(token: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| error.to_string())?;
    headers.insert(AUTHORIZATION, value);
    Ok(headers)
}

fn should_attach_api_auth(upload_url: &str, api_base: &str) -> bool {
    let upload_origin = parse_origin(upload_url);
    let api_origin = parse_origin(api_base);
    upload_origin.is_some() && upload_origin == api_origin
}

fn fetch_api_metadata(api_base: &str) -> Option<ApiMetadata> {
    let client = Client::new();
    let response = client.get(format!("{api_base}/v1/meta")).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<ApiMetadata>().ok()
}

fn is_interactive_prompt_session() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn resolve_api_base(args: &[String], stored_api_base: Option<&str>) -> Result<String, String> {
    if let Some(value) = read_flag(args, "--api-base") {
        return normalize_api_base(&value);
    }
    if let Some(value) = env::var("CFSURGE_API_BASE")
        .ok()
        .and_then(|value| read_string(&value))
    {
        return normalize_api_base(&value);
    }
    if let Some(value) = stored_api_base {
        return normalize_api_base(value);
    }
    if is_interactive_prompt_session() {
        println!("{API_BASE_PROMPT_GUIDANCE}");
    }
    prompt_until_valid("API base URL", normalize_api_base)
}

fn resolve_username(args: &[String]) -> Result<String, String> {
    if let Some(value) = read_flag(args, "--username").and_then(|value| read_string(&value)) {
        return Ok(value);
    }
    if let Some(value) = env::var("CFSURGE_USERNAME")
        .ok()
        .and_then(|value| read_string(&value))
    {
        return Ok(value);
    }
    if is_interactive_prompt_session() {
        println!("{USERNAME_PROMPT_GUIDANCE}");
    }
    prompt_until_valid("Username", |value| {
        let value = value.trim();
        if value.is_empty() {
            Err("username is required".into())
        } else {
            Ok(value.to_string())
        }
    })
}

fn resolve_password(args: &[String]) -> Result<String, String> {
    if let Some(value) = read_flag(args, "--password") {
        if value.trim().is_empty() {
            return Err("password is required".into());
        }
        return Ok(value);
    }
    if let Ok(value) = env::var("CFSURGE_PASSWORD") {
        if value.trim().is_empty() {
            return Err("password is required".into());
        }
        return Ok(value);
    }
    prompt_until_valid("Password", |value| {
        if value.trim().is_empty() {
            Err("password is required".into())
        } else {
            Ok(value.to_string())
        }
    })
}

fn resolve_slug(
    args: &[String],
    default_slug: Option<&str>,
    reserved_labels: &[String],
) -> Result<String, String> {
    if let Some(value) = read_flag(args, "--slug") {
        return parse_slug_input(&value, reserved_labels);
    }
    if is_interactive_prompt_session() {
        println!("Project slug is used as hostname label (a-z, 0-9, hyphen).");
    }
    prompt_with_default_until_valid("Project slug", default_slug, |value| {
        parse_slug_input(value, reserved_labels)
    })
}

fn resolve_publish_dir(args: &[String], default_publish_dir: &str) -> Result<String, String> {
    if let Some(value) = read_flag(args, "--publish-dir") {
        return parse_publish_dir_input(&value);
    }
    if is_interactive_prompt_session() {
        println!("Publish directory is relative to the current working directory.");
    }
    prompt_with_default_until_valid(
        "Publish directory",
        Some(default_publish_dir),
        parse_publish_dir_input,
    )
}

fn resolve_visibility(
    args: &[String],
    default_visibility: Visibility,
) -> Result<Visibility, String> {
    if let Some(value) = read_flag(args, "--visibility") {
        return parse_visibility_input(&value);
    }
    if !is_interactive_prompt_session() {
        return Ok(default_visibility);
    }

    prompt_select(
        "Visibility",
        &[
            SelectOption {
                value: Visibility::Public,
                label: "public",
                description: "Published at https://<slug>.<publicSuffix>",
            },
            SelectOption {
                value: Visibility::Unlisted,
                label: "unlisted",
                description: "Published at unlisted host with a share token",
            },
        ],
        default_visibility,
    )
}

fn resolve_token_storage_preference(
    requested_token_storage: Option<TokenStorage>,
) -> Result<TokenStorage, String> {
    if let Some(value) = requested_token_storage {
        return Ok(value);
    }
    if !is_interactive_prompt_session() {
        return Ok(TokenStorage::File);
    }

    prompt_select(
        "Token storage",
        &[
            SelectOption {
                value: TokenStorage::File,
                label: "file",
                description: "Store token in ~/.config/cfsurge/config.json",
            },
            SelectOption {
                value: TokenStorage::Keychain,
                label: "keychain",
                description: "Store token in macOS Keychain (fails on non-macOS)",
            },
        ],
        TokenStorage::File,
    )
}

fn print_token_creation_hint_if_needed(
    token_from_flag: Option<&str>,
    token_from_env: Option<&str>,
    metadata: Option<&ApiMetadata>,
) -> Result<(), String> {
    if token_from_flag.is_some() || token_from_env.is_some() {
        return Ok(());
    }
    let url = metadata
        .and_then(|item| read_string_opt(item.token_creation_url.as_ref()))
        .unwrap_or_else(|| GENERIC_TOKEN_DASHBOARD_URL.to_string());
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Create a Cloudflare API token if you do not have one:"
    )
    .map_err(|error| error.to_string())?;
    writeln!(stdout, "{url}").map_err(|error| error.to_string())
}

fn normalize_visibility(value: Option<&str>) -> Option<Visibility> {
    let normalized = value?.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "public" => Some(Visibility::Public),
        "unlisted" => Some(Visibility::Unlisted),
        _ => None,
    }
}

fn parse_visibility_input(value: &str) -> Result<Visibility, String> {
    normalize_visibility(Some(value))
        .ok_or_else(|| "invalid visibility: expected public or unlisted".into())
}

fn parse_slug_input(value: &str, reserved_labels: &[String]) -> Result<String, String> {
    let slug = value.trim();
    assert_valid_slug(slug, reserved_labels)?;
    Ok(slug.to_string())
}

fn parse_publish_dir_input(value: &str) -> Result<String, String> {
    let publish_dir = value.trim();
    if publish_dir.is_empty() {
        return Err("publish directory cannot be empty".into());
    }
    Ok(publish_dir.to_string())
}

fn parse_token_storage_input(value: Option<String>) -> Result<Option<TokenStorage>, String> {
    let value = match value {
        Some(value) => value,
        None => return Ok(None),
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "file" => Ok(Some(TokenStorage::File)),
        "keychain" => Ok(Some(TokenStorage::Keychain)),
        _ => Err("invalid token storage: expected file or keychain".into()),
    }
}

fn resolve_login_mode(
    requested_mode: Option<String>,
    token_from_flag: &Option<String>,
    token_from_env: &Option<String>,
) -> Result<AuthType, String> {
    if let Some(value) = parse_login_mode_input(requested_mode)? {
        return Ok(value);
    }
    if token_from_flag.is_some() || token_from_env.is_some() {
        return Ok(AuthType::CloudflareAdmin);
    }
    Ok(AuthType::ServiceSession)
}

fn parse_login_mode_input(value: Option<String>) -> Result<Option<AuthType>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "service-session" | "service" | "password" => Ok(Some(AuthType::ServiceSession)),
        "cloudflare-admin" | "cloudflare" => Ok(Some(AuthType::CloudflareAdmin)),
        _ => Err("invalid login mode: expected service-session or cloudflare-admin".into()),
    }
}

fn resolve_stored_auth_type(config: &StoredConfig) -> AuthType {
    config
        .auth
        .as_ref()
        .map(|item| item.auth_type)
        .unwrap_or(AuthType::CloudflareAdmin)
}

fn resolve_admin_username(args: &[String], flag: &str, label: &str) -> Result<String, String> {
    if let Some(value) = read_flag(args, flag).and_then(|item| read_string(&item)) {
        return Ok(value);
    }
    prompt_until_valid(label, |value| {
        let username = value.trim();
        if username.is_empty() {
            Err("username is required".into())
        } else {
            Ok(username.to_string())
        }
    })
}

fn resolve_admin_positional_username(
    args: &[String],
    usage_message: &str,
) -> Result<String, String> {
    let Some(username) = read_positional_arg(args) else {
        return Err(usage_message.to_string());
    };
    let username = username.trim();
    if username.is_empty() {
        return Err(usage_message.to_string());
    }
    Ok(username.to_string())
}

fn parse_user_role(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "user" || normalized == "admin" {
        Ok(normalized)
    } else {
        Err("invalid role: expected user or admin".into())
    }
}

fn normalize_api_base(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(
            "invalid API base URL: expected absolute http(s) URL like https://api.example.com"
                .into(),
        );
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| {
        "invalid API base URL: expected absolute http(s) URL like https://api.example.com"
            .to_string()
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(
            "invalid API base URL: expected absolute http(s) URL like https://api.example.com"
                .into(),
        );
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("invalid API base URL: do not include path, query, or fragment".into());
    }
    Ok(url.origin().ascii_serialization())
}

fn assert_valid_slug(slug: &str, reserved_labels: &[String]) -> Result<(), String> {
    match validate_slug(slug, reserved_labels) {
        None => Ok(()),
        Some("reserved_slug") => Err(format!("invalid slug: {slug} (reserved_slug)")),
        Some(_) => Err(format!("invalid slug: {slug} (invalid_slug)")),
    }
}

fn validate_slug(slug: &str, reserved_labels: &[String]) -> Option<&'static str> {
    if slug.is_empty() || slug.len() > MAX_SLUG_LENGTH {
        return Some("invalid_slug");
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Some("invalid_slug");
    }
    if !slug
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Some("invalid_slug");
    }
    if reserved_labels.iter().any(|value| value == slug) {
        return Some("reserved_slug");
    }
    None
}

fn build_reserved_labels(api_base: Option<&str>, metadata: Option<&ApiMetadata>) -> Vec<String> {
    let mut reserved = vec![
        ALWAYS_RESERVED_LABEL.to_string(),
        UNLISTED_FALLBACK_LABEL.to_string(),
    ];
    let api_label = resolve_api_host_first_label(api_base, metadata)
        .unwrap_or_else(|| FALLBACK_API_RESERVED_LABEL.to_string());
    if !reserved.contains(&api_label) {
        reserved.push(api_label);
    }
    if let Some(label) = resolve_unlisted_host_first_label(metadata)
        && !reserved.contains(&label)
    {
        reserved.push(label);
    }
    reserved
}

fn resolve_api_host_first_label(
    api_base: Option<&str>,
    metadata: Option<&ApiMetadata>,
) -> Option<String> {
    metadata
        .and_then(|item| read_string_opt(item.api_base.as_ref()))
        .and_then(|item| extract_first_hostname_label(&item))
        .or_else(|| api_base.and_then(extract_first_hostname_label))
}

fn extract_first_hostname_label(value: &str) -> Option<String> {
    let hostname = reqwest::Url::parse(value)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    hostname
        .split('.')
        .next()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn resolve_unlisted_host_first_label(metadata: Option<&ApiMetadata>) -> Option<String> {
    let value = metadata.and_then(|item| read_string_opt(item.unlisted_host.as_ref()))?;
    let url_value = if value.contains("://") {
        value
    } else {
        format!("https://{value}")
    };
    let hostname = reqwest::Url::parse(&url_value)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    hostname
        .split('.')
        .next()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn assert_unlisted_publish_supported(metadata: Option<&ApiMetadata>) -> Result<(), String> {
    if metadata
        .and_then(|item| read_string_opt(item.unlisted_host.as_ref()))
        .is_some()
    {
        Ok(())
    } else {
        Err("unlisted publish is not supported by this server: /v1/meta did not include unlistedHost".into())
    }
}

fn prompt(label: &str) -> Result<String, String> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "{label}: ").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())?;
    read_stdin_line()
}

fn prompt_until_valid<T, F>(label: &str, parse: F) -> Result<T, String>
where
    F: Fn(&str) -> Result<T, String>,
{
    loop {
        let value = prompt(label)?;
        match parse(&value) {
            Ok(parsed) => return Ok(parsed),
            Err(error) if is_interactive_prompt_session() => eprintln!("{error}"),
            Err(error) => return Err(error),
        }
    }
}

fn prompt_with_default(label: &str, default_value: Option<&str>) -> Result<String, String> {
    let prompt_label = match default_value {
        Some(value) if !value.is_empty() => format!("{label} [{value}]"),
        _ => label.to_string(),
    };
    let value = prompt(&prompt_label)?;
    if !value.is_empty() {
        return Ok(value);
    }
    if let Some(value) = default_value
        && !value.is_empty()
    {
        return Ok(value.to_string());
    }
    Err(format!("{label} is required"))
}

fn prompt_with_default_until_valid<T, F>(
    label: &str,
    default_value: Option<&str>,
    parse: F,
) -> Result<T, String>
where
    F: Fn(&str) -> Result<T, String>,
{
    loop {
        let value = prompt_with_default(label, default_value)?;
        match parse(&value) {
            Ok(parsed) => return Ok(parsed),
            Err(error) if is_interactive_prompt_session() => eprintln!("{error}"),
            Err(error) => return Err(error),
        }
    }
}

fn prompt_select<T>(label: &str, options: &[SelectOption<T>], default_value: T) -> Result<T, String>
where
    T: Copy + PartialEq,
{
    if !is_interactive_prompt_session() {
        return Ok(default_value);
    }
    if options.is_empty() {
        return Err(format!("select options are required for {label}"));
    }

    let mut stdout = io::stdout().lock();
    let mut selected_index = options
        .iter()
        .position(|option| option.value == default_value)
        .unwrap_or(0);
    let mut rendered_line_count = 0u16;
    let was_raw_mode_enabled = is_raw_mode_enabled().map_err(|error| error.to_string())?;

    if !was_raw_mode_enabled {
        enable_raw_mode().map_err(|error| error.to_string())?;
    }
    let selection_result = (|| {
        render_select(
            &mut stdout,
            label,
            options,
            selected_index,
            &mut rendered_line_count,
        )?;
        loop {
            let event = read().map_err(|error| error.to_string())?;
            let Event::Key(key_event) = event else {
                continue;
            };
            if matches!(key_event.kind, KeyEventKind::Release) {
                continue;
            }
            match key_event.code {
                KeyCode::Up => {
                    selected_index = if selected_index == 0 {
                        options.len() - 1
                    } else {
                        selected_index - 1
                    };
                    render_select(
                        &mut stdout,
                        label,
                        options,
                        selected_index,
                        &mut rendered_line_count,
                    )?;
                }
                KeyCode::Down => {
                    selected_index = (selected_index + 1) % options.len();
                    render_select(
                        &mut stdout,
                        label,
                        options,
                        selected_index,
                        &mut rendered_line_count,
                    )?;
                }
                KeyCode::Enter => break Ok(options[selected_index].value),
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err("interrupted".into());
                }
                _ => {}
            }
        }
    })();
    if !was_raw_mode_enabled {
        disable_raw_mode().map_err(|error| error.to_string())?;
    }

    if let Ok(selected_value) = selection_result {
        let selected_label = options
            .iter()
            .find(|option| option.value == selected_value)
            .map(|option| option.label)
            .unwrap_or(options[0].label);
        clear_rendered_select(&mut stdout, rendered_line_count)?;
        writeln!(stdout, "{label}: {selected_label}").map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())?;
        return Ok(selected_value);
    }

    selection_result
}

fn render_select<W, T>(
    stdout: &mut W,
    label: &str,
    options: &[SelectOption<T>],
    selected_index: usize,
    rendered_line_count: &mut u16,
) -> Result<(), String>
where
    W: Write,
{
    if *rendered_line_count > 0 {
        stdout
            .execute(MoveUp(*rendered_line_count))
            .map_err(|error| error.to_string())?;
    }
    write!(stdout, "{label} (Use ↑/↓, Enter to confirm)\r\n").map_err(|error| error.to_string())?;
    for (index, option) in options.iter().enumerate() {
        stdout
            .execute(Clear(ClearType::CurrentLine))
            .map_err(|error| error.to_string())?;
        let marker = if index == selected_index { ">" } else { " " };
        write!(
            stdout,
            "\r{marker} {} - {}\r\n",
            option.label, option.description
        )
        .map_err(|error| error.to_string())?;
    }
    stdout.flush().map_err(|error| error.to_string())?;
    *rendered_line_count = (options.len() + 1) as u16;
    Ok(())
}

fn clear_rendered_select<W>(stdout: &mut W, rendered_line_count: u16) -> Result<(), String>
where
    W: Write,
{
    if rendered_line_count == 0 {
        return Ok(());
    }
    stdout
        .execute(MoveUp(rendered_line_count))
        .map_err(|error| error.to_string())?;
    for _ in 0..rendered_line_count {
        stdout
            .execute(Clear(ClearType::CurrentLine))
            .map_err(|error| error.to_string())?;
        write!(stdout, "\r\n").map_err(|error| error.to_string())?;
    }
    stdout
        .execute(MoveUp(rendered_line_count))
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

fn read_stdin_line() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| error.to_string())?;
    Ok(input.trim().to_string())
}

fn print_help() {
    print!(
        "cfsurge commands:\n  login [--api-base <url>] [--auth <service-session|cloudflare-admin>] [--username <username>] [--password <password>] [--token <token>] [--token-storage <file|keychain>]\n  init [--api-base <url>] [--slug <slug>] [--publish-dir <dir>] [--visibility <public|unlisted>]\n  publish [dir] [--slug <slug>]\n  --version\n  list\n  remove [slug]\n  passwd [--current-password <password>] [--new-password <password>]\n  admin users list\n  admin users create --username <username> [--role <user|admin>] [--temporary-password <password>]\n  admin users reset-password <username>\n  admin users disable <username>\n  admin users enable <username>\n  logout\n  interactive choices: use ↑/↓ and Enter\n"
    );
}

fn print_version() {
    println!("{}", resolve_cli_version());
}

fn resolve_cli_version() -> String {
    env::var("CFSURGE_CLI_VERSION")
        .ok()
        .and_then(|value| read_string(&value))
        .or_else(|| option_env!("CFSURGE_CLI_VERSION").map(|value| value.to_string()))
        .unwrap_or_else(|| DEV_CLI_VERSION.to_string())
}

fn read_flag(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|value| value == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn read_positional_arg(args: &[String]) -> Option<String> {
    args.iter().find(|value| !value.starts_with("--")).cloned()
}

fn read_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn read_string_opt(value: Option<&String>) -> Option<String> {
    read_string(value?.as_str())
}

fn parse_origin(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    Some(url.origin().ascii_serialization())
}

fn config_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config").join("cfsurge"))
}

fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("config.json"))
}

fn site_config_path() -> Result<PathBuf, String> {
    Ok(env::current_dir()
        .map_err(|error| error.to_string())?
        .join(SITE_CONFIG_FILE_NAME))
}

fn home_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Ok(path);
    }
    if let Some(path) = env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Ok(path);
    }
    if let Some(path) = home::home_dir() {
        return Ok(path);
    }
    Err("failed to resolve home directory".into())
}

fn append_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.push(b'\n');
    bytes
}

fn hex_digest(buffer: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(buffer);
    format!("{:x}", hasher.finalize())
}

fn format_http_error(error: reqwest::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        ApiMetadata, AuthType, FALLBACK_API_RESERVED_LABEL, assert_valid_slug,
        build_reserved_labels, normalize_api_base, parse_login_mode_input, parse_publish_dir_input,
        parse_user_role,
    };

    #[test]
    fn normalize_api_base_accepts_http_and_https_origins() {
        assert_eq!(
            normalize_api_base("https://api.example.com").expect("https normalize"),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_api_base("http://127.0.0.1:8787").expect("http normalize"),
            "http://127.0.0.1:8787"
        );
    }

    #[test]
    fn normalize_api_base_rejects_non_origin_values() {
        let with_path = normalize_api_base("https://api.example.com/v1");
        assert!(with_path.is_err());
        assert_eq!(
            with_path.expect_err("path must fail"),
            "invalid API base URL: do not include path, query, or fragment"
        );

        let with_query = normalize_api_base("https://api.example.com?foo=bar");
        assert!(with_query.is_err());
        assert_eq!(
            with_query.expect_err("query must fail"),
            "invalid API base URL: do not include path, query, or fragment"
        );
    }

    #[test]
    fn reserved_labels_include_api_and_unlisted_labels() {
        let metadata = ApiMetadata {
            api_base: Some("https://manage.example.test".to_string()),
            public_suffix: Some("example.test".to_string()),
            unlisted_host: Some("u.example.test".to_string()),
            token_creation_url: None,
        };
        let labels = build_reserved_labels(Some("https://api.example.test"), Some(&metadata));
        assert!(labels.iter().any(|value| value == "manage"));
        assert!(labels.iter().any(|value| value == "u"));
        assert!(
            !labels
                .iter()
                .any(|value| value == FALLBACK_API_RESERVED_LABEL)
        );
    }

    #[test]
    fn assert_valid_slug_rejects_reserved_slug() {
        let reserved = vec!["api".to_string(), "www".to_string()];
        let error = assert_valid_slug("api", &reserved).expect_err("reserved slug must fail");
        assert_eq!(error, "invalid slug: api (reserved_slug)");
    }

    #[test]
    fn parse_publish_dir_input_rejects_empty_string() {
        let error = parse_publish_dir_input("   ").expect_err("empty publish dir must fail");
        assert_eq!(error, "publish directory cannot be empty");
    }

    #[test]
    fn parse_login_mode_input_accepts_aliases() {
        assert_eq!(
            parse_login_mode_input(Some("service".to_string())).expect("service alias"),
            Some(AuthType::ServiceSession)
        );
        assert_eq!(
            parse_login_mode_input(Some("password".to_string())).expect("password alias"),
            Some(AuthType::ServiceSession)
        );
        assert_eq!(
            parse_login_mode_input(Some("cloudflare".to_string())).expect("cloudflare alias"),
            Some(AuthType::CloudflareAdmin)
        );
    }

    #[test]
    fn parse_user_role_rejects_unknown_role() {
        let error = parse_user_role("owner").expect_err("unknown role must fail");
        assert_eq!(error, "invalid role: expected user or admin");
    }
}
