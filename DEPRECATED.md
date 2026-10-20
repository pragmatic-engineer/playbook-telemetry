# Deprecated

This repository is archived and `playbook-telemetry` is deprecated. The usage dashboard now ships as `pbtop`, and inside playbook as `playbook usage`. Both run the same code, which lives in the [playbook repository](https://github.com/pragmatic-engineer/playbook/tree/main/crates/telemetry).

## What to do

- Installed with Homebrew: `brew uninstall playbook-telemetry && brew install pbtop`
- Want the dashboard alone: `curl -fsSL https://raw.githubusercontent.com/pragmatic-engineer/playbook/main/install-pbtop.sh | sh`, or `brew install pragmatic-engineer/tap/pbtop`
- Want playbook too: `brew install pragmatic-engineer/tap/playbook`, then run `playbook usage`
- A script that calls `playbook-telemetry`: call `pbtop` with the same arguments.

The Homebrew formula `playbook-telemetry` is disabled from 2026-11-16. Copies already installed keep working. Please open issues and pull requests in the playbook repository.
