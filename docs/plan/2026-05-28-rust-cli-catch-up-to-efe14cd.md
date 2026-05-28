# Execution Plan: Rust CLI catch-up to upstream `efe14cd29f1ca1730cc0fc9158542ad9ad4ac381`

## Goal
Bring `/Users/daiki/projects/cfsurge-cli` to client-side behavioral parity with upstream `/Users/daiki/projects/cfsurge` commit `efe14cd29f1ca1730cc0fc9158542ad9ad4ac381` for cancellable upload sessions:

- after `publish` receives a prepared `deploymentId`, any later publish failure must send a best-effort cancel request
- upload failure, activate HTTP failure, and activate response parsing/final URL failures must preserve the original publish error
- cancel failure must be reported as a warning on stderr without replacing the original error
- successful publish must not call cancel
- prepare failure or pre-prepare validation failure must not call cancel
- interrupt signals during an in-flight prepared publish must attempt cancel once before process termination

## Upstream Delta Summary
The TypeScript upstream commit adds:

- CLI-side prepared deployment tracking in `publish`
- `POST /v1/projects/:slug/deployments/:deploymentId/cancel`
- best-effort cancellation in the CLI catch path
- `SIGINT`/`SIGTERM` handlers that cancel the current prepared deployment once, then re-signal the process
- server-side cancel and 30-minute upload-session TTL cleanup in the worker/durable object
- contract tests for cancel success, idempotency, mismatches, cleanup warnings, TTL recovery, and CLI failure behavior

This Rust CLI repo owns only the standalone CLI. Worker/durable-object TTL behavior is out of scope here except for compatibility with response fields returned by the server.

## Current Rust State
Relevant current implementation is in `src/lib.rs`:

- `publish()` prepares, uploads, activates, then prints final URLs from `ActivateResponse`
- `PrepareResponse` already includes `deployment_id` and `upload_urls`
- upload failure currently exits before activate and does not cancel
- activate failure and missing final URL currently do not cancel
- there is no signal handling dependency or cancellation helper
- tests in `tests/cli.rs` cover publish success, upload failure, activate missing URL, access/basic/link behavior, and progress output

## Scope Boundaries
In scope:

- `src/lib.rs` publish control flow and helper functions
- `Cargo.toml` / `Cargo.lock` only if a signal-handling crate is added
- focused CLI integration tests in `tests/cli.rs`

Out of scope:

- implementing worker routes, durable-object TTL cleanup, R2 object deletion, or server contract tests in the Rust CLI repo
- changing CLI command surface, output format, progress format, auth behavior, access/basic/link behavior, or README content unless tests reveal stale statements about cancel behavior
- refactoring publish into async or replacing blocking `reqwest`
- changing existing successful publish stdout

## Implementation Plan

### 1. Add a cancel helper
Implement a small helper in `src/lib.rs`:

```rust
fn cancel_prepared_deployment(config: &CliConfig, slug: &str, deployment_id: &str) -> Result<(), String>
```

Behavior:

- send `POST {apiBase}/v1/projects/{slug}/deployments/{deploymentId}/cancel`
- use the same bearer auth headers as prepare/activate
- send no body, matching upstream CLI behavior
- if the response is non-2xx, return an error formatted as `{status} {body}`, for example `500 cancel-broken`
- use `format_http_error` for transport errors

Decision: keep this helper private and located near `publish()` because no other command should call it.

### 2. Track prepared publish state
Inside `publish()`, after config/slug/basic-auth setup and before entering the publish work closure, introduce shared state with these concepts:

- `prepared_deployment_id: Option<String>`
- `publish_activated: bool`
- `cancel_attempted: bool`

Because signal handling needs to read this state from another thread, store cancellation data in an `Arc<Mutex<PublishCancelState>>`. The state should include cloned `api_base`, `token`, `slug`, and `deployment_id` so the interrupt path does not borrow stack data.

Update state immediately after parsing `PrepareResponse`:

- set `deployment_id` to `prepared.deployment_id.clone()`
- leave it set until publish completes
- set `publish_activated = true` only after successful activate response parsing and final URL validation, immediately before or after progress reaches `Complete`

### 3. Cancel once on post-prepare failures
Wrap the existing publish work so that failures after prepare call a `cancel_prepared_once(...)` helper before returning the original error.

Required behavior:

- no cancel before a deployment ID exists
- no cancel after activation has succeeded
- no duplicate cancel if both the failure path and signal path race
- if cancel succeeds, stderr contains only the original error and normal progress lines
- if cancel fails, append:
  - `warning: failed to cancel upload session (<cancel error>)`
- return the original publish error, not the cancel error

Recommended structure:

- keep the current `publish_result = (|| -> Result<...>)()` pattern if desired
- after the closure returns `Err(error)`, call `cancel_prepared_once`
- if cancel warns, write warning to stderr, then return `Err(error)`
- ensure `progress.stop()` still runs exactly once before final error printing by `main`

### 4. Handle interrupt cancellation
Add `signal-hook = "0.3"` to `Cargo.toml` and update `Cargo.lock`.

Decision: use `signal-hook` instead of hand-written OS signal code because the target behavior requires both `SIGINT` and `SIGTERM` and the CLI currently has no signal abstraction.

Implementation shape:

