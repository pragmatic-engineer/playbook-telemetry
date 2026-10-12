// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! One-time move of older usage databases to the single data file,
//! `~/.config/playbook/usage.db`.
//!
//! Older versions kept it in `~/.config/playbook/usage/usage.db` (playbook) or
//! in `~/.local/state/playbook-telemetry/usage.db` and its older name
//! `playbook-usage` (the standalone tool, also under `$XDG_STATE_HOME`). On
//! open, when the data file is missing, the larger and then newer old database
//! is moved into place with its `-wal` and `-shm` files. Every other old
//! database is merged into the data file in one transaction, keyed on each
//! table's primary key, and then renamed to `*.migrated`. Nothing is deleted.
//! An old file that cannot be read is left alone with a warning. A second run
//! finds nothing to do.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const SIDE_FILES: [&str; 2] = ["-wal", "-shm"];

/// The old locations of the database for the user at `home`.
pub fn legacy_paths(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![home.join(".config/playbook/usage/usage.db")];
    let mut states = Vec::new();
    if home == crate::util::home_dir() {
        if let Some(x) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
            if Path::new(&x).is_absolute() {
                states.push(PathBuf::from(x));
            }
        }
    }
    states.push(home.join(".local/state"));
    for state in states {
        for name in ["playbook-telemetry", "playbook-usage"] {
            out.push(state.join(name).join("usage.db"));
        }
    }
    let mut seen = Vec::new();
    out.retain(|p| {
        let fresh = !seen.contains(p);
        seen.push(p.clone());
        fresh
    });
    out
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn size_and_time(path: &Path) -> (u64, SystemTime) {
    std::fs::metadata(path).map_or((0, SystemTime::UNIX_EPOCH), |m| {
        (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH))
    })
}

/// Moves `from` (and its side files) to `to`.
fn move_db(from: &Path, to: &Path) -> Result<(), String> {
    let err =
        |e: std::io::Error| format!("cannot move {} to {}: {e}", from.display(), to.display());
    std::fs::rename(from, to).map_err(err)?;
    for suffix in SIDE_FILES {
        let side = with_suffix(from, suffix);
        if side.exists() {
            std::fs::rename(&side, with_suffix(to, suffix)).map_err(err)?;
        }
    }
    Ok(())
}

/// Renames `db` and its side files to `*.migrated`.
fn retire(db: &Path) -> Result<(), String> {
    move_db(db, &with_suffix(db, ".migrated"))
}

