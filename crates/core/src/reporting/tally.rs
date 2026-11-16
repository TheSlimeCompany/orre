//! [`Tally`]: ledger lines summed the way reports count them, by
//! [`Side`], category and currency, and the difference between two tallies.
//!
//! What a change would do to reports is a tally of the lines it touches
//! read after the change (in a rolled-back DB transaction,
//! [`crate::change::dry_run`]) less the same tally before
//! ([`Tally::change`]). The Inbox's "If you approve" and the review queue's
//! "If you merge" both read it, so they move the same amounts the reports
//! do: the user's share on [`Basis::Personal`], lines out of reports left
//! out ([`Ledger`](super::Ledger)), and the side decided by [`Side::of`].

use std::collections::BTreeMap;

use crate::category::CategoryKey;
use crate::money::{Currency, Money};

use super::{Basis, Error, LedgerLine, Side};

/// Signed sums of the counted amounts of ledger lines (money out below
/// zero), per side, category (`None`: uncategorised) and currency. Lines
/// that count as [`Side::Neither`], and amounts of zero, aren't kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    sums: BTreeMap<(Side, Option<CategoryKey>, Currency), Money>,
}

impl Tally {
    /// Nothing tallied.
    pub fn new() -> Tally {
        Tally::default()
    }

    /// `lines` tallied on `basis`. Fails only when a sum overflows `i64`.
    pub fn of<'a>(
        lines: impl IntoIterator<Item = &'a LedgerLine>,
        basis: Basis,
    ) -> Result<Tally, Error> {
        let mut tally = Tally::new();
        for line in lines {
            tally.add(line, basis)?;
        }
        Ok(tally)
    }

    /// Adds `line`'s counted amount on `basis` to its side and category.
    pub fn add(&mut self, line: &LedgerLine, basis: Basis) -> Result<(), Error> {
        let side = line.side(basis);
        let category = line.category.as_ref().map(|category| category.key.clone());
        self.add_money(side, category, line.counted(basis))
    }

    fn add_money(
        &mut self,
        side: Side,
        category: Option<CategoryKey>,
        money: Money,
    ) -> Result<(), Error> {
        if side == Side::Neither || money.is_zero() {
            return Ok(());
        }
        let key = (side, category, money.currency());
        let sum = match self.sums.get(&key) {
            Some(sum) => sum.checked_add(money)?,
            None => money,
        };
        if sum.is_zero() {
            self.sums.remove(&key);
        } else {
            self.sums.insert(key, sum);
        }
        Ok(())
    }

    /// What changes from `self` (before) to `after`: `after` less `self`,
    /// per side, category and currency, with what stays the same left out.
    /// On the spending side a positive change is less spent.
    pub fn change(&self, after: &Tally) -> Result<Tally, Error> {
        let mut change = after.clone();
        for ((side, category, _), money) in &self.sums {
            change.add_money(*side, category.clone(), money.checked_neg()?)?;
        }
        Ok(change)
    }

    /// Every sum: its side, category (`None`: uncategorised) and signed
    /// amount, by side, then category key (uncategorised first), then
    /// currency code.
    pub fn iter(&self) -> impl Iterator<Item = (Side, Option<&CategoryKey>, Money)> + '_ {
        self.sums
            .iter()
            .map(|((side, category, _), money)| (*side, category.as_ref(), *money))
    }

    /// The sum of `side` in `currency`, over every category: zero when
    /// there is none.
    pub fn total(&self, side: Side, currency: Currency) -> Result<Money, Error> {
        let mut total = Money::zero(currency);
        for (_, _, money) in self
            .iter()
            .filter(|(s, _, money)| *s == side && money.currency() == currency)
        {
            total = total.checked_add(money)?;
        }
        Ok(total)
    }

    /// Nothing tallied, or no change.
    pub fn is_empty(&self) -> bool {
        self.sums.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::CategoryKind;
    use crate::id::{AccountId, TransactionId};
    use crate::reporting::LineCategory;

    fn line(kind: Option<(&str, CategoryKind)>, amount: i64, personal: i64) -> LedgerLine {
        LedgerLine {
            transaction_id: TransactionId::new(1),
            split_id: None,
            line_no: None,
            link_id: None,
            link_kind: None,
            target_id: None,
            account_id: AccountId::new(1),
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            amount: Money::new(amount, Currency::GBP),
            personal: Money::new(personal, Currency::GBP),
            category: kind.map(|(key, kind)| LineCategory {
                id: 1,
                key: CategoryKey::new(key).unwrap(),
                kind,
            }),
            merchant_display: None,
            bank_text: None,
        }
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    #[test]
    fn the_side_is_the_category_kind_or_the_sign_of_what_counts() {
        let groceries = Some(("food.groceries", CategoryKind::Spending));
        let salary = Some(("income.salary", CategoryKind::Income));
        let transfer = Some(("transfer", CategoryKind::Transfer));
        // A refund under a spending category lowers spending.
        assert_eq!(
            line(groceries, 500, 500).side(Basis::Personal),
            Side::Spending
        );
        assert_eq!(line(salary, -500, -500).side(Basis::Personal), Side::Income);
        assert_eq!(
            line(transfer, -500, -500).side(Basis::Personal),
            Side::Neither
        );
        assert_eq!(line(None, -500, -250).side(Basis::Personal), Side::Spending);
        assert_eq!(line(None, 500, 500).side(Basis::Personal), Side::Income);
        // Claimed back: nothing of it is the user's.
        assert_eq!(line(None, -500, 0).side(Basis::Personal), Side::Neither);
        assert_eq!(line(None, -500, 0).side(Basis::Gross), Side::Spending);
    }

    #[test]
    fn a_tally_counts_the_users_share_and_its_change_drops_what_stays() {
        let groceries = Some(("food.groceries", CategoryKind::Spending));
        let shared = line(groceries, -10_000, -5_000);
        let mine = line(None, -2_000, -2_000);
        let claimed = line(None, -3_000, 0);
        let before = Tally::of([&shared, &mine, &claimed], Basis::Personal).unwrap();
        let groceries_key = CategoryKey::new("food.groceries").unwrap();
        assert_eq!(
            before.iter().collect::<Vec<_>>(),
            vec![
                (Side::Spending, None, gbp(-2_000)),
                (Side::Spending, Some(&groceries_key), gbp(-5_000)),
            ]
        );
        assert_eq!(
            before.total(Side::Spending, Currency::GBP).unwrap(),
            gbp(-7_000)
        );
        assert_eq!(before.total(Side::Income, Currency::GBP).unwrap(), gbp(0));

        // The uncategorised row moves to groceries.
        let moved = line(groceries, -2_000, -2_000);
        let after = Tally::of([&shared, &moved, &claimed], Basis::Personal).unwrap();
        let change = before.change(&after).unwrap();
        assert_eq!(
            change.iter().collect::<Vec<_>>(),
            vec![
                (Side::Spending, None, gbp(2_000)),
                (Side::Spending, Some(&groceries_key), gbp(-2_000)),
            ]
        );
        assert!(before.change(&before).unwrap().is_empty());
    }
}
