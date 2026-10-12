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

Releasing updates Homebrew automatically. When the release workflow finishes, its last job sends a `tool-released` event to `pragmatic-engineer/homebrew-tap`, which opens, checks and merges a pull request that bumps `Formula/playbook-telemetry.rb` from this release's `SHA256SUMS`. The event needs the `TAP_DISPATCH_TOKEN` repository secret, a fine-grained token limited to the tap with Contents read and write access. If the secret is missing the job logs that and succeeds, and the tap's daily run picks the release up instead.
