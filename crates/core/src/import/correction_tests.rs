//! Tests for the bank's corrections (`supersede`): a source ID the account
//! holds, on another date or for another amount.

use chrono::NaiveDate;
use rusqlite::params;
use rusqlite::types::Value as SqlValue;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, EnteredBalance, EnteredSource, NewAccount};
use crate::change::{ExclusionValue, Recorder, Value};
use crate::exclusion::{self, ExclusionReason};
use crate::reporting::{self, Period};

struct Fixture {
    db: Database,
    _dir: TempDir,
    account: AccountId,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("orre.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            _dir: dir,
            account,
        }
    }

    /// A file of `rows` (day of November 2026, amount, source ID).
    fn preview(&self, rows: &[(u32, i64, &str)]) -> ImportPreview {
        let rows: Vec<_> = rows
            .iter()
            .map(|&(d, minor, id)| (day(d), minor, id))
            .collect();
        self.preview_dated(&rows)
    }

    /// A file of `rows` (date, amount, source ID).
    fn preview_dated(&self, rows: &[(NaiveDate, i64, &str)]) -> ImportPreview {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        for (index, (date, minor, id)) in rows.iter().enumerate() {
            let mut line = StatementLine::new(*date, gbp(*minor), format!("Shop {id}"));
            line.external_id = Some((*id).to_owned());
            statement.push(SourceLocation::Entry(index as u64 + 1), Ok(line));
        }
        preview(&self.db, self.account, "statement.ofx", statement).unwrap()
    }

    /// Imports `rows`, accepting whatever the preview asks for.
    fn import(&mut self, rows: &[(u32, i64, &str)]) -> CommitSummary {
        let preview = self.preview(rows);
        self.accept(&preview)
    }

    /// Imports `rows` (date, amount, source ID).
    fn import_dated(&mut self, rows: &[(NaiveDate, i64, &str)]) -> CommitSummary {
        let preview = self.preview_dated(rows);
        self.accept(&preview)
    }

    /// Commits `preview`, accepting whatever it asks for.
    fn accept(&mut self, preview: &ImportPreview) -> CommitSummary {
        let confirm = preview.needs_confirmation();
        match commit(&mut self.db, preview, confirm).unwrap() {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => panic!("expected a batch"),
        }
    }

    /// A monthly schedule for `amount_minor` from 3 November, matched by
    /// `merchant`, with no amount tolerance. Inserted directly, so it
    /// matches nothing on its own.
    fn schedule(&self, merchant: &str, amount_minor: i64) -> i64 {
        self.db
            .conn()
            .execute(
                "INSERT INTO schedule (account_id, merchant_display, cadence, start_date, \
                 amount_minor, currency, tolerance_days) \
                 VALUES (?1, ?2, 'monthly', '2026-11-03', ?3, 'GBP', 3)",
                params![self.account, merchant, amount_minor],
            )
            .unwrap();
        self.db.conn().last_insert_rowid()
    }

    /// Matches occurrence 3 November of `schedule` to `transaction`, as
    /// made by `by` at `at`.
    fn match_on(&self, schedule: i64, transaction: TransactionId, by: &str, at: &str) {
        self.db
            .conn()
            .execute(
                "INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, \
                 matched_by, matched_at) VALUES (?1, '2026-11-03', ?2, ?3, ?4)",
                params![schedule, transaction, by, at],
            )
            .unwrap();
    }

    /// Every schedule match: schedule, occurrence date, transaction, how
    /// and when it was matched.
    fn matches(&self) -> Vec<ScheduleMatch> {
        let mut statement = self
            .db
            .conn()
            .prepare(
                "SELECT schedule_id, occurrence_date, transaction_id, matched_by, matched_at \
                 FROM schedule_match ORDER BY schedule_id, occurrence_date",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The live row with source ID `id`.
    fn live(&self, id: &str) -> TransactionId {
        self.db
            .conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE external_id = ?1 AND superseded_by IS NULL",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn sql(&self, sql: &str, id: TransactionId) {
        self.db.conn().execute(sql, [id]).unwrap();
    }

    /// Every column of transaction `id`.
    fn row(&self, id: TransactionId) -> Vec<SqlValue> {
        let mut statement = self
            .db
            .conn()
            .prepare("SELECT * FROM \"transaction\" WHERE id = ?1")
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_row([id], |row| {
                (0..columns)
                    .map(|index| row.get::<_, SqlValue>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str, id: TransactionId) -> T {
        self.db
            .conn()
            .query_row(sql, [id], |row| row.get(0))
            .unwrap()
    }

    fn exclusion(&self, id: TransactionId) -> Option<exclusion::Exclusion> {
        exclusion::get(&self.db, id).unwrap()
    }

    fn lock(&mut self, d: u32) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![account::date_key(day(d)), self.account],
            )
            .unwrap();
    }

    fn movements(&self) -> Vec<(NaiveDate, i128)> {
        crate::reconciliation::movements(self.db.conn(), self.account)
            .unwrap()
            .into_iter()
            .collect()
    }
}

/// A `schedule_match` row: schedule, occurrence date, transaction,
/// `matched_by`, `matched_at`.
type ScheduleMatch = (i64, String, TransactionId, String, String);

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn outcomes(preview: &ImportPreview) -> Vec<RowOutcome> {
    preview.rows().map(|row| row.outcome.clone()).collect()
}

fn correction(outcome: &RowOutcome) -> &Correction {
    match outcome {
        RowOutcome::Corrects(correction) => correction,
        other => panic!("expected a correction, got {other:?}"),
    }
}

fn blocked(outcome: &RowOutcome) -> CorrectionBlocked {
    match outcome {
        RowOutcome::Flagged(Flag::SourceIdConflict { blocked, .. }) => *blocked,
        other => panic!("expected a blocked correction, got {other:?}"),
    }
}

// Preview ---------------------------------------------------------------------

#[test]
fn a_new_amount_or_date_for_a_source_id_is_a_correction() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1"), (4, -2_000, "tx_2"), (5, -300, "tx_3")]);
    let tip = f.live("tx_1");
    let moved = f.live("tx_2");

    let preview = f.preview(&[(3, -1_200, "tx_1"), (6, -2_000, "tx_2"), (5, -300, "tx_3")]);

    let rows = outcomes(&preview);
    assert_eq!(
        correction(&rows[0]),
        &Correction {
            existing: tip,
            was_date: day(3),
            was_amount: gbp(-1_000),
            fingerprint: "v1|2026-11-03|-1200|GBP|0|SHOP TX_1".to_owned(),
            not_carried: vec![],
        }
    );
    assert_eq!(correction(&rows[1]).existing, moved);
    assert_eq!(correction(&rows[1]).was_date, day(4));
    assert!(matches!(rows[2], RowOutcome::AlreadyImported { .. }));
    assert_eq!(
        preview.counts(),
        PreviewCounts {
            new: 0,
            already_imported: 1,
            flagged: 0,
            corrections: 2,
        }
    );
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: false,
            balance_mismatch: false,
            corrections: true,
        }
    );
    assert!(!preview.is_nothing_new());
    assert_eq!(
        preview
            .corrections()
            .map(|(location, c)| (location, c.existing))
            .collect::<Vec<_>>(),
        [
            (SourceLocation::Entry(1), tip),
            (SourceLocation::Entry(2), moved)
        ]
    );
}

