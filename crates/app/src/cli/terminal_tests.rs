//! The terminal check (R.1): which commands refuse to run without an
//! interactive terminal, and that the others don't. Scratch data dirs only.

use std::path::{Path, PathBuf};

use clap::Parser;
use orre_core::db::Database;
use orre_core::mcp_client::{self, AccessLevel, ClientName};
use orre_core::setting;

use super::terminal::require_terminal_for;
use super::*;

const KEY: &str = "0123456789ab";

/// Commands that act as the user: refused without a terminal.
const GATED: &[&[&str]] = &[
    &["import", "a.ofx"],
    &["import", "a.ofx", "--yes"],
    &[
        "accounts",
        "add",
        "--name",
        "A",
        "--kind",
        "current",
        "--currency",
        "GBP",
    ],
    &["accounts", "close", "1"],
    &["accounts", "reopen", "1"],
    &["undo-import", "1", "--yes"],
    &["undo-import", "1", "-y"],
    &["restore", "a.db", "--yes"],
    &["backup", "--delete", "a.db", "--yes"],
    &["categorize", "1", "food.groceries"],
    &["ai-access", "enable", "claude-code"],
    &["ai-access", "on"],
    &["budgets", "set", "food.groceries", "250"],
    &["budgets", "remove", "food.groceries"],
    &["schedules", "confirm", KEY],
    &["schedules", "skip", "1", "2026-11-01"],
    &["schedules", "unskip", "1", "2026-11-01"],
    &["schedules", "match", "1", "2026-11-01", "5"],
    &["schedules", "unmatch", "1", "2026-11-01"],
    &["schedules", "pause", "1"],
    &["schedules", "resume", "1"],
    &["schedules", "end", "1"],
    &["schedules", "delete", "1", "--yes"],
    &["schedules", "dismiss", KEY],
    &["schedules", "undismiss", KEY],
    &["inbox", "approve", "1"],
    &["inbox", "approve", "1", "--apply-to-history"],
    &["inbox", "delete-report", "1", "--yes"],
    &["inbox", "delete-report", "--client", "claude-code", "--yes"],
];

/// Previews: they show what a gated command would do and write nothing, so
/// they need no terminal; the flag that commits is in `GATED`.
const PREVIEWS: &[&[&str]] = &[
    &["import", "a.ofx", "--dry-run"],
    &["import", "a.ofx", "--dry-run", "--account", "1"],
    &["undo-import", "1"],
    &["backup", "--delete", "a.db"],
    &["schedules", "delete", "1"],
    &["inbox", "delete-report", "1"],
    &["inbox", "delete-report", "--client", "claude-code"],
];

/// Commands that read, back up, export, contain or only decline: no
/// terminal needed.
const UNGATED: &[&[&str]] = &[
    &["accounts"],
    &["accounts", "list"],
    &["backup"],
    &["backup", "--list"],
    &["export", "--format", "csv"],
    &["mcp", "--client", "claude-code"],
    &["ai-access"],
    &["ai-access", "list"],
    &["ai-access", "revoke", "claude-code"],
    &["ai-access", "ignore", "claude-code"],
    &["ai-access", "off"],
    &["ai-access", "log"],
    &["budgets"],
    &["budgets", "list"],
    &["schedules", "detect"],
    &["schedules", "list"],
    &["schedules", "show", "1"],
    &["inbox"],
    &["inbox", "list"],
    &["inbox", "show", "1"],
    &["inbox", "reject", "1"],
    &["inbox", "reports"],
    &["inbox", "report", "1"],
];

fn command(args: &[&str]) -> Command {
    Cli::try_parse_from(std::iter::once("orre").chain(args.iter().copied()))
        .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"))
        .command
        .expect("a subcommand")
}

fn name(client: &str) -> ClientName {
    ClientName::new(client).unwrap()
}

/// A path no command here ever opens: the check comes first.
fn no_database() -> PathBuf {
    PathBuf::from("/nonexistent/orre.db")
}

