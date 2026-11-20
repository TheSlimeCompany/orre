//! Which commands need the user at a terminal (R.1).
//!
//! An AI client with a shell could file a suggestion over MCP and approve it
//! itself with `orre inbox approve`. Commands that act as the user
//! therefore refuse to run unless stdin is an interactive terminal. There is
//! no flag or environment variable to skip the check: an agent would use it.
//!
//! The check gates the write, not the preview. A command that only shows
//! what it would do (`import --dry-run`, `undo-import`, `schedules delete`,
//! `inbox delete-report` and `backup --delete` without `--yes`) changes none of the user's
//! data, so it doesn't act as the user and needs no terminal; an agent can
//! usefully preview. The flag that commits it needs one.
//!
//! This is a speed bump, not a boundary. A process that can fake a terminal
//! (`script`) or open the SQLite file gets past it; keeping that process
//! away from the data is the client's sandbox's job.
//!
//! `needs_terminal` classifies every command in one place; [`Terminal`] is
//! what the check runs against, so tests can stand in for both cases.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::bail;
use orre_core::mcp_client;

use super::ai_access::LevelArgs;
use super::budgets::{BudgetsArgs, BudgetsCommand};
use super::{
    AccountsArgs, AccountsCommand, AiAccessArgs, AiAccessCommand, Command, InboxArgs, InboxCommand,
    SchedulesCommand, open_existing,
};

/// Whether a person is at the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// Stdin is an interactive terminal.
    Interactive,
    /// Stdin is a pipe, a file or closed: a script or an AI client's shell
    /// tool.
    NotInteractive,
}

impl Terminal {
    /// What the running process has: whether stdin is a terminal.
    pub fn detect() -> Terminal {
        if std::io::stdin().is_terminal() {
            Terminal::Interactive
        } else {
            Terminal::NotInteractive
        }
    }
}

/// Refuses `command` when it needs a terminal and `terminal` isn't one.
///
/// The database is read only for `ai-access level`, to tell raising a
/// client's level from lowering it, and only when there is no terminal.
pub fn require_terminal_for(
    command: &Command,
    database: &Path,
    terminal: Terminal,
) -> anyhow::Result<()> {
    if terminal == Terminal::Interactive {
        return Ok(());
    }
    if let Some(name) = needs_terminal(command, database)? {
        bail!(
            "`orre {name}` needs an interactive terminal: it acts as you, which is yours to \
             do, not an AI client's. Run it yourself in a terminal, or use the app."
        );
    }
    Ok(())
}

/// The name of `command`, as the user types it, when it acts as the user
/// (approves what an AI client filed, widens AI access, or writes
/// transactions, rules, categories, accounts, budgets or schedules); `None`
/// for reading, for lowering or switching off AI access, for backing up and
/// exporting, for `mcp`, for rejecting a suggestion, and for a preview that
/// writes nothing: the preview doesn't act as the user, the commit does.
///
/// A preview is exempt only where the command's `run` reads and prints
/// first, and a flag alone separates that from the write: `import
/// --dry-run`, `undo-import`, `schedules delete` and `inbox delete-report`
/// without `--yes`. `restore` without `--yes` stays gated: its check stages
/// a copy of the backup in the data directory.
fn needs_terminal(command: &Command, database: &Path) -> anyhow::Result<Option<&'static str>> {
    Ok(match command {
        Command::Import(args) => (!args.dry_run).then_some("import"),
        Command::UndoImport(args) => args.yes.then_some("undo-import"),
        Command::Restore(_) => Some("restore"),
        Command::Categorize(_) => Some("categorize"),
        Command::Backup(args) => (args.delete.is_some() && args.yes).then_some("backup --delete"),
        Command::Export(_) | Command::Mcp(_) => None,
        Command::Accounts(AccountsArgs { command }) => match command {
            None | Some(AccountsCommand::List) => None,
            Some(AccountsCommand::Add(_)) => Some("accounts add"),
            Some(AccountsCommand::Close(_)) => Some("accounts close"),
            Some(AccountsCommand::Reopen(_)) => Some("accounts reopen"),
        },
        Command::AiAccess(AiAccessArgs { command }) => match command {
            None
            | Some(
                AiAccessCommand::List
                | AiAccessCommand::Revoke(_)
                | AiAccessCommand::Ignore(_)
                | AiAccessCommand::Off
                | AiAccessCommand::Log(_),
            ) => None,
            Some(AiAccessCommand::Enable(_)) => Some("ai-access enable"),
            Some(AiAccessCommand::On) => Some("ai-access on"),
            Some(AiAccessCommand::Level(args)) => {
                raises_level(database, args)?.then_some("ai-access level")
            }
        },
        Command::Budgets(BudgetsArgs { command }) => match command {
            None | Some(BudgetsCommand::List) => None,
            Some(BudgetsCommand::Set(_)) => Some("budgets set"),
            Some(BudgetsCommand::Remove(_)) => Some("budgets remove"),
        },
        Command::Schedules(args) => schedules_name(&args.command),
        Command::Inbox(InboxArgs { command }) => match command {
            None
            | Some(
                InboxCommand::List(_)
                | InboxCommand::Show(_)
                | InboxCommand::Reject(_)
                | InboxCommand::Reports
                | InboxCommand::Report(_),
            ) => None,
            Some(InboxCommand::Approve(_)) => Some("inbox approve"),
            Some(InboxCommand::DeleteReport(args)) => args.yes.then_some("inbox delete-report"),
        },
    })
}

fn schedules_name(command: &SchedulesCommand) -> Option<&'static str> {
    use SchedulesCommand as S;
    match command {
        S::Detect | S::List | S::Show(_) => None,
        S::Confirm(_) => Some("schedules confirm"),
        S::Skip(_) => Some("schedules skip"),
        S::Unskip(_) => Some("schedules unskip"),
        S::Match(_) => Some("schedules match"),
        S::Unmatch(_) => Some("schedules unmatch"),
        S::Pause(_) => Some("schedules pause"),
        S::Resume(_) => Some("schedules resume"),
        S::End(_) => Some("schedules end"),
        S::Delete(args) => args.yes.then_some("schedules delete"),
        S::Dismiss(_) => Some("schedules dismiss"),
        S::Undismiss(_) => Some("schedules undismiss"),
    }
}

/// Whether `ai-access level` gives the client a higher level than it has. A
/// client that isn't enabled counts as raised: the command fails for it
/// anyway, and a missing client is never a reason to skip the check.
fn raises_level(database: &Path, args: &LevelArgs) -> anyhow::Result<bool> {
    let db = open_existing(database)?;
    let clients = mcp_client::list(&db.reader()?)?;
    let current = clients
        .iter()
        .find(|client| client.name == args.name && client.enabled)
        .map(|client| client.level);
    Ok(current.is_none_or(|current| args.level > current))
}