#[test]
fn a_commit_without_accepting_the_corrections_is_refused() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let preview = f.preview(&[(3, -1_200, "tx_1")]);
    let refused = commit(&mut f.db, &preview, Confirmations::default());
    let Err(Error::NotConfirmed(needed)) = refused else {
        panic!("expected a refusal, got {refused:?}");
    };
    assert!(needed.corrections);
    assert_eq!(
        Error::NotConfirmed(needed).to_string(),
        "the import has the bank's corrections of stored rows to confirm"
    );
    // Nothing was written, not even a backup.
    let batches: i64 =
        f.db.conn()
            .query_row("SELECT COUNT(*) FROM import_batch", [], |row| row.get(0))
            .unwrap();
    assert_eq!(batches, 1);
    assert!(crate::backup::list(f.db.path()).unwrap().len() <= 1);
}

/// Each tie that would break silently keeps the row flagged, with why.
#[test]
fn a_stored_row_that_cant_be_replaced_stays_flagged_with_the_reason() {
    let mut f = Fixture::new();
    let savings = account::create(
        &mut f.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    f.import(&[
        (3, -1_000, "excluded"),
        (3, -1_100, "transfer"),
        (3, -1_200, "linked"),
        (3, 400, "refund"),
    ]);
    let excluded = f.live("excluded");
    exclusion::exclude(&mut f.db, excluded, ExclusionReason::Disputed, None).unwrap();
    let transfer = f.live("transfer");
    let other_side = crate::manual::create(
        &mut f.db,
        &crate::manual::ManualTransaction::new(savings, day(3), gbp(1_100), "From current"),
    )
    .unwrap();
    f.db.conn()
        .execute_batch("INSERT INTO transfer_group (status) VALUES ('suggested')")
        .unwrap();
    f.db.conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = 1 WHERE id IN (?1, ?2)",
            params![transfer, other_side],
        )
        .unwrap();
    let linked = f.live("linked");
    f.db.conn()
        .execute(
            "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
             VALUES ('refund', ?1, ?2, 400, 'suggested')",
            params![f.live("refund"), linked],
        )
        .unwrap();
    let preview = f.preview(&[
        (3, -1_010, "excluded"),
        (3, -1_110, "transfer"),
        (3, -1_210, "linked"),
        (4, -50, "fresh"),
    ]);

    let rows = outcomes(&preview);
    assert_eq!(
        rows[..3].iter().map(blocked).collect::<Vec<_>>(),
        [
            CorrectionBlocked::Excluded,
            CorrectionBlocked::InTransfer,
            CorrectionBlocked::Linked,
        ]
    );
    assert_eq!(
        rows[0],
        RowOutcome::Flagged(Flag::SourceIdConflict {
            existing: excluded,
            blocked: CorrectionBlocked::Excluded
        })
    );
    assert_eq!(preview.counts().flagged, 3);
    assert!(!preview.needs_confirmation().corrections);
    let flagged = match &rows[1] {
        RowOutcome::Flagged(flag) => flag.to_string(),
        _ => unreachable!(),
    };
    assert_eq!(
        flagged,
        format!(
            "the account already has transaction {transfer} with this ID, on another date or \
             for another amount, and can't replace it with this version: it is in a transfer \
             pair"
        )
    );
    // The flags are recorded with the batch, as before.
    let summary = match commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap() {
        Committed::Batch(summary) => summary,
        Committed::NothingNew => panic!("expected a batch"),
    };
    assert_eq!((summary.transactions, summary.flags), (1, 3));
    assert_eq!(
        batch_flags(&f.db, summary.batch)
            .unwrap()
            .iter()
            .map(|flag| flag.kind)
            .collect::<Vec<_>>(),
        [FlagKind::SourceIdConflict; 3]
    );
}

