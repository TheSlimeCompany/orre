//! Counting cash (ROADMAP M3.10; `docs/DESIGN.md` → "Accounts you keep
//! yourself", Cash count; `docs/ARCHITECTURE.md` → "Decision record:
//! accounts you keep yourself", point 5).
//!
//! A cash count is the cash account's [`EnteredBalance`] with source
//! `manual`: the notes and coins the user counted at the end of a day. Like
//! any balance it is checked against the transactions since the last one
//! that passed, and the account's first count is its starting point, which
//! isn't checked.
//!
//! - [`expectation`] is what the dialog shows: what Orre expects on the
//!   count's date (the last count that passed, or the starting point, plus
//!   the transactions after it up to and including that day) and how far
//!   the counted amount is from it. None before the first count.
//! - [`record`] stores the count. When asked, it also stores a manual
//!   transaction "Cash count difference", in Other, on the count's date for
//!   the difference, so the new count passes. Both are written in one DB
//!   transaction, and the difference only when the caller asks: Orre never
//!   posts one on its own.
//!
//! The guards are [`super::add_balance`]'s and [`crate::manual::create`]'s:
//! one count per day, none after the account closed or on or before its
//! lock date, and the amount in the account's currency.

use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior};

use super::{AccountKind, EnteredBalance, EnteredSource, Error as AccountError};
use super::{insert_balance, load};
use crate::db::Database;
use crate::id::{AccountId, BalanceId, TransactionId};
use crate::manual::{self, ManualTransaction};
use crate::money::{Money, MoneyError};
use crate::reconciliation::{self, Outcome};

/// The description of the transaction that covers a difference.
pub const DIFFERENCE_DESCRIPTION: &str = "Cash count difference";

/// The key of the category the difference is filed under.
pub const DIFFERENCE_CATEGORY: &str = "other";

/// What Orre expects of a count, and how far the count is from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expectation {
    /// The count the expectation counts from, or `None` before the first
    /// count: the new count is then the starting point and has nothing to
    /// differ from.
    pub since: Option<NaiveDate>,
    /// The count it started from, plus the transactions since.
    pub expected: Option<Money>,
    /// Counted less expected. Zero when they agree.
    pub difference: Option<Money>,
}

/// What [`record`] stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub balance: BalanceId,
    /// The "Cash count difference" transaction, when one was added.
    pub difference: Option<TransactionId>,
}