fn tables(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM main.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA {schema}.table_info(\"{table}\")"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Copies every row of `old` that the data file lacks, by primary key.
fn merge(target: &Path, old: &Path) -> Result<(), String> {
    // Opening the old file through the normal opener brings an older schema up
    // to date, and fails on a file that is not a database.
    let probe = crate::db::open_db(old)?;
    let check: String = probe
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if check != "ok" {
        return Err(format!("integrity check says {check}"));
    }
    drop(probe);
    let conn = crate::db::open_db(target)?;
    conn.execute(
        "ATTACH DATABASE ?1 AS legacy",
        [old.to_string_lossy().as_ref()],
    )
    .map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        for table in tables(&conn)? {
            let have = columns(&conn, "legacy", &table)?;
            let cols: Vec<String> = columns(&conn, "main", &table)?
                .into_iter()
                .filter(|c| have.contains(c))
                .map(|c| format!("\"{c}\""))
                .collect();
            if cols.is_empty() {
                continue;
            }
            let list = cols.join(", ");
            conn.execute(
                &format!(
                    "INSERT OR IGNORE INTO main.\"{table}\" ({list}) SELECT {list} FROM legacy.\"{table}\""
                ),
                [],
            )
            .map_err(|e| format!("{table}: {e}"))?;
        }
        conn.execute_batch("COMMIT").map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    let _ = conn.execute_batch("DETACH DATABASE legacy");
    result
}

/// Runs the migration into `target` from `sources`. Returns the lines to show
/// the user (one notice when something moved, plus any warnings).
pub fn run(target: &Path, sources: &[PathBuf]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut found: Vec<&PathBuf> = sources.iter().filter(|p| p.is_file()).collect();
    if found.is_empty() {
        return lines;
    }
    let Some(dir) = target.parent() else {
        return lines;
    };
    if let Err(e) = crate::util::atomic::ensure_private_dir(dir) {
        return vec![format!("cannot prepare {}: {e}", dir.display())];
    }
    // One process migrates at a time; the others carry on and find it done.
    let lock = dir.join("usage-migrate.lock");
    if let Ok(age) = std::fs::metadata(&lock)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed())
    {
        if age.is_ok_and(|a| a > Duration::from_secs(120)) {
            let _ = std::fs::remove_dir(&lock);
        }
    }
    if std::fs::create_dir(&lock).is_err() {
        return lines;
    }
    let mut moved = Vec::new();
    if !target.exists() {
        found.sort_by_key(|p| std::cmp::Reverse(size_and_time(p)));
        // A file that cannot be read is skipped, so try the candidates in turn.
        let mut chosen = None;
        for (i, path) in found.iter().enumerate() {
            match crate::db::open_db(path).and_then(|c| {
                c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())
            }) {
                Ok(ok) if ok == "ok" => {
                    chosen = Some(i);
                    break;
                }
                Ok(bad) => lines.push(format!(
                    "skipping {}: integrity check says {bad}",
                    path.display()
                )),
                Err(e) => lines.push(format!("skipping {}: {e}", path.display())),
            }
        }
        if let Some(i) = chosen {
            let path = found.remove(i);
            match move_db(path, target) {
                Ok(()) => moved.push(format!("moved {} to {}", path.display(), target.display())),
                Err(e) => lines.push(e),
            }
        }
    }
    if target.exists() {
        for path in found {
            if !path.is_file() {
                continue;
            }
            match merge(target, path) {
                Ok(()) => match retire(path) {
                    Ok(()) => moved.push(format!("merged {}", path.display())),
                    Err(e) => lines.push(e),
                },
                Err(e) => lines.push(format!("skipping {}: {e}", path.display())),
            }
        }
    }
    let _ = std::fs::remove_dir(&lock);
    if !moved.is_empty() {
        lines.insert(0, format!("usage data: {}", moved.join("; ")));
    }
    lines
}

