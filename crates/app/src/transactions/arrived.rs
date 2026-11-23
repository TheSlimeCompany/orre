//! What a read of the register changed (ROADMAP LU.5; `DESIGN.md` → "Live
//! updates: showing what arrived"), worked out from the rows before and
//! after, with no window: [`RegisterChange::between`]. The view reports it
//! ([`super::RegisterChanged`]) and words the inspector's note for rows that
//! went ([`removed_note`]).
//!
//! "Changed" means a user-facing field differs: what the user, a rule,
//! `categorize`, `inbox approve` or the transfer and link matchers set. The
//! imported source fields (date, time, amount, original amount, raw
//! description, bank merchant, payment type, bank note, source) are never
//! edited, so they are not compared; neither is the account, which can't
//! move. A row's merchant is compared as the table shows it (the display
//! merchant, else the bank's).

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use orre_core::id::TransactionId;
use orre_core::money::Money;
use orre_core::reporting::register::{RegisterRow, RowSource};

use super::linking;
use crate::format::{self, Sign};

/// What the inspector says about a row that went, taken from the register
/// loaded before the read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedRow {
    pub id: TransactionId,
    /// The merchant as the table shows it, made plain.
    pub name: String,
    /// Signed.
    pub amount: Money,
    pub date: NaiveDate,
    /// The file of the import it came from; `None` for a manual row.
    pub file: Option<String>,
}

/// What a read changed in the register.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisterChange {
    /// Rows that weren't there before, in the new register's order.
    pub new: Vec<TransactionId>,
    /// Rows in both whose user-facing fields differ.
    pub changed: Vec<TransactionId>,
    /// Rows that went, in the old register's order.
    pub removed: Vec<RemovedRow>,
    /// A row that came from an import went: the import was undone. Worked
    /// out from the rows alone (nothing else removes an imported row), so
    /// it names no batch.
    pub import_undone: bool,
}

impl RegisterChange {
    /// What a read changed: the rows before it (`None` for the first read,
    /// which has nothing to compare with and reports nothing) and after.
    pub fn of_read(before: Option<&[RegisterRow]>, after: &[RegisterRow]) -> Self {
        before.map_or_else(Self::default, |before| Self::between(before, after))
    }

    /// What differs between `before` and `after`.
    pub fn between(before: &[RegisterRow], after: &[RegisterRow]) -> Self {
        let old: HashMap<TransactionId, &RegisterRow> =
            before.iter().map(|row| (row.id, row)).collect();
        let kept: HashSet<TransactionId> = after.iter().map(|row| row.id).collect();
        let mut change = Self::default();
        for row in after {
            match old.get(&row.id) {
                None => change.new.push(row.id),
                Some(was) if user_facing_differs(was, row) => change.changed.push(row.id),
                Some(_) => {}
            }
        }
        for row in before.iter().filter(|row| !kept.contains(&row.id)) {
            let file = match &row.source {
                RowSource::Import { file, .. } => Some(file.clone()),
                RowSource::Manual { .. } => None,
            };
            change.import_undone |= file.is_some();
            change.removed.push(RemovedRow {
                id: row.id,
                name: linking::name(&row.merchant),
                amount: row.amount,
                date: row.date,
                file,
            });
        }
        change
    }

    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

fn user_facing_differs(was: &RegisterRow, now: &RegisterRow) -> bool {
    was.merchant != now.merchant
        || was.merchant_set != now.merchant_set
        || was.category != now.category
        || was.rule != now.rule
        || was.transfer != now.transfer
        || was.pair != now.pair
        || was.excluded != now.excluded
        || was.excluded_note != now.excluded_note
        || was.note != now.note
        || was.tags != now.tags
        || was.links != now.links
        || was.share != now.share
}

/// The inspector's empty state for rows that went in a read the app didn't
/// ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedNote {
    pub title: String,
    pub body: String,
}

/// The caption of a dialog about a row that went.
pub const DIALOG_CAPTION: &str = "This transaction was removed outside this window.";

/// The note for `rows`, which are all of the selection that went. `today`
/// dates them as the lists do.
///
/// One row: "Pret A Manger, −£6.45 on 9 Nov, came from monzo-november.csv.
/// That import was undone outside this window." Several: "These 3
/// transactions were removed", "They came from monzo-november.csv." A
/// manual row has no import, so it says it was deleted; rows from more than
/// one file, or some manual, say they were removed.
pub fn removed_note(rows: &[RemovedRow], today: NaiveDate) -> Option<RemovedNote> {
    let [first, rest @ ..] = rows else {
        return None;
    };
    if rest.is_empty() {
        let what = format!(
            "{}, {} on {}",
            first.name,
            format::money(first.amount, Sign::Always),
            format::list_date(first.date, today)
        );
        let body = match &first.file {
            Some(file) => {
                format!("{what}, came from {file}. That import was undone outside this window.")
            }
            None => format!("{what}, was deleted outside this window."),
        };
        return Some(RemovedNote {
            title: "This transaction was removed".to_owned(),
            body,
        });
    }
    let mut files: Vec<&str> = rows.iter().filter_map(|row| row.file.as_deref()).collect();
    files.sort_unstable();
    files.dedup();
    let body = if files.len() == 1 && rows.iter().all(|row| row.file.is_some()) {
        format!("They came from {}.", files[0])
    } else if files.is_empty() {
        "They were deleted outside this window.".to_owned()
    } else {
        "They were removed outside this window.".to_owned()
    };
    Some(RemovedNote {
        title: format!("These {} transactions were removed", rows.len()),
        body,
    })
}

