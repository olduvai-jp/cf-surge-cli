---
name: cfsurge-cli-release
description: Use the zip-distributed `cfsurge` CLI from GitHub Releases without cloning this repo. Use when asked to download or run the standalone CLI, guide `login`/`init`/`publish`/`list`/`remove`, explain the first-run `API base URL:` prompt, or give copy-paste commands for macOS, Linux, or Windows.
---

# Cfsurge CLI Release

## Overview

Guide users through the released `cfsurge` CLI, not the repo checkout.
Assume the user starts from a GitHub Releases zip archive and wants working commands quickly.

Keep the flow focused on:
- picking the right zip for the platform
- extracting it (zip names differ by platform)
- running the unified executable name (`cfsurge` on macOS/Linux, `cfsurge.exe` on Windows)
- running `login`, `init`, `publish`, `list`, `remove`, or `--version`
- handling `public` / `basic` / `link` publish via the `access` field in `.cfsurge.json`
- reminding that `access=basic` requires `CFSURGE_BASIC_AUTH_USERNAME` and `CFSURGE_BASIC_AUTH_PASSWORD` on every publish
- explaining `publish --rotate-share-link` for `access=link`

Do not switch to repo-clone, npm, or local build instructions unless the user explicitly asks for development workflow.

## Workflow

1. Stay in released-binary mode.
- Use the zip archive and extracted executable name from the current README.
- Remind users that zip names are platform-specific, but executable names are unified after extraction.
- Avoid `node packages/cli/dist/index.js ...` and avoid `npm install`.

2. Resolve the missing inputs early.
- Ask for or infer the OS: macOS Apple Silicon, Linux x64, or Windows x64.
- Ask for `apiBase` if the user has not provided it. The distributed CLI does not assume a default.
- Ask whether they already have a Cloudflare API token if `login` is part of the task.
- Ask for `slug` if the task needs `init`, `publish`, or `remove` and the user has not provided one.
- Ask for access mode (`public`, `basic`, or `link`) if the task needs `init` or `publish` and the user has not provided one.

3. Use the CLI's real first-run behavior.
- `login` prompts for `API base URL:` when `--api-base`, `CFSURGE_API_BASE`, and stored config are all absent.
- After `apiBase` is known, the CLI may show a token creation URL from `/v1/meta`; if that lookup fails, it falls back to a generic Cloudflare token page.

4. Prefer copy-paste commands.
- Load `references/commands.md` for platform-specific command sequences.
- Give only the commands relevant to the user's OS and goal.

5. Keep config guidance minimal.
- `init` writes `.cfsurge.json` in the current project directory.
- `init` stores `slug`, `publishDir`, and `access`.
- Basic credentials are not stored in `.cfsurge.json`; they are passed via environment variables during `publish`.
- `publish --rotate-share-link` is available only when `.cfsurge.json` has `access: "link"`.
- `publish` can print `share url: ...` when the API returns a share URL.
- `publish` and `remove` can use values from `.cfsurge.json` when explicit args are omitted.

## When To Use References

Load `references/commands.md` when you need:
- OS-specific unzip and executable names
- a full `login -> init -> publish -> list -> remove` sequence
- a quick `--version` or troubleshooting example

## Constraints

- Treat `apiBase` as required input unless the user already has it configured.
- Treat `slug` as user-owned input for `init`, `publish`, and `remove`.
- Treat access mode (`public`, `basic`, or `link`) as user-owned input for `init` and `publish`.
- Keep examples generic to `cfsurge`; `oldv.page` can be mentioned only as an example service.
- Do not describe repo-internal release workflows, generated files, or operator config unless the user explicitly pivots to maintainer work.
