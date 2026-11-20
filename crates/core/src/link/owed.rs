//! Owed to you: the read model of the third CHECKS queue (ROADMAP M3.11;
//! `DESIGN.md` → "Refunds, reimbursements and shared costs", Paper
//! `CCF-0`; `ARCHITECTURE.md` → "Decision record: refunds, reimbursements
//! and settlements (built)").
//!
//! [`queue`] reads, with no writes:
//!
//! - **Expense claims**: purchases claimed back and not fully repaid,
//!   oldest first, with the account they were paid from; and each payment
//!   Orre suggested as their reimbursement ([`ClaimMatch`]), so "Link as
//!   reimbursement" confirms those suggestions and "Not this" rejects them.
//! - **People**: everyone who owes the user, per currency, who owes most
//!   first (then by name, Paper `CCF-0`). Their
//!   **costs are the outstanding ones** (user, 2026-11-14): what they paid
//!   back (settlements, and refunds of shared costs) clears their oldest
//!   costs first, so the list is what is still owed and sums to it; a cost
//!   partly paid shows the rest. With their payments, newest first.
//!
//! People are local labels and merchants untrusted text: shown plain, never
//! logged, never in errors.

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use super::{Error, LinkId, ShareLabel, outstanding_claims_in, owed_by_label_in};
use crate::db::Database;
use crate::id::{AccountId, TransactionId};
use crate::money::{Currency, Money};

/// The name a row is shown by: the display merchant, else the bank's,
/// else the raw description (as the register names it).
const NAME: &str = "coalesce(nullif(trim(t.merchant_display), ''), nullif(trim(t.merchant), ''), \
     trim(t.description_raw))";

/// One outstanding claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRow {
    pub transaction: TransactionId,
    pub account: AccountId,
    pub account_name: String,
    /// The purchase's name (untrusted text).
    pub merchant: String,
    pub date: NaiveDate,
    /// What was spent: negative.
    pub spent: Money,
    /// What is still to come back: positive.
    pub owed: Money,
    /// In a payment Orre suggested ([`ClaimMatch`]).
    pub matched: bool,
}

/// A payment Orre suggested as the reimbursement of some claims.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimMatch {
    /// The money in.
    pub inflow: TransactionId,
    pub date: NaiveDate,
    /// Its name (untrusted text).
    pub merchant: String,
    /// What the suggestions take, together: positive.
    pub amount: Money,
    /// The suggested links, by claim date.
    pub links: Vec<LinkId>,
    /// The claims they repay.
    pub claims: Vec<TransactionId>,
}

/// Someone's part of one shared cost that is still owed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cost {
    pub transaction: TransactionId,
    pub date: NaiveDate,
    pub merchant: String,
    /// Positive.
    pub part: Money,
}

/// A payment from someone, linked as a settlement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payment {
    pub link: LinkId,
    pub transaction: TransactionId,
    pub date: NaiveDate,
    pub merchant: String,
    /// The bank's kind of payment ("Bank transfer"), when the export has it.
    pub payment_type: Option<String>,
    /// Positive.
    pub amount: Money,
}

/// Someone who owes the user, in one currency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub label: ShareLabel,
    /// Positive.
    pub owed: Money,
    /// The outstanding costs, oldest first; they sum to `owed`.
    pub costs: Vec<Cost>,
    /// Their settlements, newest first.
    pub payments: Vec<Payment>,
}

impl Person {
    /// The newest payment's date.
    pub fn last_paid(&self) -> Option<NaiveDate> {
        self.payments.first().map(|payment| payment.date)
    }
}

/// Everything Owed to you shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwedQueue {
    pub claims: Vec<ClaimRow>,
    pub matches: Vec<ClaimMatch>,
    pub people: Vec<Person>,
    /// What is to come back, per currency (sorted by code).
    pub totals: Vec<Money>,
}

impl OwedQueue {
    /// The open items: the CHECKS row's count.
    pub fn open(&self) -> usize {
        self.claims.len() + self.people.len()
    }
}

/// Reads Owed to you (module docs).
pub fn queue(db: &Database) -> Result<OwedQueue, Error> {
    queue_in(db.conn())
}

