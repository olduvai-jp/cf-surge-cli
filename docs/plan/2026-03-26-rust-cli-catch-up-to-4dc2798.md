# Execution Plan: Rust CLI catch-up to upstream `4dc2798441abade63847842c62bfc58761c9739c`

## Goal
Port upstream CLI change from `/Users/daiki/projects/cfsurge` commit `4dc2798441abade63847842c62bfc58761c9739c` into `/Users/daiki/projects/cfsurge-cli`:
- add `access=link` publishing mode to Rust CLI
- support `publish --rotate-share-link`
- surface `shareUrl` in publish/list outputs
- align docs/skill help text and tests with the new surface

## Upstream Delta to Mirror
- Source changes in upstream are in:
  - `packages/cli/src/index.ts`
  - `packages/cli/test/list.contract.test.js`
  - `README.md`
- Behavior to mirror:
  - `access` becomes `public|basic|link` in all CLI surfaces that mention it
  - `publish` accepts `--rotate-share-link` only when `access=link`
  - prepare payload for `access=link` does not include `basicAuth`, but can include `rotateShareLink: true`
  - publish output prints `share url: <url>` when `prepare` response includes `shareUrl`
  - list output includes a `shareUrl` TSV column
  - help/docs reflect `link` mode and rotate flag

## Scope and Non-Goals
- Scope:
  - CLI model, flags, payload, publish/list output behavior in `src/lib.rs`
  - parity tests in `tests/cli.rs` and `tests/login_and_cli_basics.rs`
  - user-facing docs in `README.md` and `skills/cfsurge-cli-release/references/commands.md`
- Non-goals:
  - worker/server implementation changes
  - release pipeline or docs outside CLI usage surface
  - changing existing `public`/`basic` behavior

## Delivery Loop (implement/test/review until completion)

### Phase 1: Core access model and publish flag support
Implementation:
- Add `Access::Link` and map CLI access parsing/normalization to `public|basic|link`.
- Extend deprecated visibility error guidance to include `link`.
- Add interactive init selector option `link` with link-mode description.
- Add `--rotate-share-link` boolean parsing in publish argument handling.
- Gate `--rotate-share-link` to `access=link` and reject on non-link usage.

Testing:
- Add/adjust `src/lib.rs`-level tests for `normalize_access` / `parse_access_input` with `link`.
- Add CLI test for `--access link` interactive/default parsing coverage.

Review:
- Reviewer validates access enum, prompt, and flag parsing are complete and no non-link path accepts rotate-share.
- Write review result to `docs/plan_result/2026-03-26-reviewer-4dc2798-phase1.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-4dc2798-phase1.md`.

### Phase 2: Publish flow and output shape
Implementation:
- Update publish prepare payload assembly:
  - keep `files` / `access` behavior from prior phases
- include `rotateShareLink: true` only when `--rotate-share-link` is set
- keep `basicAuth` behavior unchanged for `access=basic`, absent for `public` and `link`
- extend prepare response model with optional `shareUrl`
- print `share url: <shareUrl>` as an additional `stdout` line when present

Testing:
- Add/adjust publish tests for:
  - `access=link` publish success with `shareUrl` and no `basicAuth`
  - `access=link` sets `rotateShareLink` flag in prepare payload
- Confirm existing `public/basic` publish expectations remain intact.

Review:
- Reviewer validates payload contracts, error behavior for rotate flag, and output shape.
- Write review result to `docs/plan_result/2026-03-26-reviewer-4dc2798-phase2.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-4dc2798-phase2.md`.

### Phase 3: List projection updates
Implementation:
- Extend project list record parsing with optional `shareUrl`.
- Change list TSV output columns from 6 to 7 columns:
  - `slug`, `access`, `servedUrl`, `activeDeploymentId`, `updatedAt`, `updatedBy`, `shareUrl`
- Render `shareUrl` as `-` when absent.

Testing:
- Update list tests to include a `link` row and verify final column format.
- Ensure existing public/basic list tests remain stable.

Review:
- Reviewer validates list output shape and compatibility with mixed rows.
- Write review result to `docs/plan_result/2026-03-26-reviewer-4dc2798-phase3.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-4dc2798-phase3.md`.

### Phase 4: Docs and help text alignment
Implementation:
- Update `README.md`:
  - publish command usage includes `--rotate-share-link`
  - init access description becomes `public|basic|link`
  - add `link` publish semantics and note `share url` behavior
  - update error text expectations for new `--access` value set
  - update troubleshooting messages for `--visibility` guidance and share URL usage
- Update `skills/cfsurge-cli-release/references/commands.md` command/help wording for `link`.
- Update help string assertions where `--access` and `publish` command signatures are checked.

Testing:
- Add/adjust targeted assertion tests in `tests/login_and_cli_basics.rs` and `tests/cli.rs` for help/doc-facing strings.

Review:
- Reviewer checks docs/skill coverage matches implemented command surface and no stale `public|basic`-only guidance remains.
- Write review result to `docs/plan_result/2026-03-26-reviewer-4dc2798-phase4.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-4dc2798-phase4.md`.

### Phase 5: Final verification and completion
Implementation/Test:
- Run required checks:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
- Iterate additional mini-phases if failures appear, with paired implementer/reviewer plan_result entries.

Review:
- Final reviewer checks parity against upstream `4dc2798` intent and ensures no unresolved findings.
- Write review result to `docs/plan_result/2026-03-26-reviewer-4dc2798-final.md`.

Implementation result artifact:
- Implementer writes `docs/plan_result/2026-03-26-implementer-4dc2798-final.md`.

## Acceptance Criteria (completion gate)
- Access/model parity:
  - init accepts `--access <public|basic|link>`
  - publish accepts `--rotate-share-link` and gates it to `access=link`
  - `--visibility` guidance mentions `<public|basic|link>`
- Publish parity:
  - `access=link` publish works without requiring basic auth env vars
  - rotate flag passes `rotateShareLink=true` to prepare
  - `shareUrl` is printed as `share url: <url>` when returned
- List parity:
  - list includes `shareUrl` as final TSV column with `-` fallback
- Surface parity:
  - README and command-help text reflect `link` and rotate flag
- Quality gate:
  - fmt, clippy, and full tests pass
