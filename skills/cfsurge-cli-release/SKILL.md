---
name: cfsurge-cli-release
description: Help a user install and use a standalone CLI to log in, configure a project, publish a local static site directory, list published projects, remove a project, or log out. Use when the user wants copy-paste commands for macOS, Linux, or Windows, especially when starting from a GitHub Releases zip download of `cfsurge`.
---

# Cfsurge CLI Release

## Overview

Guide users through the released `cfsurge` CLI, not the repo checkout.
Assume the user starts from a GitHub Releases zip archive and wants working commands quickly.

Keep the flow focused on:
- picking the right zip for the platform
- extracting it (zip names differ by platform)
- running the unified executable name (`cfsurge` on macOS/Linux, `cfsurge.exe` on Windows)
- running `login`, `init`, `publish`, `list`, `remove`, `logout`, or `--version`
- handling `public` / `unlisted` publish via the `visibility` field in `.cfsurge.json`

Do not switch to repo-clone, cargo, or local build instructions unless the user explicitly asks for development workflow.

## Workflow

1. Stay in released-binary mode.
- Use the zip archive and extracted executable name from the current README.
- Remind users that zip names are platform-specific, but executable names are unified after extraction.
- Avoid `cargo run`, `cargo build`, or repo-clone setup.

2. Resolve the missing inputs early.
- Ask for or infer the OS: macOS Apple Silicon, Linux x64, or Windows x64.
- Ask for `apiBase` if the user has not provided it. The distributed CLI does not assume a default.
- Ask whether they already have a Cloudflare API token if `login` is part of the task.
- Ask for `slug` if the task needs `init`, `publish`, or `remove` and the user has not already provided one. Do not invent or auto-pick a slug on the user's behalf.
- Ask for the intended visibility (`public` or `unlisted`) if the task needs `init` or `publish` and the user has not already made that choice explicit. Do not assume `public` on the user's behalf.

3. Use the CLI's real first-run behavior.
- `login` prompts for `API base URL:` when `--api-base`, `CFSURGE_API_BASE`, and stored config are all absent.
- `login` then prompts for `Cloudflare API token:` when `--token`, `CFSURGE_TOKEN`, and stored config are absent.
- After `apiBase` is known, the CLI may show a token creation URL from `/v1/meta`; if that lookup fails, it falls back to a generic Cloudflare token page.
- `--token-storage` defaults to `file`; `keychain` is an explicit macOS-only choice.

4. Prefer copy-paste commands.
- Load `references/commands.md` for platform-specific command sequences.
- Give only the commands relevant to the user's OS and goal.

5. Keep config guidance minimal.
- `init` writes `.cfsurge.json` in the current project directory.
- `init` stores `slug`, `publishDir`, and `visibility`.
- `publish` and `remove` can use values from `.cfsurge.json` when explicit args are omitted.
- `logout` clears the stored login config.

## When To Use References

Load `references/commands.md` when you need:
- OS-specific unzip and executable names
- a full `login -> init -> publish -> list -> remove -> logout` sequence
- a quick `--version` or troubleshooting example

## Constraints

- Treat `apiBase` as required input unless the user already has it configured.
- Treat `slug` as user-owned input. If it is missing for `init`, `publish`, or `remove`, ask the user instead of choosing one.
- Treat visibility as user-owned input for `init` and `publish`. If it is missing, ask the user instead of defaulting to `public` in your guidance.
- Keep examples generic to `cfsurge`; `oldv.page` can be mentioned only as an example service.
- Do not describe repo-internal release workflows, generated files, or Rust development commands unless the user explicitly pivots to maintainer work.
