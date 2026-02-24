<!-- Copyright (c) 2026 l5yth -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Repository Guidelines

## Project Structure & Module Organization
This repository is a small Rust CLI/TUI project.
- `src/main.rs`: application entry point and core logic (systemd query, log fetch, table rendering, input loop).
- `Cargo.toml`: package metadata and dependencies (`ratatui`, `crossterm`, `serde`, `anyhow`).
- `Cargo.lock`: locked dependency versions.
- `target/`: build artifacts (generated; do not edit).

When the codebase grows, prefer splitting logic into `src/` modules (for example `ui.rs`, `systemd.rs`, `models.rs`) and keep `main.rs` as orchestration.

## Build, Test, and Development Commands
Use Cargo commands from the repo root:
- `cargo run`: run the TUI locally.
- `cargo build`: debug build.
- `cargo build --release`: optimized build.
- `cargo check`: fast compile-time validation without producing binaries.
- `cargo test`: run unit/integration tests.
- `cargo fmt --all`: format code.
- `cargo clippy --all-targets --all-features -D warnings`: lint with warnings treated as errors.

Note: the app shells out to `systemctl` and `journalctl`, so development/testing is Linux systemd-oriented.

## Coding Style & Naming Conventions
- Follow Rust defaults: 4-space indentation, `snake_case` for functions/variables/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep functions focused; prefer `Result<T>` with `anyhow::Context` for actionable errors.
- Run `cargo fmt` before opening a PR; keep clippy clean.

## Testing Guidelines
There are currently no committed tests; add tests with new features and bug fixes.
- Unit tests: place in `#[cfg(test)] mod tests` blocks near the code.
- Integration tests: place under `tests/` (for command behavior and parsing boundaries).
- Naming: describe behavior, e.g. `parses_systemctl_json_with_missing_fields`.

## Commit & Pull Request Guidelines
Git history is currently empty, so adopt this baseline:
- Commit messages: imperative, concise subject (optionally Conventional Commits, e.g. `feat: add manual refresh key`).
- Keep commits scoped to one logical change.
- PRs should include: summary, rationale, test/lint results, and terminal screenshots/GIFs for visible TUI changes.
- Link related issues and note any environment assumptions (for example, required systemd permissions).