#[cfg(test)]
mod tests {
    use orre_core::money::Currency;
    use orre_core::reporting::register::RowCategory;

    use super::*;
    use crate::transactions::fixture;

    fn rows() -> Vec<RegisterRow> {
        fixture::register().rows().to_vec()
    }

    fn row<'a>(rows: &'a mut [RegisterRow], name: &str) -> &'a mut RegisterRow {
        let id = fixture::id(name);
        rows.iter_mut().find(|row| row.id == id).unwrap()
    }

    #[test]
    fn rows_that_are_the_same_report_nothing() {
        let rows = rows();
        assert!(RegisterChange::between(&rows, &rows).is_empty());
    }

    #[test]
    fn a_first_read_reports_nothing() {
        assert!(RegisterChange::of_read(None, &rows()).is_empty());
    }

    #[test]
    fn a_row_that_wasnt_there_is_new() {
        let after = rows();
        let before: Vec<_> = after[1..].to_vec();
        let change = RegisterChange::between(&before, &after);
        assert_eq!(change.new, [after[0].id]);
        assert!(change.changed.is_empty() && change.removed.is_empty());
    }

    #[test]
    fn a_row_whose_user_facing_field_differs_is_changed() {
        let before = rows();
        let mut after = before.clone();
        row(&mut after, "txn-pret-09").category = RowCategory::Uncategorised;
        row(&mut after, "txn-amazon-09").note = Some("Gift".to_owned());
        row(&mut after, "txn-steam-07").merchant = "Steam Games".to_owned();
        let change = RegisterChange::between(&before, &after);
        let mut expected = vec![
            fixture::id("txn-pret-09"),
            fixture::id("txn-amazon-09"),
            fixture::id("txn-steam-07"),
        ];
        let mut changed = change.changed.clone();
        expected.sort();
        changed.sort();
        assert_eq!(changed, expected);
        assert!(change.new.is_empty() && change.removed.is_empty());
    }

    #[test]
    fn a_source_field_is_not_a_change() {
        let before = rows();
        let mut after = before.clone();
        row(&mut after, "txn-pret-09").description = "OTHER TEXT".to_owned();
        assert!(RegisterChange::between(&before, &after).is_empty());
    }

    #[test]
    fn a_row_that_went_is_removed_with_what_the_note_needs() {
        let before = rows();
        let gone = fixture::id("txn-pret-09");
        let after: Vec<_> = before
            .iter()
            .filter(|row| row.id != gone)
            .cloned()
            .collect();
        let change = RegisterChange::between(&before, &after);
        let was = before.iter().find(|row| row.id == gone).unwrap();
        assert_eq!(change.removed.len(), 1);
        let removed = &change.removed[0];
        assert_eq!(removed.id, gone);
        assert_eq!(removed.name, linking::name(&was.merchant));
        assert_eq!(removed.amount, was.amount);
        assert_eq!(removed.date, was.date);
        assert_eq!(
            removed.file.is_some(),
            matches!(was.source, RowSource::Import { .. })
        );
        assert_eq!(change.import_undone, removed.file.is_some());
        assert!(change.new.is_empty() && change.changed.is_empty());
    }

    fn removed(name: &str, file: Option<&str>) -> RemovedRow {
        RemovedRow {
            id: TransactionId::new(1),
            name: name.to_owned(),
            amount: Money::new(-645, Currency::GBP),
            date: NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
            file: file.map(str::to_owned),
        }
    }

    #[test]
    fn the_note_names_the_row_and_its_import() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let note = removed_note(
            &[removed("Pret A Manger", Some("monzo-november.csv"))],
            today,
        )
        .unwrap();
        assert_eq!(note.title, "This transaction was removed");
        assert_eq!(
            note.body,
            "Pret A Manger, −£6.45 on 9 Nov, came from monzo-november.csv. That import was \
             undone outside this window."
        );
        let many = [
            removed("A", Some("monzo-november.csv")),
            removed("B", Some("monzo-november.csv")),
            removed("C", Some("monzo-november.csv")),
        ];
        let note = removed_note(&many, today).unwrap();
        assert_eq!(note.title, "These 3 transactions were removed");
        assert_eq!(note.body, "They came from monzo-november.csv.");
    }

    #[test]
    fn the_note_without_a_file_says_deleted() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let note = removed_note(&[removed("Cash", None)], today).unwrap();
        assert_eq!(
            note.body,
            "Cash, −£6.45 on 9 Nov, was deleted outside this window."
        );
        let note = removed_note(&[removed("A", None), removed("B", None)], today).unwrap();
        assert_eq!(note.body, "They were deleted outside this window.");
        let mixed = [removed("A", None), removed("B", Some("a.csv"))];
        assert_eq!(
            removed_note(&mixed, today).unwrap().body,
            "They were removed outside this window."
        );
        assert_eq!(removed_note(&[], today), None);
    }
}