/// Why a count couldn't be read or stored. Messages never contain
/// transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} accounts aren't counted; counts are for cash accounts")]
    NotCash(&'static str),

    #[error("there is no difference to add")]
    NoDifference,

    #[error("{0}")]
    Money(#[from] MoneyError),

    #[error("{0}")]
    Account(#[from] AccountError),

    #[error("{0}")]
    Manual(#[from] manual::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// What Orre expects the cash account to hold at the end of `date`, and
/// how far `counted` is from it.
pub fn expectation(
    db: &Database,
    account: AccountId,
    date: NaiveDate,
    counted: Money,
) -> Result<Expectation, Error> {
    expectation_in(db.conn(), account, date, counted)
}

/// [`expectation`] through `conn`: inside [`record`]'s DB transaction, so the
/// difference it stores is for what the database holds when it writes.
fn expectation_in(
    conn: &Connection,
    account: AccountId,
    date: NaiveDate,
    counted: Money,
) -> Result<Expectation, Error> {
    let stored = load(conn, account)?;
    check_cash(stored.kind)?;
    check_currency(&stored, counted)?;
    let checks = reconciliation::load(conn, &stored)?;
    // A failed count isn't a base, as in `balance::check`.
    let base = checks
        .checks
        .iter()
        .rfind(|check| {
            check.date <= date
                && matches!(
                    check.outcome,
                    Outcome::StartingPoint | Outcome::Passed { .. }
                )
        })
        .copied();
    let Some(base) = base else {
        return Ok(Expectation {
            since: None,
            expected: None,
            difference: None,
        });
    };
    let moved: i128 = reconciliation::movements(conn, account)?
        .range((
            std::ops::Bound::Excluded(base.date),
            std::ops::Bound::Included(date),
        ))
        .map(|(_, sum)| sum)
        .sum();
    let expected =
        i64::try_from(i128::from(base.stated.minor()) + moved).map_err(|_| MoneyError::Overflow)?;
    let expected = Money::new(expected, stored.currency);
    Ok(Expectation {
        since: Some(base.date),
        expected: Some(expected),
        difference: Some(counted.checked_sub(expected)?),
    })
}

/// Stores a count of `account`'s cash. With `add_difference`, also stores
/// the "Cash count difference" transaction for what [`expectation`] says is
/// missing, in the same DB transaction; there must be one.
pub fn record(
    db: &mut Database,
    account: AccountId,
    count: &EnteredBalance,
    add_difference: bool,
) -> Result<Recorded, Error> {
    // Locked from the start: the difference is worked out from what the
    // database holds when the count is written, not from an earlier read.
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let difference = if add_difference {
        let found = expectation_in(&tx, account, count.date, count.balance)?
            .difference
            .filter(|difference| !difference.is_zero())
            .ok_or(Error::NoDifference)?;
        Some(found)
    } else {
        None
    };
    let stored = load(&tx, account)?;
    check_cash(stored.kind)?;
    let balance = insert_balance(
        &tx,
        &stored,
        &EnteredBalance {
            source: EnteredSource::Manual,
            ..count.clone()
        },
    )?;
    let difference = difference
        .map(|amount| {
            let category = crate::category::CategoryKey::new(DIFFERENCE_CATEGORY)
                .expect("the default category's key is valid");
            let mut entry =
                ManualTransaction::new(account, count.date, amount, DIFFERENCE_DESCRIPTION);
            // The user may have deleted Other (M3.7): the row is then left
            // uncategorised, where it still shows, rather than refused.
            let exists: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM category WHERE key = ?1)",
                [category.as_str()],
                |row| row.get(0),
            )?;
            entry.category = exists.then_some(category);
            manual::create_in(&tx, &entry)
        })
        .transpose()?;
    tx.commit()?;
    tracing::info!(%account, "counted cash");
    Ok(Recorded {
        balance,
        difference,
    })
}

fn check_cash(kind: AccountKind) -> Result<(), Error> {
    match kind {
        AccountKind::Cash => Ok(()),
        other => Err(Error::NotCash(other.as_str())),
    }
}

fn check_currency(stored: &super::Account, counted: Money) -> Result<(), Error> {
    if counted.currency() == stored.currency {
        Ok(())
    } else {
        Err(AccountError::CurrencyMismatch {
            expected: stored.currency,
            found: counted.currency(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::account::{self, NewAccount};
    use crate::money::Currency;

    fn open() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("orre.db")).unwrap();
        (db, dir)
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn count(d: u32, minor: i64) -> EnteredBalance {
        EnteredBalance {
            date: day(d),
            balance: gbp(minor),
            source: EnteredSource::Manual,
            note: None,
        }
    }

    fn wallet(db: &mut Database) -> AccountId {
        account::create(
            db,
            &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
        )
        .unwrap()
    }

    fn spend(db: &mut Database, account: AccountId, d: u32, minor: i64) {
        manual::create(
            db,
            &ManualTransaction::new(account, day(d), gbp(minor), "Coffee"),
        )
        .unwrap();
    }

    #[test]
    fn the_first_count_is_the_starting_point() {
        let (mut db, _dir) = open();
        let cash = wallet(&mut db);
        let found = expectation(&db, cash, day(1), gbp(2_800)).unwrap();
        assert_eq!(
            found,
            Expectation {
                since: None,
                expected: None,
                difference: None
            }
        );
        // There's nothing to add a difference to.
        assert!(matches!(
            record(&mut db, cash, &count(1, 2_800), true),
            Err(Error::NoDifference)
        ));
        let recorded = record(&mut db, cash, &count(1, 2_800), false).unwrap();
        assert_eq!(recorded.difference, None);
        let stored = account::entered_balances(&db, cash).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source, reconciliation::BalanceSource::Manual);
    }

    #[test]
    fn a_later_count_is_compared_with_the_last_count_and_the_entries_since() {
        let (mut db, _dir) = open();
        let cash = wallet(&mut db);
        record(&mut db, cash, &count(1, 5_000), false).unwrap();
        // On the starting day: already in its balance.
        spend(&mut db, cash, 1, -100);
        spend(&mut db, cash, 4, -2_700);
        spend(&mut db, cash, 9, -9_999);
        let found = expectation(&db, cash, day(10), gbp(2_000)).unwrap();
        assert_eq!(found.since, Some(day(1)));
        assert_eq!(found.expected, Some(gbp(2_300 - 9_999)));
        let found = expectation(&db, cash, day(5), gbp(1_800)).unwrap();
        assert_eq!(found.expected, Some(gbp(2_300)));
        assert_eq!(found.difference, Some(gbp(-500)));
        let found = expectation(&db, cash, day(5), gbp(2_300)).unwrap();
        assert_eq!(found.difference, Some(gbp(0)));
    }

    /// With Other deleted (M3.7), the difference is still added, left
    /// uncategorised.
    #[test]
    fn the_difference_is_uncategorised_when_other_is_gone() {
        let (mut db, _dir) = open();
        let other = crate::category::CategoryKey::new(DIFFERENCE_CATEGORY).unwrap();
        crate::category::delete(&mut db, &other).unwrap();
        let cash = wallet(&mut db);
        record(&mut db, cash, &count(1, 5_000), false).unwrap();
        let recorded = record(&mut db, cash, &count(10, 4_000), true).unwrap();
        let difference = recorded.difference.expect("a difference row");
        let row = manual::get(&db, difference).unwrap();
        assert_eq!(row.amount, gbp(-1_000));
        assert_eq!(row.category, None);
    }

    #[test]
    fn a_short_count_adds_its_difference_only_when_asked() {
        let (mut db, _dir) = open();
        let cash = wallet(&mut db);
        record(&mut db, cash, &count(1, 5_000), false).unwrap();
        spend(&mut db, cash, 4, -2_700);

        let recorded = record(&mut db, cash, &count(10, 1_800), false).unwrap();
        assert_eq!(recorded.difference, None);
        let checks = account::entered_balances(&db, cash).unwrap();
        assert_eq!(checks.len(), 2);
        assert!(
            reconciliation::account_checks(&db, cash)
                .unwrap()
                .failures()
                .next()
                .is_some()
        );
        assert_eq!(
            db.conn()
                .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );

        // The same day again is refused, and nothing is written.
        assert!(matches!(
            record(&mut db, cash, &count(10, 1_800), true),
            Err(Error::Account(AccountError::BalanceExists(_)))
        ));
        assert_eq!(rows(&db), 1);

        let recorded = record(&mut db, cash, &count(12, 2_000), true).unwrap();
        let id = recorded.difference.unwrap();
        let entry = manual::get(&db, id).unwrap();
        assert_eq!(entry.amount, gbp(-300));
        assert_eq!(entry.date, day(12));
        assert_eq!(entry.description, DIFFERENCE_DESCRIPTION);
        assert_eq!(entry.category.unwrap().as_str(), "other");
        // The earlier failed count isn't a base: the new one counts from the
        // first, the difference is in the window, and the check passes.
        let checks = reconciliation::account_checks(&db, cash).unwrap();
        let last = checks.checks.last().unwrap();
        assert!(last.outcome.is_passed(), "{:?}", last.outcome);
    }

    fn rows(db: &Database) -> i64 {
        db.conn()
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn nothing_is_half_written() {
        let (mut db, _dir) = open();
        let cash = wallet(&mut db);
        record(&mut db, cash, &count(1, 5_000), false).unwrap();
        // The difference's date is the count's, so a closed account refuses
        // both; the count isn't kept without its difference.
        db.conn()
            .execute("UPDATE account SET closed_on = '2026-11-03'", [])
            .unwrap();
        assert!(matches!(
            record(&mut db, cash, &count(5, 100), true),
            Err(Error::Account(AccountError::AfterClosing { .. }))
        ));
        assert_eq!(account::entered_balances(&db, cash).unwrap().len(), 1);
        assert_eq!(rows(&db), 0);
    }

    #[test]
    fn the_guards_of_a_balance_and_a_manual_entry_apply() {
        let (mut db, _dir) = open();
        let cash = wallet(&mut db);
        assert!(matches!(
            record(
                &mut db,
                cash,
                &EnteredBalance {
                    balance: Money::new(1, Currency::EUR),
                    ..count(1, 1)
                },
                false
            ),
            Err(Error::Account(AccountError::CurrencyMismatch { .. }))
        ));
        record(&mut db, cash, &count(1, 5_000), false).unwrap();
        record(&mut db, cash, &count(3, 5_000), false).unwrap();
        db.conn()
            .execute("UPDATE account SET reconciled_through = '2026-11-03'", [])
            .unwrap();
        assert!(matches!(
            record(&mut db, cash, &count(2, 1_000), true),
            Err(Error::Account(AccountError::Locked { .. }))
        ));
        assert_eq!(rows(&db), 0);
        // Only a cash account is counted.
        let current = account::create(
            &mut db,
            &NewAccount::new("Joint", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        assert!(matches!(
            record(&mut db, current, &count(1, 5_000), false),
            Err(Error::NotCash("current"))
        ));
    }
}
