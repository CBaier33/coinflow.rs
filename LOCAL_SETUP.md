# Local Setup

Project: `coinflow.rs`

This document describes a minimal and repeatable local development setup.

## 1. Install Toolchain

Install Rust stable:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
```

Verify:

```bash
rustc --version
cargo --version
```

## 2. Build and Validate

From repository root:

```bash
cargo check
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## 3. Run the CLI

Display help:

```bash
cargo run -- --help
cargo run -- export-csv --help
```

Example execution:

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

## 4. Common Troubleshooting

- TLS or connection errors:
  - verify Electrum URL and port
  - confirm endpoint supports your transport (`ssl://` vs `tcp://`)
- Empty history:
  - validate XPUB and address type pairing
  - try `--include-unconfirmed`
- Missing price rows:
  - verify network access to `mempool.space`

## 5. Optional: Local Electrum Endpoint

If you run your own Electrum server, pass its URL using `--electrum-url`.
No code changes are required.