/// [`queue`] on any connection.
pub fn queue_in(conn: &Connection) -> Result<OwedQueue, Error> {
    let matches = matches_in(conn)?;
    let mut claims = Vec::new();
    for claim in outstanding_claims_in(conn)? {
        let (account_name, merchant): (String, String) = conn.query_row(
            &format!(
                "SELECT a.name, {NAME} FROM \"transaction\" t \
                 JOIN account a ON a.id = t.account_id WHERE t.id = ?1"
            ),
            [claim.transaction],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        claims.push(ClaimRow {
            matched: matches
                .iter()
                .any(|found| found.claims.contains(&claim.transaction)),
            transaction: claim.transaction,
            account: claim.account,
            account_name,
            merchant,
            date: claim.date,
            spent: claim.spent,
            owed: claim.owed,
        });
    }
    let mut people = Vec::new();
    for person in owed_by_label_in(conn)? {
        if !person.owed.is_positive() {
            continue;
        }
        let currency = person.owed.currency();
        people.push(Person {
            costs: costs_in(conn, &person.label, currency, person.owed.minor())?,
            payments: payments_in(conn, &person.label, currency)?,
            label: person.label,
            owed: person.owed,
        });
    }
    people.sort_by(|a, b| {
        b.owed
            .minor()
            .cmp(&a.owed.minor())
            .then_with(|| a.label.cmp(&b.label))
    });
    let mut totals: Vec<Money> = Vec::new();
    let amounts = claims
        .iter()
        .map(|claim| claim.owed)
        .chain(people.iter().map(|person| person.owed));
    for amount in amounts {
        match totals
            .iter_mut()
            .find(|total| total.currency() == amount.currency())
        {
            Some(total) => {
                *total = Money::new(
                    total.minor().saturating_add(amount.minor()),
                    total.currency(),
                );
            }
            None => totals.push(amount),
        }
    }
    totals.sort_by_key(|total| total.currency());
    Ok(OwedQueue {
        claims,
        matches,
        people,
        totals,
    })
}

/// Suggested reimbursements, one per payment, oldest payment first.
fn matches_in(conn: &Connection) -> Result<Vec<ClaimMatch>, Error> {
    let mut statement = conn.prepare(&format!(
        "SELECT l.id, l.inflow_id, l.target_id, l.amount_minor, t.currency, t.date, {NAME} \
         FROM transaction_link l \
         JOIN \"transaction\" t ON t.id = l.inflow_id \
         JOIN \"transaction\" p ON p.id = l.target_id \
         WHERE l.kind = 'reimbursement' AND l.status = 'suggested' \
         ORDER BY t.date, t.id, p.date, p.id"
    ))?;
    let rows = statement.query_map([], |row| {
        let currency: Currency = row.get(4)?;
        Ok((
            row.get::<_, LinkId>(0)?,
            row.get::<_, TransactionId>(1)?,
            row.get::<_, TransactionId>(2)?,
            Money::new(row.get(3)?, currency),
            crate::account::date(row, "date")?,
            row.get::<_, String>(6)?,
        ))
    })?;
    let mut out: Vec<ClaimMatch> = Vec::new();
    for row in rows {
        let (link, inflow, claim, amount, date, merchant) = row?;
        match out.last_mut() {
            Some(last) if last.inflow == inflow => {
                last.amount = Money::new(
                    last.amount.minor().saturating_add(amount.minor()),
                    amount.currency(),
                );
                last.links.push(link);
                last.claims.push(claim);
            }
            _ => out.push(ClaimMatch {
                inflow,
                date,
                merchant,
                amount,
                links: vec![link],
                claims: vec![claim],
            }),
        }
    }
    Ok(out)
}

/// `label`'s outstanding costs in `currency`, oldest first, summing to
/// `owed` (module docs).
fn costs_in(
    conn: &Connection,
    label: &ShareLabel,
    currency: Currency,
    owed: i64,
) -> Result<Vec<Cost>, Error> {
    let mut statement = conn.prepare(&format!(
        "SELECT l.transaction_id, l.date, l.my_amount_minor - l.amount_minor AS part, {NAME} \
         FROM ledger_line l JOIN \"transaction\" t ON t.id = l.transaction_id \
         WHERE l.share_label = ?1 AND l.currency = ?2 AND l.in_reports AND l.reimbursable = 0 \
           AND l.link_kind IS NULL AND l.amount_minor < 0 \
           AND l.my_amount_minor <> l.amount_minor \
         ORDER BY l.date, l.transaction_id, l.line_no"
    ))?;
    let all: Vec<Cost> = statement
        .query_map(params![label.as_str(), currency], |row| {
            Ok(Cost {
                transaction: row.get(0)?,
                date: crate::account::date(row, "date")?,
                part: Money::new(row.get(2)?, currency),
                merchant: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    // What was paid back clears the oldest costs first.
    let total: i64 = all
        .iter()
        .fold(0i64, |sum, cost| sum.saturating_add(cost.part.minor()));
    let mut covered = total.saturating_sub(owed).max(0);
    let mut out = Vec::new();
    for mut cost in all {
        let part = cost.part.minor();
        if covered >= part {
            covered -= part;
            continue;
        }
        cost.part = Money::new(part - covered, currency);
        covered = 0;
        out.push(cost);
    }
    Ok(out)
}

/// `label`'s confirmed settlements in `currency`, newest first.
fn payments_in(
    conn: &Connection,
    label: &ShareLabel,
    currency: Currency,
) -> Result<Vec<Payment>, Error> {
    let mut statement = conn.prepare(&format!(
        "SELECT l.id, t.id, t.date, {NAME}, t.source_type, l.amount_minor \
         FROM transaction_link l JOIN \"transaction\" t ON t.id = l.inflow_id \
         WHERE l.kind = 'settlement' AND l.status = 'confirmed' AND l.share_label = ?1 \
           AND t.currency = ?2 \
         ORDER BY t.date DESC, t.id DESC"
    ))?;
    let payments = statement
        .query_map(params![label.as_str(), currency], |row| {
            Ok(Payment {
                link: row.get(0)?,
                transaction: row.get(1)?,
                date: crate::account::date(row, "date")?,
                merchant: row.get(3)?,
                payment_type: row.get(4)?,
                amount: Money::new(row.get(5)?, currency),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(payments)
}

#[cfg(test)]
mod tests;
