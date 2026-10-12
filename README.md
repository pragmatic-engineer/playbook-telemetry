# playbook-telemetry

A token and cost dashboard for Claude Code sessions. It reads the session history Claude Code already keeps on your machine, prices it, and shows it in a terminal view or as a text or JSON report. Everything stays local.

It is part of the [playbook](https://github.com/pragmatic-engineer/playbook) toolkit and also works on its own. If you use playbook, you do not need to download anything here: `playbook usage` runs this same code.

## What it shows

- Cost and tokens per day, model, repo, branch and session, with a week over week comparison.
- Cache read and write tokens, split into the five minute and one hour tiers.
- Tool, skill and agent breakdowns and latency, when Claude Code telemetry is on (see the receiver below).
- Budget and rate limit panels, when you set budgets or provide limits.
- Codex session history, read the same way.

## Install

The tool ships two command names for the same program: `playbook-telemetry` and the short alias `pbtop`. Anywhere this page says `playbook-telemetry` you can type `pbtop`.

With Homebrew (macOS and Linux), which installs both names:

```sh
brew install pragmatic-engineer/tap/playbook-telemetry
```

Or download the archive for your platform from the [latest release](https://github.com/pragmatic-engineer/playbook-telemetry/releases/latest). Releases carry `playbook-telemetry-<version>-<target>.tar.gz` for macOS (arm64 and x86_64), Linux (arm64 and x86_64, static musl) and Windows (x86_64), each holding both `playbook-telemetry` and `pbtop`, plus a `SHA256SUMS` file with build provenance attestations.

```sh
sha256sum -c --ignore-missing SHA256SUMS
gh attestation verify playbook-telemetry-<version>-<target>.tar.gz --repo pragmatic-engineer/playbook-telemetry
tar -xzf playbook-telemetry-<version>-<target>.tar.gz -C ~/.local/bin
```

Or build from source with the Rust version in `rust-toolchain.toml`:

```sh
cargo build --release
```

## Use

```sh
playbook-telemetry                 # terminal view, this week against last week
playbook-telemetry --summary       # text summary and exit
playbook-telemetry --json          # the same numbers as JSON
playbook-telemetry --range 30d     # today, week, last-week, month, last-month, 30d, 60d, 90d, all
playbook-telemetry status          # is the telemetry receiver running
playbook-telemetry collect         # run the local OpenTelemetry receiver in the foreground
playbook-telemetry prune --dry-run # say what old telemetry rows would be removed
```

Days are UTC. Weeks start on Monday unless `usage.weekStart` says otherwise.

It finds Claude Code history in `~/.claude/projects`, or in `$CLAUDE_CONFIG_DIR/projects` when that variable is set.

## Where it keeps its data

The tool shares its data and settings with playbook, so both show the same numbers.

State is one SQLite database, `~/.config/playbook/usage.db`, owner-only. The first run moves an older database from `~/.config/playbook/usage/usage.db` or `~/.local/state/playbook-usage/usage.db` into place and renames the old file to `*.migrated`.

Settings live in `~/.config/playbook/config.json`, in playbook's nested JSON format under the `usage` key. The tool has no config file of its own. Playbook users can also set them with `playbook config set usage.<key> <value>`.

```json
{
  "usage": {
    "theme": "default",
    "weekStart": "monday",
    "refreshSeconds": 5,
    "budget": { "monthlyUsd": 200, "weeklyUsd": 50 },
    "telemetry": "auto",
    "telemetryRetentionDays": 90,
    "telemetryMaxRows": 500000
  }
}
```

A missing file or key means the default.

This shared location is landing in a coming release. Until then, older releases keep state under `~/.local/state/playbook-telemetry` and read settings from `~/.config/playbook-telemetry/config.json`.

## Telemetry receiver

`playbook-telemetry collect` listens on 127.0.0.1 only and stores API request cost and token rows, tool call timings and API errors from Claude Code's JSON telemetry. Prompts, tool content, paths, emails and user ids are never stored. The receiver checks a bearer token. Its port and token are in `collector.json` in the state folder (owner-only; `playbook-telemetry status` shows the port and never prints the token). To point a Claude Code session at it, set `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_METRICS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>` and `OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <token>"` for that session. Playbook's `cc` launcher does all of this for you. Without the receiver, every total still comes from the session transcripts.

## Use it as a library

The crate `playbook-telemetry` (library `playbook_telemetry`, published from this repository) exposes the same code the binary runs. A program that embeds it implements the `host::Host` trait (config lookup, state folder, program name, repo layout, sessions panel data) and installs it once with `host::install` before calling `entry::run`. Without an installed host the standalone one described above is used.

```toml
playbook-telemetry = { git = "https://github.com/pragmatic-engineer/playbook-telemetry", tag = "v0.1.4", package = "playbook-telemetry" }
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
