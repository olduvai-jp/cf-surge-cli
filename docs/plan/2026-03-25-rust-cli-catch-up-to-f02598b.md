# Execution Plan: Rust CLI catch-up to upstream `f02598b974085611c239cc404f690a9e24d1ba9f`

## Goal
Bring `/Users/daiki/projects/cfsurge-cli` to behavioral parity with `/Users/daiki/projects/cfsurge/packages/cli/src/index.ts` at commit `f02598b974085611c239cc404f690a9e24d1ba9f` (effective CLI delta introduced by `4b7c14f feat(auth): add service-session auth and admin user management`).

## Current Gap Snapshot (Rust vs upstream)
- Missing commands: `passwd`, `admin users <list|create|reset-password|disable|enable>`.
- `login` lacks service-session mode (`--auth`, `--username`, `--password`, env fallbacks) and mode auto-selection.
- Config model/parser lacks `auth` object support (`auth.type`, `auth.tokenStorage`, `auth.accessToken`, `mustChangePassword`, etc.).
- `readConfig` does not enforce `mustChangePassword` gating for normal commands.
- `logout` does not revoke service session via `POST /v1/auth/logout` before local cleanup.
- Help text does not include new auth/admin/password command surface.
- Contract-test parity gaps remain for several upstream scenarios (listed below).

## Scope and Non-Goals
- Scope: CLI parity only for this repository (`src`, `tests`, minimal docs updates if command surface changes).
- Non-goals: worker-side behavior changes, packaging/release pipeline redesign, broad refactor unrelated to parity.

## Phase Plan

### Phase 1: Auth/Config foundation
Target behaviors:
- Parse both legacy config shape and new `auth` shape.
- Resolve stored auth type: legacy defaults to `cloudflare-admin`; `auth.type` supports `cloudflare-admin|service-session`.
- Read token from new auth storage fields first, then legacy fields; preserve keychain/file semantics and existing error strings where possible.
- Add `read_config({ allow_must_change_password })` behavior to block commands when `mustChangePassword=true` (except where explicitly allowed).

Likely files:
- `src/lib.rs` (data models, parser, token read path, config read path).
- `tests/cli.rs` (new parser/runtime contract cases).

Tests to port/add:
- Auth config parsing acceptance/rejection (`auth.type`, `auth.tokenStorage`, backward compatibility).
- `mustChangePassword` blocks normal command execution with message: `password change required. Run cfsurge passwd.`

Exit criteria:
- Legacy login configs still work unchanged.
- Auth-shaped configs are accepted and used for token resolution.

### Phase 2: Login parity (cloudflare-admin + service-session)
Target behaviors:
- `login` mode selection parity:
  - explicit `--auth` (`service-session|cloudflare-admin` plus aliases).
  - implicit `cloudflare-admin` when token provided by flag/env.
  - implicit `service-session` otherwise.
- Service-session login flow:
  - resolve username/password (`--username/--password`, `CFSURGE_USERNAME/CFSURGE_PASSWORD`, prompt fallback).
  - call `POST /v1/auth/login`.
  - persist auth block with `type=service-session`, token storage, actor/username/role, `mustChangePassword`.
  - print required password-change hint when flagged by server.
- Cloudflare-admin flow remains compatible with existing behavior and writes auth metadata.

Likely files:
- `src/lib.rs` (login flow, new helpers: login-mode parsing, username/password resolution).
- `tests/cli.rs`, optionally `tests/login_and_cli_basics.rs` (service-session scenarios).

Tests to port/add:
- `login defaults to service-session auth with username/password`.
- Token-mode guard: token-based login without `cloudflare-admin` mode must error.
- Ensure existing cloudflare token login tests remain green.

Exit criteria:
- Both auth modes produce expected config JSON and follow upstream output/error behavior.

### Phase 3: New commands (`passwd`, `admin users`, enhanced `logout`)
Target behaviors:
- Add `passwd` command:
  - only valid for `service-session` login.
  - `POST /v1/auth/change-password` with current/new password.
  - clear stored `mustChangePassword` on success.
- Add `admin users` command group:
  - `list` -> GET `/v1/admin/users`, TSV output with `yes/no` for `mustChangePassword`.
  - `create` -> POST `/v1/admin/users` with `username`, `role` (default `user`), optional `temporaryPassword`.
  - `reset-password`, `disable`, `enable` -> POST expected endpoints with positional username.
- Update `logout`:
  - if stored auth type is `service-session`, attempt `POST /v1/auth/logout` with bearer token before local deletion.
  - always continue local cleanup even if remote revoke fails.

Likely files:
- `src/lib.rs` (dispatch, command impls, helper parsers/prompts, help text).
- `tests/cli.rs` (new command contract tests).

Tests to port/add:
- `mustChangePassword blocks normal commands until passwd succeeds`.
- `admin users commands call expected endpoints`.
- `logout revokes service session before clearing local state`.

Exit criteria:
- New commands and logout semantics match upstream behavior and outputs.

### Phase 4: Fill remaining contract-test parity gaps and docs alignment
Target behaviors:
- Port remaining upstream contract scenarios currently missing in Rust tests:
  - `init still saves site config when metadata is unavailable`.
  - `init fails with clear error when prompted apiBase is invalid`.
  - `publish explicit args override .cfsurge.json`.
  - `remove explicit slug overrides .cfsurge.json slug`.
  - `remove reserves API host first label from configured apiBase`.
  - `remove reserves unlisted host label`.
- Update help output to include full command surface and flags.
- If command surface in README is now stale, update `README.md` command list and auth env vars.

Likely files:
- `tests/cli.rs`.
- `src/lib.rs`.
- `README.md` (only if needed for parity/documentation consistency).

Exit criteria:
- Rust test matrix covers all upstream `packages/cli/test/list.contract.test.js` scenarios relevant to CLI behavior at `f02598b`.

## Validation Commands
Run in `/Users/daiki/projects/cfsurge-cli`:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Recommended targeted checks during implementation:

```bash
cargo test --test cli login_defaults_to_service_session_auth_with_username_password
cargo test --test cli mustChangePassword_blocks_normal_commands_until_passwd_succeeds
cargo test --test cli admin_users_commands_call_expected_endpoints
cargo test --test cli logout_revokes_service_session_before_clearing_local_state
```

## Completion Criteria (Decision Gate)
- Command parity: `login`, `init`, `publish`, `list`, `remove`, `passwd`, `admin users ...`, `logout`, `--help`, `--version` match upstream behavior for the covered contract cases.
- Config parity: both legacy and `auth` schemas are supported; token storage semantics preserved.
- Security/session parity: service-session logout revocation attempted; `mustChangePassword` enforcement and reset flow implemented.
- Test parity: all upstream contract scenarios listed above exist in Rust tests and pass.
- Quality gate: `fmt`, `clippy -D warnings`, and full `cargo test` pass.
