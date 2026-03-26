# Execution Plan: Rust CLI catch-up to upstream `6d42db9fc59943327fcd87e588fa1aa77f8b0bca`

## Goal
Bring `/Users/daiki/projects/cfsurge-cli` to behavioral parity with the upstream CLI change introduced at `6d42db9fc59943327fcd87e588fa1aa77f8b0bca`:
- replace publish mode `visibility` (`public|unlisted`) with `access` (`public|basic`)
- remove unlisted URL flow and `unlistedHost` dependency
- support `basic` publish credentials via environment variables
- align help/docs/tests with the new surface

## Delta Snapshot (current Rust vs upstream 6d42db9f)
- Rust still models site mode as `Visibility` and serializes `visibility` in `.cfsurge.json` and `/deployments/prepare` payload.
- Rust still supports `unlisted` prompt/flag/help/output and validates `unlistedHost` from `/v1/meta`.
- Rust `list` prints `visibility` column from API response.
- Rust still accepts only `--visibility` for `init`.
- Rust README and skill references still describe `unlisted` behavior.
- Rust tests still assert `visibility` payload/output and unlisted-specific error paths.

## Scope and Non-Goals
- Scope:
  - CLI behavior and data model updates in `src/lib.rs`
  - parity tests in `tests/cli.rs` (and `tests/login_and_cli_basics.rs` only if help surface assertions require)
  - user-facing docs in `README.md` and `skills/cfsurge-cli-release/*`
- Non-goals:
  - worker/server implementation changes
  - release pipeline changes
  - unrelated auth/admin/password behavior refactors

## Delivery Loop (implement/test/review until completion)

### Phase 1: Core CLI model and command-surface migration
Implementation:
- Replace `Visibility` enum and `SiteConfig.visibility` with `Access` enum and `SiteConfig.access` (`public|basic`).
- Change `ProjectRecord` parsing from `visibility` to `access` and list output column accordingly.
- Replace `resolve_visibility`/`parse_visibility_input`/`normalize_visibility` with access equivalents.
- Update `init` CLI option from `--visibility` to `--access`.
- Add hard error for deprecated `--visibility` usage (do not silently map).
- Remove `ApiMetadata.unlisted_host` usage from `init` preview and publish flow.
- Keep reserved slug handling for `u` label unchanged.

Testing:
- Update existing unit tests in `src/lib.rs` that reference unlisted host metadata, keeping reserved-label intent.
- Update CLI tests that assert help text and init/list behavior for new `access` surface.

Review:
- Reviewer validates no remaining runtime path requires `unlistedHost` and no command still documents `--visibility`.
- Write review result to `docs/plan_result/2026-03-26-reviewer-6d42db9f-phase1.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-6d42db9f-phase1.md`.

### Phase 2: Publish payload and basic-auth credential flow
Implementation:
- Change publish prepare payload from `{ files, visibility }` to `{ files, access, basicAuth? }`.
- For `access=basic`, require both env vars:
  - `CFSURGE_BASIC_AUTH_USERNAME`
  - `CFSURGE_BASIC_AUTH_PASSWORD`
- Validate credentials with upstream-compatible constraints:
  - username: non-empty, printable ASCII, must not contain `:`
  - password: non-empty, printable ASCII
- Do not persist basic credentials in `.cfsurge.json`.

Testing:
- Add/adjust tests for:
  - `publish` with `access=public` sends no `basicAuth`
  - `publish` with `access=basic` includes `basicAuth`
  - missing env vars fail with explicit guidance
  - invalid credential formats fail deterministically
- Remove obsolete unlisted publish tests:
  - `publish_unlisted_fails_when_meta_lacks_unlisted_host`
  - `publish_unlisted_sends_visibility_and_prints_served_url`

Review:
- Reviewer checks payload shape, env dependency, and credential-validation parity.
- Write review result to `docs/plan_result/2026-03-26-reviewer-6d42db9f-phase2.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-6d42db9f-phase2.md`.

### Phase 3: Backward-compat migration behavior for existing `.cfsurge.json`
Implementation:
- In site-config parser:
  - accept `access` when present (`public|basic`)
  - accept legacy `visibility: "public"` as `access=public`
  - reject legacy `visibility: "unlisted"` with migration-required error instructing `access: "basic"` and env vars
- Default to `access=public` when neither field exists but config is otherwise valid.

Testing:
- Add/adjust tests for all migration branches above.
- Ensure legacy public config still publishes successfully.

Review:
- Reviewer verifies legacy compatibility for public sites and explicit failure path for legacy unlisted.
- Write review result to `docs/plan_result/2026-03-26-reviewer-6d42db9f-phase3.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-6d42db9f-phase3.md`.

### Phase 4: Docs and skill alignment
Implementation:
- Update `README.md`:
  - public/basic terminology
  - `init --access <public|basic>`
  - list TSV column `access`
  - `.cfsurge.json` key `access`
  - troubleshooting strings for access/deprecated visibility/basic auth env vars
- Update skill docs:
  - `skills/cfsurge-cli-release/SKILL.md`
  - `skills/cfsurge-cli-release/references/commands.md`

Testing:
- Only adjust tests if they assert help/doc-facing command strings.

Review:
- Reviewer ensures docs match actual CLI behavior and no lingering unlisted instructions remain in user guidance files.
- Write review result to `docs/plan_result/2026-03-26-reviewer-6d42db9f-phase4.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-6d42db9f-phase4.md`.

### Phase 5: Final verification gate and completion decision
Implementation/Test:
- Run full required checks:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
- If any check fails, open follow-up implementation/review mini-loop(s) and append new phase result docs under `docs/plan_result/` until clean.

Review:
- Final reviewer sign-off confirming parity with upstream `6d42db9f` intent and no unresolved findings.
- Write review result to `docs/plan_result/2026-03-26-reviewer-6d42db9f-final.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-6d42db9f-final.md`.

## Acceptance Criteria (completion gate)
- CLI surface parity:
  - `init` uses `--access <public|basic>`
  - deprecated `--visibility` errors clearly
  - `publish` sends `access` and optional `basicAuth`
  - `list` outputs `access` column
- Migration parity:
  - legacy `.cfsurge.json` with `visibility:"public"` works
  - legacy `.cfsurge.json` with `visibility:"unlisted"` fails with migration guidance
- Behavior parity:
  - no runtime dependency on `unlistedHost`
  - basic credentials are env-only and never stored in site config
- Documentation parity:
  - README and skill reference files describe `access/basic` and required env vars
- Quality gate:
  - fmt, clippy, and full tests pass
