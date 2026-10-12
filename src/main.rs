// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! The `playbook-usage` command: a terminal view and reports of Claude Code
//! token and cost usage, read from local session history.

use clap::Parser;
use playbook_usage::command::UsageCommand;
use playbook_usage::entry::{interactive, run, Opts};
use playbook_usage::run::Paths;

/// Show token and cost usage across your Claude Code sessions
///
/// With no subcommand, opens the terminal view, which opens on this week
/// compared with last week. When stdout is not a terminal, or with
/// `--summary`, prints a text summary of estimated cost instead. `--json`
/// prints the same numbers as JSON for scripts. Days are UTC. Ranges: today,
/// week, last-week, month, last-month, 30d, 60d, 90d, all.
///
/// Example: `playbook-usage --range today`
#[derive(Parser, Debug)]
#[command(name = "playbook-usage", version)]
struct Cli {
    #[command(subcommand)]
    sub: Option<UsageCommand>,
    /// Print the totals, groupings and live view as JSON
    #[arg(long, conflicts_with_all = ["summary"])]
    json: bool,
    /// Print the text summary and exit (every recorded session unless `--range` is given)
    #[arg(long, conflicts_with_all = ["json"])]
    summary: bool,
    /// Date range (UTC days): today, week, last-week, month, last-month, 30d, 60d, 90d or all. Default week for the terminal view, 30d for `--json`, all for `--summary`
    #[arg(long, value_name = "RANGE")]
    range: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let paths = Paths::real();
    let opts = Opts {
        json: cli.json,
        summary: cli.summary,
        range: cli.range,
    };
    match run(cli.sub, &opts, &paths, interactive()) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{}", output.trim_end());
            }
        }
        Err(err) => {
            eprintln!("playbook-usage: {err}");
            std::process::exit(1);
        }
    }
}
