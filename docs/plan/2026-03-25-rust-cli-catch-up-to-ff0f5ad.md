# Execution Plan: Rust CLI catch-up to upstream `ff0f5adf58448cd47aa4dc960f5c9030c3523815`

## Goal
Bring `/Users/daiki/projects/cfsurge-cli` to behavioral parity with `/Users/daiki/projects/cfsurge/packages/cli` at commit `ff0f5adf58448cd47aa4dc960f5c9030c3523815`, scoped strictly to the post-`f02598b` delta:
- service-session password-change workflow in `login`
- password-change + automatic re-login workflow in `passwd`
- related help/docs surface updates
- required Rust test parity for the above behavior

## Delta Snapshot (Rust vs upstream ff0f5ad)
- `login` in Rust does not accept or handle `--new-password`.
- Rust `login` does not complete password-change-required accounts inline; it stores a session with `mustChangePassword=true` and asks users to run `passwd`.
- Rust `passwd` changes password then clears session and forces manual `login`; upstream now auto re-logs in and persists a fresh session.
- Rust does not enforce upstream errors/guardrails introduced by `ff0f5ad`:
  - reject `--new-password` for `cloudflare-admin` login
  - fail non-interactive required-password-change login without `--new-password` using explicit guidance text
  - fail `passwd` when stored service-session username is missing
  - clear stored session and return explicit fallback error when automatic re-login fails
- Help/docs currently omit `login --new-password` behavior and still describe old `passwd` outcome.

## Scope and Non-Goals
- Scope:
  - `src/lib.rs`
  - `tests/cli.rs` (primary contract parity)
  - `tests/login_and_cli_basics.rs` only if needed for surface/help consistency
  - `README.md` and command reference docs for behavior alignment
- Non-goals:
  - auth schema redesign
  - API changes
  - unrelated command refactors (`init/publish/list/remove/admin/logout`)

## Phase Plan

### Phase 1: Login flow parity for required password change
Target behavior:
- Add `--new-password` support to `login` command surface.
- If resolved mode is `cloudflare-admin`, reject `--new-password` with:
  - `--new-password is only available with service-session login`
- For service-session login:
  - perform initial `/v1/auth/login`
  - when response has `mustChangePassword=true`:
    - use `--new-password` when present
    - otherwise, interactive prompt: print `password change required for this account.` then prompt `New password`
    - otherwise, non-interactive error:
      - `password change required for this account. Re-run cfsurge login with --new-password <password>.`
  - call `/v1/auth/change-password` with temporary access token
  - immediately re-login with new password
  - if second login still has `mustChangePassword=true`, fail with:
    - `login failed: password change completed but server still requires password change`
  - on success, persist final service-session auth and print:
    - `password updated`
    - `logged in as <actor>`

Likely code changes:
- `src/lib.rs`:
  - add helpers equivalent to upstream:
    - parse/validate login `--new-password`
    - resolve new password with interactive/non-interactive behavior
    - service-session login request helper
    - shared change-password request helper
    - service-session auth persistence helper

Exit criteria:
- `login` matches upstream `ff0f5ad` behavior and error messages for required password-change flows.

### Phase 2: `passwd` parity with automatic re-login and fallback cleanup
Target behavior:
- Keep `passwd` service-session-only guard.
- Require stored service-session username; if missing:
  - `stored service-session username is missing. Run cfsurge login.`
- On successful `/v1/auth/change-password`:
  - perform automatic re-login using stored username + new password
  - if re-login succeeds and `mustChangePassword=false`, persist refreshed token/auth and print:
    - `password updated`
    - `logged in as <actor>`
- If automatic re-login fails for any reason:
  - clear stored service-session auth/token state (including keychain cleanup path)
  - fail with:
    - `password updated, but automatic re-login failed. Run cfsurge login with your new password.`

Likely code changes:
- `src/lib.rs`:
  - replace current `finalize_password_change_local_auth_state` terminal-revoke behavior with split helpers:
    - persist refreshed auth on success
    - clear stored auth on fallback failure

Exit criteria:
- `passwd` no longer forces manual re-login on success and matches upstream fallback semantics.

### Phase 3: Contract-test parity for ff0f5ad scenarios
Target tests to add/update in `tests/cli.rs`:
- `login_completes_required_password_change_and_relogin_in_one_flow`
- `login_fails_clearly_when_password_change_required_without_new_password_in_non_interactive_mode`
- `cloudflare_admin_login_rejects_new_password`
- update existing `must_change_password_blocks_commands_until_passwd` expectations:
  - `passwd` output becomes `password updated\nlogged in as ...\n`
  - refreshed token is stored
  - `list` succeeds immediately after `passwd`
- update keychain-backed password-change test to reflect refreshed-session behavior.
- add fallback coverage:
  - `passwd` auto re-login failure clears stored session and emits fallback error
  - `passwd` missing stored username fails with explicit guidance

Exit criteria:
- Rust contract tests cover all behavior introduced/changed by upstream `ff0f5ad` in `list.contract.test.js`.

### Phase 4: Help/docs alignment and full validation
Target behavior:
- Update help surface in `src/lib.rs`:
  - include `login ... [--new-password <password>] ...`
- Update docs:
  - `README.md`
  - `skills/cfsurge-cli-release/references/commands.md` (if command examples mention old behavior)
- Ensure docs describe:
  - inline login password-change completion
  - non-interactive `--new-password` requirement for required-change accounts
  - `passwd` automatic re-login outcome

Exit criteria:
- CLI help and docs do not contradict runtime behavior.

## Validation Commands
Run in `/Users/daiki/projects/cfsurge-cli`:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --test cli
cargo test --test login_and_cli_basics
cargo test
```

Recommended targeted checks during implementation:

```bash
cargo test --test cli login_completes_required_password_change_and_relogin_in_one_flow
cargo test --test cli login_fails_clearly_when_password_change_required_without_new_password_in_non_interactive_mode
cargo test --test cli cloudflare_admin_login_rejects_new_password
cargo test --test cli must_change_password_blocks_commands_until_passwd
```

## Completion Criteria (Decision Gate)
- Runtime parity:
  - `login` and `passwd` behavior match upstream `ff0f5ad` for required password-change workflows.
- Message parity:
  - new success/error strings listed above are implemented and tested.
- Persistence parity:
  - successful flows persist refreshed service-session auth/token correctly (file/keychain paths).
  - fallback flows clear local session state as upstream does.
- Test parity:
  - Rust tests explicitly cover upstream `ff0f5ad` additions/changes for these flows.
- Quality gate:
  - `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and full `cargo test` all pass.
