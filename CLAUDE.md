<!-- Copyright (c) 2026 l5yth -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Repository Guidelines

## Project Structure & Module Organization
`lsu` is a Rust CLI/TUI for browsing `systemd` service units and journal logs.

Current layout is modular and should stay that way:
- `src/main.rs`: binary entry point only, delegates to the library.
- `src/lib.rs`: crate root, module exports, crate-level lint policy.
- `src/app/`: runtime app orchestration.
- `src/app/tui/`: TUI runtime split by concern:
  - `input.rs`: key mapping and UI command translation.
  - `render.rs`: frame rendering only.
  - `state.rs`: status text and pure view-state helpers.
  - `workers.rs`: background worker orchestration.
- `src/cli.rs`: argument parsing and usage/version text.
- `src/systemd.rs`: `systemctl` querying/parsing logic.
- `src/journal.rs`: `journalctl` querying/parsing logic.
- `src/command.rs`: subprocess execution helpers and timeout handling.
- `src/rows.rs`: list row transformation and selection helpers.
- `src/types.rs`: shared domain/UI state types.
- `tests/`: integration/runtime tests.
- `packaging/`: Arch and Gentoo packaging files.

Architecture rule: keep modules focused and small. Avoid monolithic files and avoid mixing rendering, input, I/O, and state mutation in one place.

## Build, Test, and Development Commands
Use Cargo commands from the repo root:
- `cargo run -- --help`: quick CLI sanity check.
- `cargo run`: run the TUI locally.
- `cargo build`: debug build.
- `cargo build --release`: optimized build.
- `cargo check`: fast compile-time validation without producing binaries.
- `cargo test`: run unit/integration tests.
- `cargo fmt --all`: format code.
- `cargo clippy --all-targets --all-features -D warnings`: lint with warnings treated as errors.
- `cargo doc --no-deps`: ensure docs build cleanly.

Coverage gate (required for completed work):
- `cargo llvm-cov --all-targets --all-features --workspace --fail-under-lines 100 --fail-under-functions 100`

If `cargo llvm-cov` is not installed:
- `cargo install cargo-llvm-cov`

Note: the app shells out to `systemctl` and `journalctl`, so development/testing is Linux systemd-oriented.

## Coding Style & Naming Conventions
- Follow Rust defaults: 4-space indentation, `snake_case` for functions/variables/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep code minimal and explicit. Prefer straightforward control flow over clever abstractions.
- Keep functions focused; prefer `Result<T>` with `anyhow::Context` for actionable errors.
- Prefer pure functions for parsing/state updates; isolate side effects (terminal/process I/O) behind narrow seams.
- Keep `main.rs` thin and orchestration-only.
- Do not add dependencies unless the benefit is clear and substantial.
- Run `cargo fmt` before opening a PR; keep clippy clean.

## Testing Guidelines
All changed behavior must be test-driven and fully covered.
- Unit tests: place in `#[cfg(test)] mod tests` blocks near the code.
- Integration tests: place under `tests/` (runtime/process behavior).
- Naming: describe behavior, e.g. `parses_systemctl_json_with_missing_fields`.
- Coverage expectation: 100% unit-test coverage for touched code and 100% line/function coverage for the repository gate.
- Design for testability: if a path is hard to unit test, refactor to separate pure logic from runtime I/O and then test the pure logic directly.
- Every bug fix must include a regression test.

## Documentation Guidelines
- Keep rustdoc complete and current.
- Public API items must have rustdoc comments (the crate enforces `#![deny(missing_docs)]`).
- Non-trivial private functions, branches, and invariants should have concise inline comments explaining intent.
- Document module responsibilities at the top of each module file.
- Update docs/tests in the same change when behavior changes.

## Commit & Pull Request Guidelines
- Commit messages: imperative, concise subject (optionally Conventional Commits, e.g. `feat: add manual refresh key`).
- Keep commits scoped to one logical change.
- PRs should include: summary, rationale, architecture impact (if any), and command output for `cargo fmt`, `cargo clippy`, `cargo test`, and coverage gate.
- For visible TUI changes, include terminal screenshots/GIFs.
- Link related issues and note any environment assumptions (for example, required systemd permissions).