#[test]
fn a_lock_on_either_date_keeps_the_row_flagged() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "old"), (8, -1_000, "new")]);
    f.lock(5);

    let preview = f.preview(&[(9, -1_000, "old"), (4, -1_000, "new")]);

    assert_eq!(
        outcomes(&preview).iter().map(blocked).collect::<Vec<_>>(),
        [
            CorrectionBlocked::Locked { through: day(5) },
            CorrectionBlocked::Locked { through: day(5) },
        ]
    );
}

/// A pot's mirrored row isn't the bank's row for that ID.
#[test]
fn a_mirrored_row_is_not_replaced() {
    let mut f = Fixture::new();
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(f.account);
    let pot = account::create(&mut f.db, &pot).unwrap();
    f.db.conn()
        .execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 'a.csv', 'csv')",
            [f.account],
        )
        .unwrap();
    let batch = f.db.conn().last_insert_rowid();
    f.db.conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, external_id, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-03', 500, 'GBP', 'x', 'X', 'pot_1', 'fp', 'mirrored')",
            params![pot, batch],
        )
        .unwrap();
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    let mut line = StatementLine::new(day(3), gbp(600), "x");
    line.external_id = Some("pot_1".to_owned());
    statement.push(SourceLocation::Line(2), Ok(line));
    let preview = preview(&f.db, pot, "pot.csv", statement).unwrap();
    assert_eq!(
        blocked(&outcomes(&preview)[0]),
        CorrectionBlocked::NotImported
    );
}

// The same payment -----------------------------------------------------------

/// A reused source ID far from the stored row's date isn't a correction:
/// the row stays flagged and out, and the stored row stays as it was.
#[test]
fn a_reused_id_40_days_on_is_not_the_same_payment() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let stored = f.live("tx_1");
    let before = f.row(stored);
    let far = day(3) + chrono::Days::new(40);

    let preview = f.preview_dated(&[(far, -1_000, "tx_1"), (day(4), -50, "fresh")]);

    let rows = outcomes(&preview);
    assert_eq!(
        rows[0],
        RowOutcome::Flagged(Flag::SourceIdConflict {
            existing: stored,
            blocked: CorrectionBlocked::NotSamePayment
        })
    );
    assert_eq!(
        CorrectionBlocked::NotSamePayment.to_string(),
        "its date or direction is too far from the stored row's to be the bank's correction \
         of it"
    );
    assert!(!preview.needs_confirmation().corrections);
    let summary = f.accept(&preview);
    assert_eq!((summary.transactions, summary.flags), (1, 1));
    assert!(summary.corrections.is_empty());
    assert_eq!(f.live("tx_1"), stored);
    assert_eq!(f.row(stored), before);
}

