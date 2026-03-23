# Execution Plan: rust-cli-port v1 (phased)

## Summary
Build `/Users/daiki/projects/cfsurge-cli` as an independent Rust CLI repository with behavioral parity to `/Users/daiki/projects/cfsurge/packages/cli`.

Required public parity for v1:
- Commands: `login`, `init`, `publish`, `list`, `remove`, `logout`, `--version`, `--help`
- Config files: `~/.config/cfsurge/config.json` and `.cfsurge.json`
- Release assets: `cfsurge-darwin-arm64.zip`, `cfsurge-linux-x64.zip`, `cfsurge-windows-x64.zip`, `SHA256SUMS`

## Implementation Contract
- Source of truth for behavior is the TypeScript CLI implementation and tests in `/Users/daiki/projects/cfsurge/packages/cli`.
- Backend API contract is unchanged:
  - `POST /v1/auth/verify`
  - `GET /v1/meta`
  - `GET /v1/projects`
  - `POST /v1/projects/:slug/deployments/prepare`
  - `POST /v1/projects/:slug/deployments/:deploymentId/activate`
  - `DELETE /v1/projects/:slug`
- Priority order when conflicts appear:
  - machine-readable compatibility (`list` TSV and config JSON shape)
  - command flags and resolution order
  - prompt labels and success/error text
  - internal implementation details

## Delivery Phases

### Phase 0: Bootstrap and Architecture
Scope:
- Initialize Cargo binary project and module layout.
- Add dependencies (`clap`, `tokio`, `reqwest`, `serde`, `serde_json`, `sha2`, `walkdir`, `directories`).
- Define shared data models for config and API payloads.
Exit criteria:
- `cargo check` passes.
- `--help` and dispatch skeleton for all required commands exists.
- Module boundaries are clear enough for parallel command implementation.

### Phase 1: Config, Validation, and Auth Base
Scope:
- Implement config IO compatibility for global and site-local files.
- Implement API base normalization and slug validation/reserved-label rules.
- Implement token read/write with macOS Keychain (`security`) and file fallback.
- Implement `login` with `/v1/meta` token hint and `/v1/auth/verify`.
Exit criteria:
- `login` behavior matches TS fallback order and prompt behavior.
- Global config and token storage semantics match TS expectations.
- Integration tests for `login` and config parsing pass.

### Phase 2: Project Init and Publish Flow
Scope:
- Implement `init` (`slug`, `publishDir`, `visibility`) and preview output.
- Implement recursive file collection, content-type mapping, SHA256 digest generation.
- Implement `publish` prepare/upload/activate flow and visibility handling.
- Keep auth header attachment rule for upload URLs (same-origin with `apiBase` only).
Exit criteria:
- `init` and `publish` parity tests pass, including unlisted behavior gates.
- Error handling for empty publish target and invalid inputs is covered.

### Phase 3: List, Remove, Logout, Version
Scope:
- Implement `list` with TSV output and legacy fallback behavior.
- Implement `remove` with config/default slug resolution and validation.
- Implement `logout` cleanup semantics.
- Implement deterministic `--version` behavior (`0.0.0-dev` local, injected release version in release builds).
Exit criteria:
- Remaining command parity tests pass.
- CLI command surface is fully compatible for documented usage.

### Phase 4: Release Pipeline and Packaging
Scope:
- Add GitHub Actions release workflow for tag builds.
- Build and package three target zips and generate `SHA256SUMS`.
- Add binary smoke checks (`--help`) per runner.
Exit criteria:
- Workflow validates expected artifacts and checksums.
- Tag pipeline is reproducible from clean checkout.

## Iterative Loop Policy
- For each phase, run this loop until the phase exit criteria is fully met:
  - implement and test
  - independent review
  - fix findings
  - re-test
  - re-review if fixes are non-trivial
- Advancement rule:
  - do not start the next phase while open high-severity review findings exist in the current phase.
- Completion rule:
  - all phase exit criteria are met
  - no open high-severity findings
  - known medium/low items are either fixed or explicitly accepted with rationale in `docs/plan_result`

## Test Expectations
- Unit and integration tests must cover at least:
  - command/flag resolution order
  - prompt-required flows (`login` API base prompt)
  - config compatibility (`config.json`, `.cfsurge.json`)
  - publish pipeline (`prepare -> upload -> activate`)
  - visibility behavior (`public`, `unlisted`)
  - list TSV output compatibility
  - remove/logout semantics
  - version behavior in local and release contexts
- CI baseline for completion:
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
  - release workflow dry-run checks (or equivalent local validation scripts)

## Assumptions
- Repository is intentionally independent from `/Users/daiki/projects/cfsurge`.
- `darwin-x64` is out of scope for v1.
- Linux and Windows keep file-based token storage in v1; macOS Keychain compatibility is required.
