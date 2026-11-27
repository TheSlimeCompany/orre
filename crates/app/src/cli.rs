//! The `orre` command line (M1.18): argument parsing and the headless
//! commands.
//!
//! `orre` with no subcommand opens the GUI (`src/main.rs`); every
//! subcommand here runs without a window, so none of this touches GPUI.
//! Each command takes the database (or its path) and a writer for its
//! output, so the tests run them against a scratch directory and read what
//! they print.
//!
//! - `orre import <file>`: detect the file's importer, preview the import
//!   into an account, and commit it ([`import`]).
//! - `orre accounts [list]`, `orre accounts add`: the accounts an import
//!   goes into; `accounts close` and `reopen` (M3.17) ([`accounts`]).
//! - `orre undo-import <batch>`: remove an import again ([`undo`]).
//! - `orre backup [--list]`, `orre restore <file>`: local backups
//!   ([`backup`]).
//! - `orre export --format csv|json`: the ledger as a file ([`export`]).
//! - `orre ai-access list|enable|level|revoke|ignore|off|on|log`: which AI
//!   clients may use the MCP server, at which level ([`ai_access`]).
//! - `orre budgets [list|set|remove]`: monthly budgets per category
//!   ([`budgets`]).
//! - `orre mcp --client <name>`: the MCP server over stdio ([`mcp`]). It
//!   ignores the output writer: stdout carries MCP messages only.
//! - `orre schedules detect|list|show|confirm|…`: recurring payments and
//!   schedules ([`schedules`]).
//! - `orre inbox [list|show|approve|reject|reports|report|delete-report]`:
//!   review AI clients' suggestions and decide them, and read and delete
//!   their saved reports ([`inbox`]).
//! - `orre categorize <ID>... <CATEGORY>`: set transactions' category as
//!   the user, as one change batch ([`categorize`]).
//!
//! Commands that act as the user refuse to run without an interactive
//! terminal ([`terminal`], R.1); [`run_with`] applies that in one place.
//!
//! Output is what the user asked for and nothing else. Errors are returned
//! (as `anyhow` errors: this is the binary's layer) for `main` to print on
//! stderr. Nothing is logged; transaction text never reaches a log.

pub mod accounts;
pub mod ai_access;
pub mod backup;
pub mod budgets;
pub mod categorize;
pub mod export;
pub mod import;
pub mod inbox;
pub mod mcp;
pub mod schedules;
pub mod terminal;
pub mod undo;

#[cfg(test)]
mod ai_access_tests;
#[cfg(test)]
mod backup_tests;
#[cfg(test)]
mod budgets_tests;
#[cfg(test)]
mod categorize_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod schedules_tests;
#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use orre_core::db::Database;
use orre_import::preset::Presets;

pub use accounts::{
    AccountsArgs, AccountsCommand, AddAccountArgs, CloseAccountArgs, ReopenAccountArgs,
};
pub use ai_access::{AiAccessArgs, AiAccessCommand};
pub use backup::{BackupArgs, RestoreArgs};
pub use categorize::CategorizeArgs;
pub use export::{ExportArgs, ExportFormat};
pub use import::ImportArgs;
pub use inbox::{
    ApproveArgs, DeleteReportArgs, InboxArgs, InboxCommand, ListArgs, RejectArgs, ReportArgs,
    ShowArgs,
};
pub use mcp::McpArgs;
pub use schedules::{SchedulesArgs, SchedulesCommand};
pub use terminal::Terminal;
pub use undo::UndoImportArgs;

/// Orre: a local-first personal finance dashboard.
///
/// With no command, opens the app.
#[derive(Debug, Parser)]
#[command(name = "orre", version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Open the component showcase (a development tool) instead of the app.
    /// Only in tests and `dev-samples` builds: a release `orre` has no such
    /// flag (W1-3).
    #[cfg(any(test, feature = "dev-samples"))]
    #[arg(long)]
    pub showcase: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The headless commands. None of them opens a window.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Import a bank export (CSV with a bundled preset, OFX or QFX) into an
    /// account.
    Import(ImportArgs),
    /// List the accounts, add one, or close or reopen one.
    Accounts(AccountsArgs),
    /// Undo an import: remove its transactions, balances and flags.
    UndoImport(UndoImportArgs),
    /// Back up the database now (kept until you delete it), or list the
    /// backups.
    Backup(BackupArgs),
    /// Restore a backup over the database, after backing up the current one.
    Restore(RestoreArgs),
    /// Export transactions (split lines as rows) as CSV or JSON. Notes and
    /// private tags only when asked for.
    Export(ExportArgs),
    /// Run the MCP server over stdin/stdout for an AI client (Claude Code,
    /// Codex, …). Needs an existing database.
    Mcp(McpArgs),
    /// Which AI clients may use the MCP server and at which level, the
    /// kill switch, and the AI access log.
    AiAccess(AiAccessArgs),
    /// List, set or remove budgets: what you mean to spend in a category
    /// each month.
    Budgets(budgets::BudgetsArgs),
    /// Find recurring payments, confirm them as schedules, and manage
    /// schedules.
    Schedules(SchedulesArgs),
    /// Review the suggestions AI clients filed, and approve or reject them.
    /// Lists the pending ones when given no command.
    Inbox(InboxArgs),
    /// Set the category of transactions, as your own choice, recorded as
    /// one change batch: `orre categorize 12 13 food.groceries`.
    Categorize(CategorizeArgs),
    /// Drive the running dev build's window (ROADMAP U.5): `describe`,
    /// `show <view>`, `press <keys…>`, `type <text>`, `click <id>`, `run
    /// <label>`. Dev builds only, against a scratch ORRE_DATA_DIR.
    #[cfg(feature = "dev-driver")]
    Dev(DevArgs),
}