/// Money in under the ID of money out (or the other way) is another row.
#[test]
fn a_reused_id_moving_money_the_other_way_is_not_the_same_payment() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_200, "out"), (3, 1_200, "in")]);

    let preview = f.preview(&[(3, 1_200, "out"), (4, -1_200, "in")]);

    assert_eq!(
        outcomes(&preview).iter().map(blocked).collect::<Vec<_>>(),
        [
            CorrectionBlocked::NotSamePayment,
            CorrectionBlocked::NotSamePayment
        ]
    );
    assert_eq!(preview.counts().corrections, 0);
}

/// The window is inclusive, either way; a day past it is too far.
#[test]
fn a_correction_may_be_dated_up_to_31_days_from_the_stored_row() {
    let mut f = Fixture::new();
    assert_eq!(CORRECTION_WINDOW_DAYS, 31);
    let later = day(3) + chrono::Days::new(31);
    f.import_dated(&[
        (day(3), -1_000, "forward"),
        (later, -1_000, "back"),
        (day(3), -1_000, "too_far"),
    ]);

    let preview = f.preview_dated(&[
        (later, -1_050, "forward"),
        (day(3), -1_050, "back"),
        (later + chrono::Days::new(1), -1_050, "too_far"),
    ]);

    let rows = outcomes(&preview);
    assert_eq!(correction(&rows[0]).existing, f.live("forward"));
    assert_eq!(correction(&rows[1]).existing, f.live("back"));
    assert_eq!(blocked(&rows[2]), CorrectionBlocked::NotSamePayment);
}

/// A zero amount, a pending authorisation, fits either direction.
#[test]
fn a_zero_amount_corrects_to_and_from_any_amount() {
    let mut f = Fixture::new();
    f.import(&[(3, 0, "pending"), (3, -1_000, "voided"), (3, 0, "credit")]);

    let preview = f.preview(&[(4, -1_000, "pending"), (4, 0, "voided"), (4, 500, "credit")]);

    let rows = outcomes(&preview);
    assert_eq!(correction(&rows[0]).existing, f.live("pending"));
    assert_eq!(correction(&rows[1]).existing, f.live("voided"));
    assert_eq!(correction(&rows[2]).existing, f.live("credit"));
    let summary = f.accept(&preview);
    assert_eq!(summary.corrections.len(), 3);
}

// Commit ----------------------------------------------------------------------

/// The user's fields set on the stored row, as SQL on its ID.
const USER_FIELDS: &str = "UPDATE \"transaction\" SET \
    category_id = (SELECT id FROM category WHERE key = 'food'), category_source = 'user', \
    merchant_display = 'Corner Café', merchant_display_source = 'user', \
    user_note = 'with Sam', my_share_minor = -500, share_label = 'Sam', reimbursable = 1 \
    WHERE id = ?1";

#[test]
fn a_correction_supersedes_the_stored_row_and_carries_the_users_fields() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    f.sql(USER_FIELDS, old);
    f.db.conn()
        .execute_batch("INSERT INTO tag (name) VALUES ('holiday')")
        .unwrap();
    f.sql(
        "INSERT INTO transaction_tag (transaction_id, tag_id) VALUES (?1, 1)",
        old,
    );
    let before = f.row(old);

    let summary = f.import(&[(3, -1_200, "tx_1")]);

    let new = f.live("tx_1");
    assert_ne!(new, old);
    assert_eq!(
        summary.corrections,
        [AppliedCorrection {
            replaced: old,
            by: new,
            not_carried: vec![],
        }]
    );
    assert_eq!(summary.transactions, 1);
    // The stored row: excluded as superseded by the new one, all else as it
    // was.
    let exclusion = f.exclusion(old).unwrap();
    assert_eq!(exclusion.reason, ExclusionReason::Superseded);
    assert_eq!(exclusion.superseded_by, Some(new));
    assert_eq!(exclusion.note, None);
    let after = f.row(old);
    let column = |name: &str| {
        let statement =
            f.db.conn()
                .prepare("SELECT * FROM \"transaction\" LIMIT 0")
                .unwrap();
        statement.column_index(name).unwrap()
    };
    let changed: Vec<usize> = (0..before.len())
        .filter(|&i| before[i] != after[i])
        .collect();
    assert_eq!(
        changed,
        [
            column("excluded_reason"),
            column("excluded_at"),
            column("superseded_by")
        ]
    );
    // The new row: the bank's amount, the user's fields.
    assert_eq!(
        f.one::<i64>(
            "SELECT amount_minor FROM \"transaction\" WHERE id = ?1",
            new
        ),
        -1_200
    );
    let fields: (String, String, String, String, i64, String, i64) =
        f.db.conn()
            .query_row(
                "SELECT c.key, t.category_source, t.merchant_display, t.user_note, \
             t.my_share_minor, t.share_label, t.reimbursable \
             FROM \"transaction\" t JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
                [new],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(
        fields,
        (
            "food".to_owned(),
            "user".to_owned(),
            "Corner Café".to_owned(),
            "with Sam".to_owned(),
            -500,
            "Sam".to_owned(),
            1
        )
    );
    assert_eq!(
        f.one::<i64>(
            "SELECT COUNT(*) FROM transaction_tag WHERE transaction_id = ?1",
            new
        ),
        1
    );
    // Only the live row holds the ID: importing the file again adds nothing.
    assert!(f.preview(&[(3, -1_200, "tx_1")]).is_nothing_new());
}

