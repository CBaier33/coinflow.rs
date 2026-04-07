# coinflow.rs

CLI tool for privately collecting crypto wallet history into CSV formats suitable for reporting or accounting workflows.

Future developments will support other chains.

## Repository Status

- Project name: `coinflow.rs`
- Language: Rust (edition 2024)
- Package name: `coinflow`
- Library crate path: `coinflow`
- Executable: `coinflow`
- Main command: `export-csv`

## Features

- Connects to an Electrum server
- Scans XPUB transaction history
- Fetches spot and historical prices from mempool.space
- Exports CSV in either:
  - `report` format (detailed wallet/valuation rows)
  - `actual` format (ready for import to Actual budget because I'm lazy)

## Prerequisites

- Rust toolchain (stable)
- An Electrum endpoint (Personal instance recommended for privacy)

Install Rust (if needed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

## Local Setup

For full local setup details, see `LOCAL_SETUP.md`.

Minimal setup:

```bash
cargo check
cargo build
```

## Usage

Show command help:

```bash
cargo run -- --help
cargo run -- export-csv --help
```

Example export:

```bash
cargo run -- export-csv \
  --electrum-url ssl://electrum.blockstream.info:60002 \
  --xpub "<YOUR_XPUB>" \
  --name wallet \
  --address-type p2wpkh \
  --fiat usd \
  --interval day \
  --format report \
  --output wallet_report.csv
```

## Development Commands

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Contributing

See `CONTRIBUTING.md` for contribution workflow and coding standards.

## Push Checklist

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test`
- [ ] Update docs if CLI behavior changed
- [ ] Verify no secrets are committed