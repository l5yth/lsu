<!-- Copyright (c) 2026 l5yth -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# lsu

`lsu` is a Rust terminal UI for viewing `systemd` service units and their latest log line.

## Dependencies

- Linux system with `systemd`
- `systemctl` and `journalctl` available in `PATH`
- Rust toolchain (Rust 2021 edition, Cargo)

Core crates: `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`.

## Installation

Build from source:

```bash
git clone <repo-url>
cd lsu
cargo build --release
```

Run the built binary:

```bash
./target/release/lsu
```

Or run directly in development:

```bash
cargo run --
```

## Usage

```bash
lsu [OPTIONS]
```

Options:

- `-a`, `--all`: include non-active service units
- `-r`, `--refresh <num>`: auto-refresh every `<num>` seconds (`0` disables, default: `0`)
- `-h`, `--help`: show usage help

Examples:

```bash
lsu
lsu --all
lsu --all --refresh 5
lsu -r 0
```

In-app keys:

- `q`: quit
- `r`: refresh now

## Development

```bash
cargo check
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features -D warnings
```