#[test]
fn what_doesnt_fit_the_new_amount_stays_on_the_stored_row_and_is_listed() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "split"), (4, -1_000, "share")]);
    let split = f.live("split");
    f.sql(
        "INSERT INTO transaction_split (transaction_id, line_no, amount_minor) \
         VALUES (?1, 1, -600), (?1, 2, -400)",
        split,
    );
    let share = f.live("share");
    f.sql(
        "UPDATE \"transaction\" SET my_share_minor = -800, share_label = 'Sam' WHERE id = ?1",
        share,
    );

    let preview = f.preview(&[(3, -500, "split"), (4, -500, "share")]);
    let rows = outcomes(&preview);
    assert_eq!(correction(&rows[0]).not_carried, [NotCarried::Split]);
    assert_eq!(correction(&rows[1]).not_carried, [NotCarried::Share]);
    let summary = match commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap() {
        Committed::Batch(summary) => summary,
        Committed::NothingNew => panic!("expected a batch"),
    };

    assert_eq!(
        summary
            .corrections
            .iter()
            .map(|c| (c.replaced, c.not_carried.clone()))
            .collect::<Vec<_>>(),
        [
            (split, vec![NotCarried::Split]),
            (share, vec![NotCarried::Share])
        ]
    );
    // The stored rows keep theirs; the new ones have none.
    assert_eq!(
        f.one::<i64>(
            "SELECT COUNT(*) FROM transaction_split WHERE transaction_id = ?1",
            split
        ),
        2
    );
    assert_eq!(
        f.one::<i64>(
            "SELECT COUNT(*) FROM transaction_split WHERE transaction_id = ?1",
            f.live("split")
        ),
        0
    );
    assert_eq!(
        f.one::<i64>(
            "SELECT my_share_minor FROM \"transaction\" WHERE id = ?1",
            share
        ),
        -800
    );
    assert_eq!(
        f.one::<Option<i64>>(
            "SELECT my_share_minor FROM \"transaction\" WHERE id = ?1",
            f.live("share")
        ),
        None
    );
}

#[test]
fn a_split_moves_when_only_the_date_changes() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    f.sql(
        "INSERT INTO transaction_split (transaction_id, line_no, amount_minor) \
         VALUES (?1, 1, -600), (?1, 2, -400)",
        old,
    );

    let summary = f.import(&[(5, -1_000, "tx_1")]);

    assert_eq!(summary.corrections[0].not_carried, []);
    let new = f.live("tx_1");
    assert_eq!(
        f.one::<i64>(
            "SELECT COUNT(*) FROM transaction_split WHERE transaction_id = ?1",
            new
        ),
        2
    );
    assert_eq!(
        f.one::<Option<i64>>("SELECT category_id FROM \"transaction\" WHERE id = ?1", new),
        None
    );
}

/// The rules run on the new row as on any imported row; a category the
/// user set wins over theirs, a rule's own category is worked out again.
#[test]
fn the_rules_run_on_the_new_row_but_the_users_category_wins() {
    let mut f = Fixture::new();
    {
        use crate::rule::{AmountBounds, MatchField, MatchKind, Pattern, RuleAction};
        let definition = crate::rule::RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Contains, "SHOP").unwrap(),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SetCategory(
                CategoryKey::new("transport").unwrap(),
            )],
        )
        .unwrap();
        crate::rule::create(&mut f.db, &definition).unwrap();
    }
    f.import(&[(3, -1_000, "user"), (3, -2_000, "rule")]);
    let user = f.live("user");
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'user', rule_id = NULL WHERE id = ?1",
        user,
    );

    f.import(&[(3, -1_100, "user"), (3, -2_100, "rule")]);

    let category = |id: TransactionId| -> (String, String) {
        f.db.conn()
            .query_row(
                "SELECT c.key, t.category_source FROM \"transaction\" t \
                 JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    assert_eq!(
        category(f.live("user")),
        ("food".to_owned(), "user".to_owned())
    );
    assert_eq!(
        category(f.live("rule")),
        ("transport".to_owned(), "rule".to_owned())
    );
}

