# playbook-usage

A token and cost dashboard for Claude Code sessions. It reads the session history Claude Code already keeps on your machine, prices it, and shows it in a terminal view or as a text or JSON report. Everything stays local.

It is part of the [playbook](https://github.com/pragmatic-engineer/playbook) toolkit and also works on its own. If you use playbook, you do not need to download anything here: `playbook usage` runs this same code.

## What it shows

- Cost and tokens per day, model, repo, branch and session, with a week over week comparison.
- Cache read and write tokens, split into the five minute and one hour tiers.
- Tool, skill and agent breakdowns and latency, when Claude Code telemetry is on (see the receiver below).
- Budget and rate limit panels, when you set budgets or provide limits.
- Codex session history, read the same way.

## Install

`pbtop` is a short alias for `playbook-usage`: the same program, installed alongside it. Anywhere this page says `playbook-usage` you can type `pbtop`.

Download the binary for your platform from the [latest release](https://github.com/pragmatic-engineer/playbook-usage/releases/latest). Releases carry binaries for macOS (arm64 and x86_64), Linux (arm64 and x86_64, static musl) and Windows (x86_64), plus a `SHA256SUMS` file with build provenance attestations.

```sh
sha256sum -c --ignore-missing SHA256SUMS
gh attestation verify playbook-usage-<version>-<target> --repo pragmatic-engineer/playbook-usage
chmod +x playbook-usage-<version>-<target>
mv playbook-usage-<version>-<target> ~/.local/bin/playbook-usage
# optional short alias: download pbtop-<version>-<target> the same way, or
ln -sf playbook-usage ~/.local/bin/pbtop
```

Or build from source with the Rust version in `rust-toolchain.toml`:

```sh
cargo build --release
```

## Use

```sh
playbook-usage                 # terminal view, this week against last week
playbook-usage --summary       # text summary and exit
playbook-usage --json          # the same numbers as JSON
playbook-usage --range 30d     # today, week, last-week, month, last-month, 30d, 60d, 90d, all
playbook-usage status          # is the telemetry receiver running
playbook-usage collect         # run the local OpenTelemetry receiver in the foreground
playbook-usage prune --dry-run # say what old telemetry rows would be removed
```

Days are UTC. Weeks start on Monday unless `usage.weekStart` says otherwise.

It finds Claude Code history in `~/.claude/projects`, or in `$CLAUDE_CONFIG_DIR/projects` when that variable is set.

## Where it keeps its data

State is one SQLite database, `usage.db`, owner-only, in `$XDG_STATE_HOME/playbook-usage` (default `~/.local/state/playbook-usage`). If a database already exists under `~/.config/playbook/usage`, that one is used, so playbook and this tool share one store.

Optional settings go in `$XDG_CONFIG_HOME/playbook-usage/config.json` (default `~/.config/playbook-usage/config.json`), a flat object of dotted keys:

```json
{
  "usage.theme": "default",
  "usage.weekStart": "monday",
  "usage.refreshSeconds": 5,
  "usage.budget.monthlyUsd": 200,
  "usage.budget.weeklyUsd": 50,
  "usage.telemetry": "auto",
  "usage.telemetryRetentionDays": 90,
  "usage.telemetryMaxRows": 500000
}
```

A missing file or key means the default.

## Telemetry receiver

`playbook-usage collect` listens on 127.0.0.1 only and stores API request cost and token rows, tool call timings and API errors from Claude Code's JSON telemetry. Prompts, tool content, paths, emails and user ids are never stored. The receiver checks a bearer token. Its port and token are in `collector.json` in the state folder (owner-only; `playbook-usage status` shows the port and never prints the token). To point a Claude Code session at it, set `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_METRICS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>` and `OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <token>"` for that session. Playbook's `cc` launcher does all of this for you. Without the receiver, every total still comes from the session transcripts.

## Use it as a library

The crate `playbook-telemetry` (library `playbook_telemetry`, published from this repository) exposes the same code the binary runs. A program that embeds it implements the `host::Host` trait (config lookup, state folder, program name, repo layout, sessions panel data) and installs it once with `host::install` before calling `entry::run`. Without an installed host the standalone one described above is used.

```toml
playbook-telemetry = { git = "https://github.com/pragmatic-engineer/playbook-usage", tag = "v0.1.3", package = "playbook-telemetry" }
```

Pin an exact tag. The API is still settling, so expect changes between minor versions until 1.0.

## History

This code was extracted from `src/usage` in [pragmatic-engineer/playbook](https://github.com/pragmatic-engineer/playbook). The repository allows squash merges only, so the earlier history is not copied here. Use `git log` and `git blame` in the playbook repository for anything before the first release; commit subjects there carry the pull request numbers.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::expect_used
cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution rules and [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

Apache-2.0. See [LICENSE](LICENSE).
