//! What the status bar says (`DESIGN.md` → "Status bar": import status,
//! "1 agent connected", home currency), read from the views' data so it
//! follows the database.
//!
//! An item with nothing true to say is left out rather than given new
//! wording: no "Reconciled" before any balance check exists, no import line
//! before the first import, no currency while accounts differ in theirs.
//! The sample data keeps Paper's artboard (`1CU-0`).

use chrono::{DateTime, Utc};
use coffer_core::clock;
use coffer_core::import::{BatchInfo, BatchStatus};

use crate::accounts::data::AccountsData;
use crate::format;
use crate::transactions::Counts;

/// The status bar's items, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub ai: Ai,
    pub checks: ChecksStatus,
    /// "Imported 2 min ago from monzo-november.csv".
    pub import: Option<String>,
    /// "328 transactions · 2 excluded".
    pub transactions: String,
    /// "3 new": Paper's sample only, until something defines what it
    /// counts.
    pub new: Option<String>,
    /// "GBP": the one currency every open account is in.
    pub currency: Option<String>,
}

/// The first item, which opens Settings → AI clients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ai {
    /// The kill switch is off: "AI access off".
    Off,
    /// "N agents connected": the clients enabled now. Coffer can't see a
    /// live connection (each client starts `coffer mcp` itself), so
    /// enabled is what it counts.
    Connected(usize),
    /// On, with no client enabled: nothing to show.
    None,
}

/// The second item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksStatus {
    /// "N balance checks failing", which opens Balance checks.
    Failing(usize),
    /// "Reconciled": every check passes.
    Reconciled,
    /// No balance check yet: nothing to show.
    None,
}

/// What the status bar reads.
pub struct Inputs<'a> {
    pub ai_on: bool,
    pub enabled_clients: usize,
    pub failing: usize,
    pub has_checks: bool,
    pub accounts: &'a AccountsData,
    pub counts: Counts,
    pub now: DateTime<Utc>,
}

impl Status {
    pub fn read(inputs: &Inputs) -> Self {
        let sample = inputs.counts.sample;
        let ai = match (inputs.ai_on, inputs.enabled_clients) {
            (false, _) => Ai::Off,
            // Paper's "1 agent connected".
            (true, _) if sample => Ai::Connected(1),
            (true, 0) => Ai::None,
            (true, enabled) => Ai::Connected(enabled),
        };
        let checks = match (inputs.failing, inputs.has_checks || sample) {
            (0, false) => ChecksStatus::None,
            (0, true) => ChecksStatus::Reconciled,
            (failing, _) => ChecksStatus::Failing(failing),
        };
        let import = if sample {
            Some("Imported 2 min ago from monzo-november.csv".to_owned())
        } else {
            latest_import(inputs.accounts).map(|batch| import_text(batch, inputs.now))
        };
        let currency = if sample {
            Some("GBP".to_owned())
        } else {
            one_currency(inputs.accounts)
        };
        Status {
            ai,
            checks,
            import,
            transactions: transactions_count(inputs.counts),
            new: sample.then(|| "3 new".to_owned()),
            currency,
        }
    }
}

/// "1 agent connected", "2 agents connected".
pub fn agents_text(count: usize) -> String {
    let noun = if count == 1 { "agent" } else { "agents" };
    format!("{count} {noun} connected")
}

/// "1 balance check failing", "2 balance checks failing".
pub fn failing_text(failing: usize) -> String {
    let noun = if failing == 1 { "check" } else { "checks" };
    format!("{failing} balance {noun} failing")
}

/// The newest import still in an open account; an undone one doesn't
/// count.
fn latest_import(accounts: &AccountsData) -> Option<&BatchInfo> {
    accounts
        .tree
        .rows()
        .filter_map(|node| accounts.detail(node.account.id))
        .flat_map(|detail| &detail.imports)
        .filter(|batch| batch.status == BatchStatus::Committed)
        // Stored as UTC text, so they sort as times; the id breaks ties.
        .max_by(|a, b| (&a.imported_at, a.id).cmp(&(&b.imported_at, b.id)))
}

/// "Imported 2 min ago from monzo-november.csv".
fn import_text(batch: &BatchInfo, now: DateTime<Utc>) -> String {
    let when = clock::parse_stored(&batch.imported_at).map_or_else(String::new, |at| ago(at, now));
    if when.is_empty() {
        format!("Imported from {}", batch.source_file)
    } else {
        format!("Imported {when} from {}", batch.source_file)
    }
}

/// "just now", "2 min ago", "3 hours ago", else "on 4 Nov".
fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes();
    match minutes {
        ..1 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..120 => "1 hour ago".to_owned(),
        120..1440 => format!("{} hours ago", minutes / 60),
        _ => format!(
            "on {}",
            format::list_date(then.date_naive(), now.date_naive())
        ),
    }
}

/// The currency of every open account, when they share one.
fn one_currency(accounts: &AccountsData) -> Option<String> {
    let mut currencies = accounts.tree.rows().map(|node| node.account.currency);
    let first = currencies.next()?;
    currencies
        .all(|currency| currency == first)
        .then(|| first.code().to_owned())
}

/// "328 transactions", or "328 transactions · 2 excluded".
pub fn transactions_count(counts: Counts) -> String {
    // The fixture holds a page of the month; the status bar counts the
    // whole import, as Paper does.
    const IMPORTED: usize = 328;
    let transactions = if counts.sample {
        IMPORTED
    } else {
        counts.transactions
    };
    let noun = if transactions == 1 {
        "transaction"
    } else {
        "transactions"
    };
    match counts.excluded {
        0 => format!("{transactions} {noun}"),
        excluded => format!("{transactions} {noun} · {excluded} excluded"),
    }
}

#[cfg(test)]
mod tests;