/// The superseded row leaves the balance, spending and reports: the new
/// one counts instead.
#[test]
fn the_balance_and_reports_count_the_new_row_only() {
    let mut f = Fixture::new();
    account::add_balance(
        &mut f.db,
        f.account,
        &EnteredBalance {
            date: day(1),
            balance: gbp(10_000),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();
    f.import(&[(3, -1_000, "tx_1")]);

    let preview = f.preview(&[(4, -1_200, "tx_1")]);
    // The preview reconciles against the balance after the correction.
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    let mut line = StatementLine::new(day(4), gbp(-1_200), "Shop tx_1");
    line.external_id = Some("tx_1".to_owned());
    statement.push(SourceLocation::Entry(1), Ok(line));
    statement
        .push_balance(crate::statement::DatedBalance {
            date: day(5),
            balance: gbp(8_800),
        })
        .unwrap();
    let with_balance = super::preview(&f.db, f.account, "s.ofx", statement).unwrap();
    assert_eq!(
        with_balance.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
    commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap();

    assert_eq!(f.movements(), [(day(4), -1_200)]);
    let reader = f.db.reader().unwrap();
    let account = reporting::balance::account(&reader, f.account).unwrap();
    let balance = reporting::balance::balances_at(&reader, &account, &[day(5)]).unwrap();
    assert_eq!(balance[0].as_ref().unwrap().balance, gbp(8_800));
    let november = Period::new(2026, 11).unwrap();
    let summary = reporting::summary::monthly_summary(&reader, november, november).unwrap();
    assert_eq!(
        summary.items[0].spending.get(Currency::GBP),
        Some(gbp(1_200))
    );
}

// Versions --------------------------------------------------------------------

#[test]
fn a_third_version_supersedes_the_second_and_older_versions_are_already_imported() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let first = f.live("tx_1");
    f.import(&[(3, -1_200, "tx_1")]);
    let second = f.live("tx_1");

    let third = f.preview(&[(4, -1_250, "tx_1")]);
    assert_eq!(correction(&outcomes(&third)[0]).existing, second);
    commit(&mut f.db, &third, third.needs_confirmation()).unwrap();
    let live = f.live("tx_1");

    assert_eq!(f.exclusion(first).unwrap().superseded_by, Some(second));
    assert_eq!(f.exclusion(second).unwrap().superseded_by, Some(live));
    assert_eq!(f.exclusion(live), None);
    // An older export never undoes a correction: its version is known.
    let older = f.preview(&[(3, -1_000, "tx_1"), (3, -1_200, "tx_1")]);
    assert_eq!(
        outcomes(&older)[0],
        RowOutcome::AlreadyImported {
            transaction: first,
            matched_by: MatchedBy::SourceId
        }
    );
    assert!(f.preview(&[(3, -1_000, "tx_1")]).is_nothing_new());
    assert!(f.preview(&[(3, -1_200, "tx_1")]).is_nothing_new());
    assert_eq!(f.movements(), [(day(4), -1_250)]);
}

// Undo ------------------------------------------------------------------------

#[test]
fn undoing_the_correction_restores_the_stored_row_exactly() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    f.sql(USER_FIELDS, old);
    let before = f.row(old);
    let correction = f.import(&[(3, -1_200, "tx_1")]);

    let preview = undo_preview(&f.db, correction.batch).unwrap();
    assert_eq!(preview.restored, [old]);
    assert_eq!(preview.transactions, 1);
    assert_eq!(preview.superseded, 0);
    undo(&mut f.db, correction.batch).unwrap();

    assert_eq!(f.row(old), before);
    assert_eq!(f.live("tx_1"), old);
    assert_eq!(f.movements(), [(day(3), -1_000)]);
    // The correction can come in again.
    assert!(matches!(
        outcomes(&f.preview(&[(3, -1_200, "tx_1")]))[0],
        RowOutcome::Corrects(_)
    ));
}

#[test]
fn undoing_a_correction_meets_the_replaced_rows_lock() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let correction = f.import(&[(6, -1_000, "tx_1")]);
    // Locking through day 4 covers the replaced row only.
    f.lock(4);
    assert!(matches!(
        undo_preview(&f.db, correction.batch),
        Err(Error::Locked { through }) if through == day(4)
    ));
}

