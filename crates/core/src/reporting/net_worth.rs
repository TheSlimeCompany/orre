//! `net_worth_history`: month-end balances per account and in total.
//!
//! For the last day of each month, every account's balance from
//! [`balance::balances_at`]: gross, like the bank's (shared costs and
//! claims don't change a balance), from its stated balances and
//! transactions. Pots are accounts of their own, so money moved into a pot
//! stays in net worth; a valuation account (`asset`, `liability`) holds its
//! last valuation and is left out before its first.
//!
//! - **Totals** are per currency ([`Totals`]), never converted.
//! - **Liabilities** are stored negative (what is owed, below zero: the
//!   account form stores a liability's value negative, `DESIGN.md` →
//!   "Accounts you keep yourself"). A liability always lowers net worth:
//!   it counts as minus the size of its balance
//!   ([`AccountWorth::counted`]), so a value entered positive by mistake
//!   still subtracts. Credit cards follow the bank's sign (negative when
//!   money is owed) and count as they are.
//! - **Closed accounts** are left out of every month that ends after their
//!   `closed_on` date; the month they close on its last day still has them.
//! - **Unanchored** accounts (no stated balance at all) are summed from
//!   zero and marked [`AccountWorth::unanchored`]. One rule decides whether
//!   they count, here and in the accounts panel ([`counts`]): an account
//!   other than a pot is listed but left out of the totals until a balance
//!   is entered ([`AccountWorth::is_counted`]), because its sum from zero
//!   is right only if it started empty; a pot is counted from its
//!   transfers. See [`super::account_tree`], which applies the same rule to
//!   the latest balance.

use chrono::NaiveDate;

use crate::account::{Account, AccountKind};
use crate::db::Reader;
use crate::id::AccountId;
use crate::money::Money;

use super::balance::{self, Anchor};
use super::{Capped, DateRange, Error, Limit, Period, Totals};

/// Net worth at the end of one month.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetWorthMonth {
    pub period: Period,
    /// The month's last day, when the balances are taken.
    pub date: NaiveDate,
    /// Every account open on that day with a balance, in the order they
    /// were created.
    pub accounts: Vec<AccountWorth>,
    /// The sum of [`AccountWorth::counted`] per currency, over the accounts
    /// that are [counted](AccountWorth::is_counted): a currency held only by
    /// accounts left out has no total.
    pub total: Totals,
}

/// One account's balance at a month end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountWorth {
    pub account: AccountId,
    /// The user's name for the account.
    pub name: String,
    pub kind: AccountKind,
    /// The balance as stored and computed, gross.
    pub balance: Money,
    /// What it adds to net worth: the balance, except a liability, which
    /// is minus the size of its balance; zero when it is not
    /// [counted](AccountWorth::is_counted) (module docs).
    pub counted: Money,
    /// Summed from zero: the account has no stated balance.
    pub unanchored: bool,
}

impl AccountWorth {
    /// Whether it counts toward net worth: not an unanchored account other
    /// than a pot ([`counts`]).
    pub fn is_counted(&self) -> bool {
        counts(self.kind, !self.unanchored)
    }
}

/// Net worth at the end of every month from `first` through `last` (module
/// docs). A range that ends before it starts is [`Error::EmptyRange`]; one
/// longer than [`Limit::MAX`] months is cut there, with `truncated` set.
pub fn net_worth_history(
    reader: &Reader,
    first: Period,
    last: Period,
) -> Result<Capped<NetWorthMonth>, Error> {
    let periods = Capped::new(DateRange::months(first, last)?.periods(), Limit::MAX);
    let dates: Vec<NaiveDate> = periods.items.iter().map(Period::last_day).collect();
    let mut months: Vec<NetWorthMonth> = periods
        .items
        .iter()
        .zip(&dates)
        .map(|(&period, &date)| NetWorthMonth {
            period,
            date,
            accounts: Vec::new(),
            total: Totals::new(),
        })
        .collect();

    for account in balance::accounts(reader)? {
        let balances = balance::balances_at(reader, &account, &dates)?;
        for (month, balance) in months.iter_mut().zip(balances) {
            let Some(balance) = balance else { continue };
            if account.closed_on.is_some_and(|closed| closed < month.date) {
                continue;
            }
            let unanchored = balance.anchor == Anchor::Unanchored;
            let counted = if counts(account.kind, !unanchored) {
                let counted = counted(&account, balance.balance)?;
                month.total.add(counted)?;
                counted
            } else {
                // Not added at all, so a currency held only by accounts
                // left out has no total, as in the accounts panel.
                Money::zero(balance.balance.currency())
            };
            month.accounts.push(AccountWorth {
                account: account.id,
                name: account.name.clone(),
                kind: account.kind,
                balance: balance.balance,
                counted,
                unanchored,
            });
        }
    }
    Ok(Capped {
        items: months,
        truncated: periods.truncated,
    })
}

/// Whether a balance of an account of this kind counts toward net worth:
/// one that is stated does, and so does a pot's, counted from its
/// transfers; an unanchored account other than a pot does not, until a
/// balance is entered (module docs). Shared by `account_tree`.
pub fn counts(kind: AccountKind, stated: bool) -> bool {
    stated || kind == AccountKind::Pot
}

/// What `balance` adds to net worth: a liability always subtracts.
pub(crate) fn counted(account: &Account, balance: Money) -> Result<Money, Error> {
    Ok(match account.kind {
        AccountKind::Liability => balance.checked_abs()?.checked_neg()?,
        _ => balance,
    })
}

#[cfg(test)]
mod tests;
