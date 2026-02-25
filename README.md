<!-- Copyright (c) 2026 l5yth -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# lsu

`lsu` is a Rust terminal UI for viewing `systemd` service units and their journal.

![lsu terminal UI screenshot](assets/images/lsu-tui-overview.png)

## Dependencies

- any GNU/Linux system with `systemd`
- `systemctl` and `journalctl` available in `$PATH` obviosly
- Some current Rust stable toolchain (Rust 2024 edition, Cargo)

Core crates: `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`.

## Installation

Helpers exist for Arch and Gentoo-based systems but you can install also
via crates.io or from source directly.

### Archlinux

See [PKGBUILD](./packaging/archlinux/PKGBUILD)

### Gentoo

See [lsu-9999.ebuild](./packaging/gentoo/app-misc/lsu/lsu-9999.ebuild)

### Cargo Crates

```bash
cargo install lsu
```

### From Source

Build from source:

```bash
git clone https://github.com/l5yth/lsu.git
cd lsu
cargo build --release
```

Run the built binary:

```bash
./target/release/lsu
```

Or run directly in development:

```bash
cargo run --release --
```

## Usage

```text
lsu v0.1.1
apache v2 (c) 2026 l5yth

Usage: lsu [OPTIONS]

Show systemd services in a terminal UI.
By default only loaded and active units are shown.

Options:
  -a, --all            Shorthand for --load all --active all --sub all
      --load <value>   Filter by load state (all, loaded, stub, not-found, bad-setting, error, merged, masked)
      --active <value> Filter by active state (all, active, reloading, inactive, failed, activating, deactivating, maintenance, refreshing)
      --sub <value>    Filter by sub state (all, running, exited, dead, failed, start-pre, start, start-post, auto-restart, auto-restart-queued, dead-before-auto-restart, condition, reload, reload-post, reload-signal, reload-notify, stop, stop-watchdog, stop-sigterm, stop-sigkill, stop-post, final-sigterm, final-sigkill, final-watchdog, cleaning)
      --user           Show units in user instead of system scope
  -r, --refresh <num>  Auto-refresh interval in seconds (0 disables, default: 0)
  -h, --help           Show this help text
  -v, --version        Show version and copyright
```

Examples:

```bash
lsu
lsu --all
lsu --all --refresh 5
lsu --load failed
lsu --active inactive
lsu --sub exited
lsu --load loaded --active inactive --sub dead
```

In-app keys:

- `q`: quit
- `r`: refresh now
- `↑` / `↓`: move selection in service unit list
- `l` or `enter`: open detailed logs for selected service
- Log view: `↑` / `↓` scroll logs, `b` or `esc` return to list

## Development

```bash
cargo check
cargo test --all --all-features --verbose
cargo fmt --all
cargo clippy --all-targets --all-features -D warnings
```