/// Migrates into the standard data file of the real environment and prints the
/// result to stderr.
pub fn run_standard(target: &Path) {
    let home = crate::host::home_dir();
    for line in run(target, &legacy_paths(&home)) {
        eprintln!("{}: {line}", crate::host::program());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::scratch_dir;
    use rusqlite::params;

    fn make(path: &Path, ids: &[&str]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let conn = crate::db::open_db(path).unwrap();
        for id in ids {
            conn.execute(
                "INSERT OR IGNORE INTO usage_events (event_id, timestamp, session_id, account, model, effort, repo, branch, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, cost_usd) VALUES (?1, 1, 's', 'a', 'm', '', 'r', 'b', 1, 1, 0, 0, 0.5)",
                params![id],
            )
            .unwrap();
        }
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
    }

    fn ids(path: &Path) -> Vec<String> {
        let conn = crate::db::open_db(path).unwrap();
        let mut stmt = conn
            .prepare("SELECT event_id FROM usage_events ORDER BY event_id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    struct Layout {
        target: PathBuf,
        playbook_old: PathBuf,
        standalone_old: PathBuf,
        sources: Vec<PathBuf>,
    }

    fn layout(tag: &str) -> Layout {
        let home = scratch_dir(tag);
        let playbook_old = home.join(".config/playbook/usage/usage.db");
        let standalone_old = home.join(".local/state/playbook-telemetry/usage.db");
        Layout {
            target: home.join(".config/playbook/usage.db"),
            sources: vec![playbook_old.clone(), standalone_old.clone()],
            playbook_old,
            standalone_old,
        }
    }

    #[test]
    fn neither_old_file_means_nothing_happens() {
        let l = layout("mig-neither");
        assert!(run(&l.target, &l.sources).is_empty());
        assert!(!l.target.exists());
    }

    #[test]
    fn only_the_old_playbook_file_moves_up_with_its_side_files() {
        let l = layout("mig-pb");
        make(&l.playbook_old, &["a", "b"]);
        std::fs::write(with_suffix(&l.playbook_old, "-shm"), b"").unwrap();
        let lines = run(&l.target, &l.sources);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(ids(&l.target), ["a", "b"]);
        assert!(!l.playbook_old.exists());
        assert!(
            with_suffix(&l.target, "-shm").exists()
                || !with_suffix(&l.playbook_old, "-shm").exists()
        );
    }

    #[test]
    fn only_the_old_standalone_file_moves_into_place() {
        let l = layout("mig-sa");
        make(&l.standalone_old, &["x"]);
        run(&l.target, &l.sources);
        assert_eq!(ids(&l.target), ["x"]);
        assert!(!l.standalone_old.exists());
    }

    #[test]
    fn both_old_files_end_as_one_union_and_the_smaller_is_renamed() {
        let l = layout("mig-both");
        make(&l.playbook_old, &["a", "b", "c"]);
        make(&l.standalone_old, &["c", "d"]);
        run(&l.target, &l.sources);
        assert_eq!(ids(&l.target), ["a", "b", "c", "d"]);
        assert!(!l.playbook_old.exists() && !l.standalone_old.exists());
        let kept = [
            with_suffix(&l.playbook_old, ".migrated"),
            with_suffix(&l.standalone_old, ".migrated"),
        ];
        assert_eq!(kept.iter().filter(|p| p.exists()).count(), 1, "{kept:?}");
    }

    #[test]
    fn an_existing_data_file_gets_the_old_rows_merged() {
        let l = layout("mig-existing");
        make(&l.target, &["n"]);
        make(&l.standalone_old, &["n", "o"]);
        run(&l.target, &l.sources);
        assert_eq!(ids(&l.target), ["n", "o"]);
        assert!(with_suffix(&l.standalone_old, ".migrated").exists());
    }

    #[test]
    fn a_second_run_does_nothing() {
        let l = layout("mig-twice");
        make(&l.playbook_old, &["a"]);
        make(&l.standalone_old, &["b"]);
        run(&l.target, &l.sources);
        let before = ids(&l.target);
        assert!(run(&l.target, &l.sources).is_empty());
        assert_eq!(ids(&l.target), before);
    }

    #[test]
    fn a_corrupt_old_file_is_skipped_with_a_warning_and_left_alone() {
        let l = layout("mig-corrupt");
        std::fs::create_dir_all(l.standalone_old.parent().unwrap()).unwrap();
        std::fs::write(&l.standalone_old, b"this is not a database, not even close").unwrap();
        make(&l.playbook_old, &["a"]);
        let lines = run(&l.target, &l.sources);
        assert!(lines.iter().any(|m| m.contains("skipping")), "{lines:?}");
        assert_eq!(ids(&l.target), ["a"]);
        assert!(l.standalone_old.exists());
    }

    #[test]
    fn only_a_corrupt_old_file_leaves_no_data_file() {
        let l = layout("mig-only-corrupt");
        std::fs::create_dir_all(l.playbook_old.parent().unwrap()).unwrap();
        std::fs::write(&l.playbook_old, b"garbage garbage garbage garbage").unwrap();
        let lines = run(&l.target, &l.sources);
        assert!(lines.iter().any(|m| m.contains("skipping")), "{lines:?}");
        assert!(!l.target.exists());
        assert!(l.playbook_old.exists());
    }

    #[test]
    fn two_connections_can_write_the_data_file_at_once() {
        let l = layout("mig-concurrent");
        make(&l.target, &["a"]);
        let t = l.target.clone();
        let writer = std::thread::spawn(move || {
            let c = crate::db::open_db(&t).unwrap();
            for i in 0..50 {
                c.execute(
                    "INSERT OR IGNORE INTO usage_events (event_id, timestamp, session_id, account, model, effort, repo, branch, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, cost_usd) VALUES (?1, 1, 's', 'a', 'm', '', 'r', 'b', 1, 1, 0, 0, 0.5)",
                    params![format!("w{i}")],
                )
                .unwrap();
            }
        });
        let reader = crate::db::open_db(&l.target).unwrap();
        for _ in 0..50 {
            let n: i64 = reader
                .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
                .unwrap();
            assert!(n >= 1);
        }
        writer.join().unwrap();
        assert_eq!(ids(&l.target).len(), 51);
        let mode: String = reader
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
