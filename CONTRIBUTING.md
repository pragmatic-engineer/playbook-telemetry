This project follows the standard igorjs contribution rules. Start here:

- **[.github/CONTRIBUTING-RULES.md](.github/CONTRIBUTING-RULES.md)**: DCO, CLA, commit conventions, PR process, and the code style baseline (SPDX headers, dependency policy, commit signing).
- **[README](README.md)**: project description.
- **[SECURITY.md](SECURITY.md)**: vulnerability disclosure process.

## Build and test

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::expect_used
cargo nextest run --config-file .github/nextest.toml --profile ci
```

## Try it locally

```
cargo run --bin pbtop -- --summary
cargo run --bin pbtop -- --help
```

`pbtop` and `playbook-telemetry` are the same program, so either binary works for a manual check.

## Releasing

Releasing a tool opens a bump PR in playbook and updates the Homebrew tap. When the release workflow finishes, its last job sends two events using the `TAP_DISPATCH_TOKEN` repository secret:

- `tool-released` to `pragmatic-engineer/homebrew-tap`, which opens, checks and merges a pull request that bumps `Formula/playbook-telemetry.rb` from this release's `SHA256SUMS`.
- `internal-crate-release` to `pragmatic-engineer/playbook`, with the crate, repo and tag. playbook then opens a pull request that moves its pinned `playbook-telemetry` git tag and `Cargo.lock`, runs the checks and merges it.

The secret is a fine-grained token limited to the tap and playbook, with Contents read and write access on both (the permission that allows `repository_dispatch`). If the secret is missing the job logs that and succeeds, and the daily runs in both repos pick the release up instead.
