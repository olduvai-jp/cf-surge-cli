# Execution Plan: Rust CLI catch-up to upstream `b01041c46b709dd2bb671a18232b3cb80327c5c5`

## Goal
Port upstream CLI behavior from `/Users/daiki/projects/cfsurge` commit `b01041c46b709dd2bb671a18232b3cb80327c5c5` into `/Users/daiki/projects/cfsurge-cli`:
- add weighted `publish` progress reporting to `stderr`
- keep final success output on `stdout` unchanged: `published <slug> -> <servedUrl>`

## Upstream Delta to Mirror
- Source changes are in:
  - `packages/cli/src/index.ts`
  - `packages/cli/test/list.contract.test.js`
  - `README.md`
- Behavior to mirror:
  - publish phases: `scanning`, `preparing`, `uploading`, `activating`, `complete`
  - weighted percent model:
    - scanning: `0..10%` (`floor(scanned/total * 10)`)
    - preparing: `15%`
    - uploading: `15 + floor(completed/total * 75)`
    - activating: `90%`
    - complete: `100%`
  - non-TTY `stderr` emits line-oriented progress messages
  - TTY `stderr` renders spinner frames `| / - \` at 100ms and cleans line on stop

## Implementation Changes
- `src/lib.rs`:
  - extend `publish()` to instantiate a progress reporter, update state per phase, and always stop reporter before returning (success or error)
  - keep prepare/upload/activate API order and payloads unchanged
  - keep `stdout` final success line unchanged
  - keep all error text contracts unchanged except expected interleaved progress on `stderr`
- `src/lib.rs`:
  - update `collect_files()` to optionally report progress callbacks:
    - emit `0/0` bootstrap
    - emit `0/total` after enumeration
    - emit per-file scanned increments
  - preserve existing file sorting, SHA256, and content-type behavior
- `README.md`:
  - document that publish progress is written to `stderr`
  - document spinner behavior on TTY only
  - explicitly note final success line remains on `stdout`

## Test Plan
- `tests/cli.rs`:
  - add a reusable progress assertion helper for non-TTY `stderr` ordering
  - update successful publish tests that currently require empty `stderr`
  - assert unchanged `stdout` success line in those tests
  - add coverage for:
    - single upload path
    - multi-upload progress milestones
    - zero-upload prepare response path
    - upload failure path with readable progress + error coexistence
- Keep PTY spinner rendering out of automated tests for now; test contract is non-TTY output (piped stderr).

## Validation
- Run and require clean results:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
- Manual sanity check:
  - run a publish success scenario and verify `stdout` has only final published line
  - verify `stderr` progress reaches `100%` and phase order is monotonic

## Acceptance Criteria
- `publish` emits weighted progress updates on `stderr` aligned with upstream phase model.
- `stdout` remains stable and machine-friendly with final published line only.
- Existing publish API behavior and payload contracts are unchanged.
- Test suite is updated for new `stderr` contract and passes.
- README publish section matches implemented behavior.