#[test]
fn every_gated_command_is_refused_without_a_terminal() {
    for args in GATED {
        let error = require_terminal_for(&command(args), &no_database(), Terminal::NotInteractive)
            .expect_err(&format!("{args:?} wasn't refused"));
        let message = format!("{error:#}");
        assert!(
            message.contains("needs an interactive terminal"),
            "{args:?}: {message}"
        );
        // The refusal names the command the user typed.
        let typed = args
            .iter()
            .take_while(|arg| !arg.starts_with('-'))
            .take(
                if matches!(
                    args[0],
                    "accounts" | "ai-access" | "budgets" | "schedules" | "inbox"
                ) {
                    2
                } else {
                    1
                },
            )
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        // `backup` is gated by its flag, which the refusal names too.
        let typed = if typed == "backup" {
            "backup --delete".to_owned()
        } else {
            typed
        };
        assert!(
            message.contains(&format!("`orre {typed}`")),
            "{args:?}: {message}"
        );
    }
}

#[test]
fn every_gated_command_is_allowed_with_a_terminal() {
    for args in GATED {
        require_terminal_for(&command(args), &no_database(), Terminal::Interactive)
            .unwrap_or_else(|error| panic!("{args:?} refused: {error:#}"));
    }
}

#[test]
fn ungated_commands_need_no_terminal() {
    for args in UNGATED {
        require_terminal_for(&command(args), &no_database(), Terminal::NotInteractive)
            .unwrap_or_else(|error| panic!("{args:?} refused: {error:#}"));
    }
}

#[test]
fn previews_need_no_terminal_but_the_commit_does() {
    for args in PREVIEWS {
        require_terminal_for(&command(args), &no_database(), Terminal::NotInteractive)
            .unwrap_or_else(|error| panic!("{args:?} refused: {error:#}"));
    }
    // `restore` has no exemption: its check stages a copy in the data dir.
    require_terminal_for(
        &command(&["restore", "a.db"]),
        &no_database(),
        Terminal::NotInteractive,
    )
    .unwrap_err();
}

/// A scratch database where `claude-code` is enabled at `pseudonymous`.
fn database_with_client() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orre.db");
    let mut db = Database::open(&path).unwrap();
    mcp_client::enable(&mut db, &name("claude-code")).unwrap();
    mcp_client::set_level(&mut db, &name("claude-code"), AccessLevel::Pseudonymous).unwrap();
    (dir, path)
}

fn level_allowed(database: &Path, level: &str, name: &str) -> bool {
    require_terminal_for(
        &command(&["ai-access", "level", name, level]),
        database,
        Terminal::NotInteractive,
    )
    .is_ok()
}

#[test]
fn lowering_a_level_needs_no_terminal_but_raising_it_does() {
    let (_dir, path) = database_with_client();
    assert!(level_allowed(&path, "aggregate", "claude-code"));
    assert!(level_allowed(&path, "pseudonymous", "claude-code"));
    assert!(!level_allowed(&path, "standard", "claude-code"));
    // Not enabled, or not known: refused rather than waved through.
    assert!(!level_allowed(&path, "aggregate", "codex"));
}

#[test]
fn refused_commands_change_nothing_and_ungated_ones_run() {
    let (_dir, path) = database_with_client();
    let mut out = Vec::new();

    let error = run_with(
        command(&["ai-access", "enable", "codex"]),
        &path,
        &mut out,
        Terminal::NotInteractive,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("`orre ai-access enable`"));
    assert!(out.is_empty());
    let db = Database::open(&path).unwrap();
    let clients = mcp_client::list(&db.reader().unwrap()).unwrap();
    assert!(clients.iter().all(|client| client.name != name("codex")));

    // Switching AI access off is how a user contains things: no terminal.
    run_with(
        command(&["ai-access", "off"]),
        &path,
        &mut out,
        Terminal::NotInteractive,
    )
    .unwrap();
    assert!(!setting::mcp_enabled(&db.reader().unwrap()).unwrap());

    // Switching it on again is not.
    run_with(
        command(&["ai-access", "on"]),
        &path,
        &mut Vec::new(),
        Terminal::NotInteractive,
    )
    .unwrap_err();
    assert!(!setting::mcp_enabled(&db.reader().unwrap()).unwrap());
    run_with(
        command(&["ai-access", "on"]),
        &path,
        &mut Vec::new(),
        Terminal::Interactive,
    )
    .unwrap();
    assert!(setting::mcp_enabled(&db.reader().unwrap()).unwrap());

    // A read-only command runs without a terminal.
    let mut out = Vec::new();
    run_with(
        command(&["ai-access", "list"]),
        &path,
        &mut out,
        Terminal::NotInteractive,
    )
    .unwrap();
    assert!(String::from_utf8(out).unwrap().contains("claude-code"));
}
