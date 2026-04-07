# Contributing

Project: `coinflow.rs`

## Scope

This repository is a Rust CLI for exporting XPUB wallet activity and valuation data to CSV.

## Development Setup

1. Install Rust stable via rustup.
2. Clone the repository.
3. Run:

```bash
cargo check
cargo test
```

See `LOCAL_SETUP.md` for local environment details.

## Branching and Commits

- Use focused branches per change.
- Keep commits scoped and atomic.
- Write commit messages in imperative mood.

Examples:

- `Add interval handling for monthly buckets`
- `Fix fiat valuation carry-forward logic`

## Code Standards

- Format code before opening a PR:

```bash
cargo fmt --all
```

- Lint and treat warnings as errors:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

- Ensure tests pass:

```bash
cargo test
```

## Pull Requests

PRs should include:

- problem statement
- implementation summary
- testing evidence (commands run and result)
- any behavior or CLI output changes

If CLI flags or output format changes, update `README.md` in the same PR.

## Reporting Issues

For bug reports, include:

- expected behavior
- actual behavior
- reproduction steps
- sample command used
- relevant logs/error text

Do not include wallet secrets or private keys in issues.