/// Undoing the older import removes its superseded row with the rest of
/// the batch; the correction stays, as an ordinary row.
#[test]
fn undoing_the_older_import_keeps_the_correction() {
    let mut f = Fixture::new();
    let first = f.import(&[(3, -1_000, "tx_1"), (3, -500, "tx_2")]);
    let old = f.live("tx_1");
    let second = f.import(&[(3, -1_200, "tx_1")]);
    let new = f.live("tx_1");

    let preview = undo_preview(&f.db, first.batch).unwrap();
    assert_eq!(preview.transactions, 2);
    assert_eq!(preview.superseded, 1);
    assert_eq!(preview.restored, []);
    undo(&mut f.db, first.batch).unwrap();

    let gone: i64 = f.one("SELECT COUNT(*) FROM \"transaction\" WHERE id = ?1", old);
    assert_eq!(gone, 0);
    assert_eq!(f.live("tx_1"), new);
    assert_eq!(f.exclusion(new), None);
    assert_eq!(f.movements(), [(day(3), -1_200)]);

    // Then the later one: nothing left to restore.
    assert_eq!(
        undo_preview(&f.db, second.batch).unwrap().restored,
        Vec::<TransactionId>::new()
    );
    undo(&mut f.db, second.batch).unwrap();
    let left: i64 =
        f.db.conn()
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
            .unwrap();
    assert_eq!(left, 0);
}

/// The middle of three versions: undoing its import would bring the first
/// back beside the third, so it waits for the third's import to go first.
#[test]
fn undoing_a_correction_that_was_corrected_again_waits_for_the_later_import() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let first = f.live("tx_1");
    let middle = f.import(&[(3, -1_200, "tx_1")]);
    let last = f.import(&[(4, -1_250, "tx_1")]);
    let live = f.live("tx_1");

    for refused in [
        undo_preview(&f.db, middle.batch),
        undo(&mut f.db, middle.batch),
    ] {
        assert!(
            matches!(refused, Err(Error::CorrectedAgain { later }) if later == last.batch),
            "{refused:?}"
        );
    }
    assert_eq!(f.live("tx_1"), live);

    // In order, each undo restores the version before it.
    undo(&mut f.db, last.batch).unwrap();
    undo(&mut f.db, middle.batch).unwrap();
    assert_eq!(f.live("tx_1"), first);
    assert_eq!(f.exclusion(first), None);
    assert_eq!(f.movements(), [(day(3), -1_000)]);
}

// Scheduled payments ----------------------------------------------------------

/// A matched stored row is corrected, and the match moves to the
/// correction as it was, even outside the schedule's tolerance: the payment
/// is the same.
#[test]
fn a_correction_takes_the_stored_rows_schedule_match() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    let schedule = f.schedule("Shop", -1_000);
    f.match_on(schedule, old, "user", "2026-11-04T09:30:00.000Z");

    let preview = f.preview(&[(5, -1_350, "tx_1")]);
    assert_eq!(correction(&outcomes(&preview)[0]).existing, old);
    let summary = f.accept(&preview);

    let new = f.live("tx_1");
    assert_eq!(summary.corrections[0].by, new);
    // Moved, not a new match.
    assert_eq!(summary.schedule_matches, 0);
    assert_eq!(
        f.matches(),
        [(
            schedule,
            "2026-11-03".to_owned(),
            new,
            "user".to_owned(),
            "2026-11-04T09:30:00.000Z".to_owned()
        )]
    );
}

/// A correction whose stored row had no match is matched like a new row:
/// a pending row for 0 that posts as the scheduled amount fulfils the
/// occurrence. Undoing the import moves that match to the restored row.
#[test]
fn the_import_matches_a_correction_whose_stored_row_had_no_match() {
    let mut f = Fixture::new();
    f.import(&[(3, 0, "tx_1")]);
    let pending = f.live("tx_1");
    let merchant: String = f.one(
        "SELECT coalesce(merchant_display, merchant) FROM \"transaction\" WHERE id = ?1",
        pending,
    );
    let schedule = f.schedule(&merchant, -1_200);
    assert!(f.matches().is_empty());

    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    let mut correction = StatementLine::new(day(3), gbp(-1_200), "Shop tx_1");
    correction.external_id = Some("tx_1".to_owned());
    statement.push(SourceLocation::Entry(1), Ok(correction));
    let december = NaiveDate::from_ymd_opt(2026, 12, 3).unwrap();
    let mut next = StatementLine::new(december, gbp(-1_200), "Shop tx_1");
    next.external_id = Some("tx_2".to_owned());
    statement.push(SourceLocation::Entry(2), Ok(next));
    let preview = preview(&f.db, f.account, "statement.ofx", statement).unwrap();
    let summary = f.accept(&preview);

    assert_eq!(summary.corrections.len(), 1);
    assert_eq!(summary.schedule_matches, 2);
    let matched = |f: &Fixture| -> Vec<_> {
        f.matches()
            .into_iter()
            .map(|(id, date, transaction, by, _)| (id, date, transaction, by))
            .collect()
    };
    assert_eq!(
        matched(&f),
        [
            (
                schedule,
                "2026-11-03".to_owned(),
                f.live("tx_1"),
                "auto".to_owned()
            ),
            (
                schedule,
                "2026-12-03".to_owned(),
                f.live("tx_2"),
                "auto".to_owned()
            ),
        ]
    );

    undo(&mut f.db, summary.batch).unwrap();

    assert_eq!(f.live("tx_1"), pending);
    assert_eq!(
        matched(&f),
        [(
            schedule,
            "2026-11-03".to_owned(),
            pending,
            "auto".to_owned()
        )]
    );
}

