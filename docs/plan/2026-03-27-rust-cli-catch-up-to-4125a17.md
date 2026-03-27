# Execution Plan: Rust CLI catch-up to upstream `4125a171544909e8e313b34b53433c4fa6a86d1a`

## Summary
Port the upstream CLI behavior change where `publish` must treat `activate` as the source of truth for final URLs.

Current Rust behavior reads final `servedUrl/publicUrl/shareUrl` from `prepare` response after successful activation.  
Target behavior is to read these values from `activate` response and fail with an `activate`-scoped error when final URL fields are missing.

## Concrete Implementation Changes

### 1) Response models in `src/lib.rs`
- Keep `PrepareResponse` focused on prepare-phase fields used by this CLI path:
  - `deployment_id`
  - `upload_urls`
- Add a dedicated `ActivateResponse` model with optional:
  - `served_url`
  - `public_url`
  - `share_url`

### 2) `publish()` control flow in `src/lib.rs`
- Keep prepare/upload/activate request order and payload shape unchanged.
- After successful `activate`, parse JSON as `ActivateResponse`.
- Compute final URL from:
  - `activate.served_url`
  - fallback `activate.public_url`
- Replace current prepare-based URL extraction with activate-based extraction.
- Keep `share url: ...` output, but source it only from `activate.share_url`.
- Update missing-final-URL error message to:
  - `activate failed: missing servedUrl/publicUrl in response`

### 3) Docs alignment
- Update README statement that currently says `shareUrl` is printed when `prepare` returns it.
- Rephrase to reflect that final publish output is based on activation result (no CLI surface change).

## Test Plan

### Update existing publish success fixtures in `tests/cli.rs`
- For successful publish cases, return final `servedUrl` from activate handlers.
- Ensure assertions still verify unchanged stdout contract:
  - `published <slug> -> <url>`
  - optional `share url: <url>`

### Add regression coverage for this commit intent
- New test: prepare returns preview URL(s), activate returns different final URL(s); assert CLI prints activate values.
- New test: activate succeeds (HTTP 200) but omits both `servedUrl` and `publicUrl`; assert:
  - non-zero exit
  - `activate failed: missing servedUrl/publicUrl in response`

### Keep existing behavior checks stable
- Progress stderr ordering/format remains unchanged.
- `rotateShareLink` request payload behavior unchanged.
- `basicAuth` payload inclusion behavior unchanged.
- Upload failure path still does not call activate.

## Verification Commands
- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

## Acceptance Criteria
- `publish` final URL and optional share URL are derived from `activate` response, not `prepare`.
- CLI output format and command surface remain unchanged.
- Missing final URL in activate response fails with activate-scoped error text.
- Existing publish/list/access/basic/link contracts remain passing after test updates.
- Full verification commands pass.