/// `orre dev`'s command and its arguments, sent as one line.
#[cfg(feature = "dev-driver")]
#[derive(Debug, clap::Args)]
pub struct DevArgs {
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// The bundled presets and the ones the user saved from the import screen
/// (`<data dir>/presets`). A saved preset that can't be used is left out
/// with a warning in the log; its name and path are the user's, not
/// transaction data.
fn presets(database: &Path) -> anyhow::Result<Presets> {
    let dir = orre_core::paths::presets_dir(database);
    let (presets, errors) =
        orre_import::preset::load_user_presets(&dir).context("the bundled presets don't load")?;
    for error in errors {
        tracing::warn!(%error, "a saved preset can't be used");
    }
    Ok(presets)
}

/// Runs `command` as [`run_with`] does, with no terminal check: for callers
/// that stand in for the user (the tests). The binary uses [`run_with`].
pub fn run(command: Command, database: &Path, out: &mut dyn Write) -> anyhow::Result<()> {
    run_with(command, database, out, Terminal::Interactive)
}

/// Runs `command` against the database at `database` (the data dir's
/// `orre.db` in the binary), writing its output to `out`.
///
/// A command that acts as the user is refused unless `terminal` is
/// interactive ([`terminal::require_terminal_for`]); the binary passes
/// [`Terminal::detect`].
///
/// `accounts add` and `import` create the database if there is none;
/// `restore` creates it from the backup; `backup --list` reads only the
/// backups directory. The other commands need the database to exist
/// already; `mcp` also needs it up to date and never migrates it.
pub fn run_with(
    command: Command,
    database: &Path,
    out: &mut dyn Write,
    terminal: Terminal,
) -> anyhow::Result<()> {
    terminal::require_terminal_for(&command, database, terminal)?;
    match command {
        Command::Import(args) => {
            let presets = presets(database)?;
            // A dry run writes nothing, so it doesn't create the database
            // either; it needs no terminal (`terminal`).
            let mut db = if args.dry_run {
                open_existing(database)?
            } else {
                open_or_create(database)?
            };
            import::run(&mut db, &presets, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: Some(AccountsCommand::Add(args)),
        }) => {
            let presets = presets(database)?;
            let mut db = open_or_create(database)?;
            accounts::add(&mut db, &presets, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: Some(AccountsCommand::Close(args)),
        }) => {
            let mut db = open_existing(database)?;
            accounts::close(&mut db, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: Some(AccountsCommand::Reopen(args)),
        }) => {
            let mut db = open_existing(database)?;
            accounts::reopen(&mut db, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: None | Some(AccountsCommand::List),
        }) => {
            let db = open_existing(database)?;
            accounts::list(&db, out)
        }
        Command::UndoImport(args) => {
            let mut db = open_existing(database)?;
            undo::run(&mut db, &args, out)
        }
        Command::Backup(args) => backup::run(database, &args, out),
        Command::Restore(args) => backup::restore(database, &args, out),
        Command::Export(args) => {
            let db = open_existing(database)?;
            export::run(&db, &args, out)
        }
        Command::Mcp(args) => mcp::run(database, &args),
        Command::AiAccess(args) => ai_access::run(database, args, out),
        Command::Budgets(args) => budgets::run(database, args, out),
        Command::Schedules(args) => {
            let mut db = open_existing(database)?;
            schedules::run(&mut db, &args, out)
        }
        Command::Inbox(args) => {
            let mut db = open_existing(database)?;
            inbox::run(&mut db, args, out)
        }
        Command::Categorize(args) => {
            let mut db = open_existing(database)?;
            categorize::run(&mut db, &args, out)
        }
        // Talks only to the dev build's socket; the database isn't opened.
        #[cfg(feature = "dev-driver")]
        Command::Dev(args) => {
            let reply = crate::dev_driver::client::send(&args.command)?;
            writeln!(out, "{reply}")?;
            Ok(())
        }
    }
}

/// Opens the database, creating it (and the data dir) if needed.
fn open_or_create(path: &Path) -> anyhow::Result<Database> {
    Database::open(path).with_context(|| format!("couldn't open the database {}", path.display()))
}

/// Opens the database only if it's there, so a read-only command never
/// leaves an empty database behind. Pending migrations are applied: the CLI,
/// like the app, keeps the schema current.
fn open_existing(path: &Path) -> anyhow::Result<Database> {
    if !path.try_exists().unwrap_or(true) {
        bail!(
            "there is no Orre database at {} yet; add an account with \
             `orre accounts add` to create it (or set ORRE_DATA_DIR to use another data directory)",
            path.display()
        );
    }
    open_or_create(path)
}

/// Writes rows under a header, each column padded to its widest cell.
fn write_table(out: &mut dyn Write, header: &[&str], rows: &[Vec<String>]) -> std::io::Result<()> {
    let mut widths: Vec<usize> = header.iter().map(|cell| cell.chars().count()).collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let header: Vec<String> = header.iter().map(|cell| (*cell).to_owned()).collect();
    for row in std::iter::once(&header).chain(rows) {
        let mut line = String::new();
        for (index, (cell, width)) in row.iter().zip(&widths).enumerate() {
            if index > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            if index + 1 < row.len() {
                line.extend(std::iter::repeat_n(' ', width - cell.chars().count()));
            }
        }
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}