/// Undoing the correcting import puts the match back on the restored row
/// exactly.
#[test]
fn undoing_the_correction_gives_the_match_back_to_the_restored_row() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    let schedule = f.schedule("Shop", -1_000);
    f.match_on(schedule, old, "auto", "2026-11-03T08:00:00.000Z");
    let before = (f.row(old), f.matches());
    let correction = f.import(&[(4, -1_100, "tx_1")]);
    assert_eq!(f.matches()[0].2, f.live("tx_1"));

    undo(&mut f.db, correction.batch).unwrap();

    assert_eq!(f.live("tx_1"), old);
    assert_eq!((f.row(old), f.matches()), before);
}

/// The user may match the superseded row by hand (an excluded row can be
/// matched): it keeps that match on undo, and the correction's goes with
/// the correction, as any removed row's match does.
#[test]
fn undoing_the_correction_keeps_a_match_the_user_made_on_the_superseded_row() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    let schedule = f.schedule("Shop", -1_000);
    f.match_on(schedule, old, "auto", "2026-11-03T08:00:00.000Z");
    let correction = f.import(&[(4, -1_100, "tx_1")]);
    f.db.conn()
        .execute(
            "INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, \
             matched_by, matched_at) VALUES (?1, '2026-12-03', ?2, 'user', \
             '2026-11-05T08:00:00.000Z')",
            params![schedule, old],
        )
        .unwrap();

    undo(&mut f.db, correction.batch).unwrap();

    assert_eq!(
        f.matches(),
        [(
            schedule,
            "2026-12-03".to_owned(),
            old,
            "user".to_owned(),
            "2026-11-05T08:00:00.000Z".to_owned()
        )]
    );
}

/// Undoing the older import deletes the superseded row, which no longer
/// has the match: the correction keeps it.
#[test]
fn undoing_the_older_import_leaves_the_match_on_the_correction() {
    let mut f = Fixture::new();
    let first = f.import(&[(3, -1_000, "tx_1")]);
    let old = f.live("tx_1");
    let schedule = f.schedule("Shop", -1_000);
    f.match_on(schedule, old, "user", "2026-11-03T08:00:00.000Z");
    f.import(&[(4, -1_100, "tx_1")]);
    let new = f.live("tx_1");
    let moved = f.matches();
    assert_eq!(moved[0].2, new);

    undo(&mut f.db, first.batch).unwrap();

    let gone: i64 = f.one("SELECT COUNT(*) FROM \"transaction\" WHERE id = ?1", old);
    assert_eq!(gone, 0);
    assert_eq!(f.live("tx_1"), new);
    assert_eq!(f.matches(), moved);
}

// Only an import supersedes -------------------------------------------------

#[test]
fn the_user_cant_set_change_or_clear_superseded() {
    let mut f = Fixture::new();
    f.import(&[(3, -1_000, "tx_1"), (3, -500, "tx_2")]);
    let old = f.live("tx_1");
    let other = f.live("tx_2");
    f.import(&[(3, -1_200, "tx_1")]);

    assert!(matches!(
        exclusion::include_again(&mut f.db, old),
        Err(exclusion::Error::Superseded { id, .. }) if id == old
    ));
    assert!(matches!(
        exclusion::exclude(&mut f.db, old, ExclusionReason::Disputed, None),
        Err(exclusion::Error::Superseded { .. })
    ));
    assert!(matches!(
        exclusion::exclude(&mut f.db, other, ExclusionReason::Superseded, None),
        Err(exclusion::Error::NotChoosable)
    ));
    let tx = f.db.conn_mut().transaction().unwrap();
    let mut recorder = Recorder::new(change::ChangeKind::BulkEdit);
    assert!(matches!(
        recorder.set(&tx, old, Value::Exclusion(None)),
        Err(change::Error::Unwritable { .. })
    ));
    assert!(matches!(
        recorder.set(
            &tx,
            other,
            Value::Exclusion(Some(ExclusionValue::now(ExclusionReason::Superseded)))
        ),
        Err(change::Error::Unwritable { .. })
    ));
}
