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