- register `SIGINT` and `SIGTERM` at the start of `publish()` after the cancel state exists
- spawn a small listener thread that waits for those signals
- when a signal arrives:
  - call `cancel_prepared_once`
  - write the same warning format if cancellation fails
  - restore/default or unregister the handler as needed
  - re-raise the received signal, or exit with `128 + signal` if re-raise is not reliable in tests
- after publish finishes normally or with a handled error:
  - close the signal iterator/handle so the listener exits promptly
  - join the listener thread best-effort before returning from `publish()`

Important implementation constraint:

- do not perform network I/O from a raw signal handler; only do it in the signal listener thread.

Testing the exact OS-level termination can be brittle, so cover the helper/state behavior thoroughly and add one integration-style signal test only if it is reliable on the local platform.

### 5. Preserve response compatibility
Do not require or surface new server fields:

- `PrepareResponse` may ignore upstream worker fields like `expiredDeploymentId` and `warning`
- cancel response JSON does not need to be parsed by the CLI
- activate response handling remains sourced from `ActivateResponse`

This keeps the CLI compatible with the worker-side TTL cleanup without adding server-owned logic to the Rust client.

## Test Plan

### Update existing tests
In `tests/cli.rs`:

- update `publish_uses_site_config_defaults` to assert no request URL ends with `/cancel`
- update `publish_upload_failure_includes_progress_and_error`:
  - stub `POST /v1/projects/failing-upload/deployments/dep-fail/cancel` as 200
  - assert exactly one cancel request
  - assert stderr does not include `warning: failed to cancel upload session`
  - retain existing progress and original upload error assertions
- update `publish_fails_when_activate_response_is_missing_final_url`:
  - stub cancel as 200
  - assert exactly one cancel request
  - assert original missing URL error is preserved

### Add new regression tests
Add focused publish tests in `tests/cli.rs`:

- `publish_upload_failure_preserves_original_error_when_cancel_fails`
  - prepare succeeds with `dep-cancel-fail`
  - upload returns `500 upload-original`
  - cancel returns `500 cancel-broken`
  - assert exit code 1
  - assert stderr contains `upload failed for index.html: upload-original`
  - assert stderr contains `warning: failed to cancel upload session (500 cancel-broken)`

- `publish_activate_failure_sends_cancel_and_preserves_activate_error`
  - prepare succeeds
  - upload succeeds or `uploadUrls` is empty
  - activate returns `409 activate-original`
  - cancel returns 200
  - assert one cancel request
  - assert stderr contains `activate failed: activate-original`

- `publish_prepare_failure_does_not_send_cancel`
  - prepare returns `409 prepare-original`
  - assert no cancel request
  - assert stderr contains `prepare failed: prepare-original`

- `publish_missing_file_descriptor_sends_cancel`
  - prepare returns an upload URL for a path not present in collected files
  - assert original `missing file descriptor for ...` error
  - assert one cancel request

- `publish_success_does_not_cancel`
  - if the existing success test is not enough, add an explicit test that a server-side cancel handler would fail the test if called

### Optional signal test
Add a Unix-only integration test if it proves stable:

- run `cfsurge publish` against a stub server that returns prepare success and then blocks upload or activate
- send `SIGINT` to the child after prepare is observed
- assert the stub received one cancel request
- assert process terminates non-successfully

If this test is flaky locally, do not include it in the default suite. Instead, keep signal behavior documented in this plan and verify manually during implementation.

## Verification Commands
Run after implementation:

- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

For interrupt behavior, also run a manual smoke test if no automated signal test is committed:

1. start a local stub server that delays upload or activate after prepare
2. run `cfsurge publish`
3. send `SIGINT`
4. verify the stub received `POST /v1/projects/<slug>/deployments/<deploymentId>/cancel`

## Acceptance Criteria
- Successful publish behavior and stdout are unchanged.
- Prepare failures and pre-prepare failures do not send cancel.
- Any failure after a successful prepare and before validated activation completion sends exactly one cancel request.
- Original publish failure text remains the primary error.
- Cancel failures produce `warning: failed to cancel upload session (...)` on stderr.
- Interrupt handling attempts cancellation for an in-flight prepared deployment.
- Signal listener/thread does not leak after normal publish or normal publish errors.
- Full Rust formatting, clippy, and test verification pass.

## Assumptions
- The Rust CLI should catch up only the CLI-owned behavior from upstream `efe14cd`; server-side cancel semantics and TTL cleanup already live in the upstream worker and are not ported into this repo.
- The cancel endpoint accepts an authenticated empty `POST` body, matching the TypeScript CLI.
- Adding one small dependency for robust signal handling is acceptable for this standalone CLI.
- If exact re-signal semantics are difficult to test portably, implementation may use conventional `128 + signal` process termination after best-effort cancel, but should prefer re-raising where reliable.

## Risks
- Blocking `reqwest` cancellation from a signal listener can briefly delay process exit while the cancel request times out. Keep default client behavior unless implementation discovers a need for a short cancel-specific timeout.
- Signal tests can be platform-sensitive. Do not weaken the default test suite with flaky process-signal timing.
- Shared cancellation state introduces race potential between normal failure and interrupt paths; the `cancel_attempted` guard must be protected by the same mutex as the deployment ID.
