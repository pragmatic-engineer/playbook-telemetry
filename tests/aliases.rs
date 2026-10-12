// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! `pbtop` is the same program as `playbook-usage` under a shorter name.

use std::process::Command;

fn run(bin: &str, arg: &str) -> String {
    let path = match bin {
        "playbook-usage" => env!("CARGO_BIN_EXE_playbook-usage"),
        _ => env!("CARGO_BIN_EXE_pbtop"),
    };
    let out = Command::new(path).arg(arg).output().expect("spawn");
    assert!(out.status.success(), "{bin} {arg} failed");
    String::from_utf8(out.stdout).expect("utf8")
}

#[test]
fn both_binaries_print_the_same_version_under_their_own_name() {
    let full = run("playbook-usage", "--version");
    let short = run("pbtop", "--version");
    assert_eq!(
        full.trim(),
        format!("playbook-usage {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(short.trim(), format!("pbtop {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_shows_the_invoked_name() {
    assert!(run("pbtop", "--help").contains("Usage: pbtop"));
    assert!(run("playbook-usage", "--help").contains("Usage: playbook-usage"));
}
