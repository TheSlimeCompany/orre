//! `coffer import` and the bank's corrections (R.4): the preview lists them,
//! `--yes` accepts them like flagged rows and a mismatch, and the summary
//! says what each stored row kept back. Runs against a scratch database and
//! the synthetic `fixtures/ofx/corrections-*.ofx`.

use std::path::{Path, PathBuf};

use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::TransactionId;
use coffer_core::import::Confirmations;
use coffer_core::money::{Currency, Money};
use coffer_core::split::{self, SplitLine};
use coffer_import::preset::Presets;
use tempfile::TempDir;

use super::*;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/ofx")
        .join(name)
}

/// A scratch database with one GBP current account that has imported
/// `corrections-first.ofx` (transactions 1 to 4).
struct Scratch {
    _dir: TempDir,
    db: Database,
    presets: Presets,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let mut scratch = Scratch {
            _dir: dir,
            db,
            presets: Presets::bundled().unwrap(),
        };
        scratch
            .import("corrections-first.ofx", false, false)
            .unwrap();
        scratch
    }

    fn import(&mut self, file: &str, dry_run: bool, yes: bool) -> Result<String, String> {
        let args = ImportArgs {
            file: fixture(file),
            account: None,
            preset: None,
            statement: None,
            dry_run,
            yes,
        };
        let mut out = Vec::new();
        let result = run(&mut self.db, &self.presets, &args, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }
}

const NEXT: &str = "corrections-next.ofx";

#[test]
fn corrections_are_listed_and_need_yes() {
    let mut scratch = Scratch::new();

    let out = scratch.import(NEXT, true, false).unwrap();
    assert!(
        out.contains("Rows:      1 new, 1 already imported, 0 flagged, 3 corrections\n"),
        "{out}"
    );
    assert!(
        out.contains(
            "Corrections (the bank's new version of a stored row, which is excluded as \
             superseded):\n  \
             entry 1: replaces transaction 1 (2026-11-05, -40.00 GBP)\n  \
             entry 2: replaces transaction 2 (2026-11-06, -1.00 GBP)\n  \
             entry 3: replaces transaction 3 (2026-11-07, -12.50 GBP)\n"
        ),
        "{out}"
    );
    assert!(
        out.ends_with(
            "Importing it needs --yes to accept the bank's corrections of stored rows.\n"
        ),
        "{out}"
    );

    let error = scratch.import(NEXT, false, false).unwrap_err();
    assert!(
        error.starts_with(
            "not imported: the import has the bank's corrections of stored rows (listed above)"
        ),
        "{error}"
    );
    assert!(
        error.ends_with(
            "each correction replaces its stored row, which stays excluded as superseded until \
             the import is undone"
        ),
        "{error}"
    );

    let out = scratch.import(NEXT, false, true).unwrap();
    assert!(
        out.contains("as batch 2: 4 transactions, 1 balance, 0 flags.\n"),
        "{out}"
    );
    assert!(
        out.contains(
            "Corrections: 3 stored rows replaced by the bank's new version, excluded as \
             superseded.\n"
        ),
        "{out}"
    );
    // Accepted, the file is in: importing it again changes nothing.
    let out = scratch.import(NEXT, false, false).unwrap();
    assert!(out.contains("Nothing new to import"), "{out}");
}

#[test]
fn what_a_stored_row_keeps_back_is_said_before_and_after() {
    let mut scratch = Scratch::new();
    let gbp = |minor| Money::new(minor, Currency::GBP);
    let line = |minor| SplitLine {
        amount: gbp(minor),
        category: None,
        my_share: None,
        share_label: None,
        user_note: None,
    };
    split::set(
        &mut scratch.db,
        TransactionId::new(1),
        &[line(-2_500), line(-1_500)],
    )
    .unwrap();

    let out = scratch.import(NEXT, true, false).unwrap();
    assert!(
        out.contains(
            "  entry 1: replaces transaction 1 (2026-11-05, -40.00 GBP)\n    \
             its split stays on transaction 1: the new amount doesn't fit\n"
        ),
        "{out}"
    );
    let out = scratch.import(NEXT, false, true).unwrap();
    assert!(
        out.contains("  transaction 1 keeps its split, which transaction 5 couldn't take\n"),
        "{out}"
    );
}

#[test]
fn yes_names_everything_it_accepts() {
    let all = Confirmations {
        flagged_rows: true,
        balance_mismatch: true,
        corrections: true,
    };
    assert_eq!(
        accepting(all),
        "flagged rows, a balance mismatch and the bank's corrections of stored rows"
    );
    assert_eq!(
        accepting(Confirmations {
            corrections: false,
            ..all
        }),
        "flagged rows and a balance mismatch"
    );
    assert_eq!(
        accepting(Confirmations {
            flagged_rows: false,
            corrections: false,
            ..all
        }),
        "a balance mismatch"
    );
    assert_eq!(
        yes_explained(Confirmations {
            corrections: false,
            ..all
        }),
        "the flagged rows stay out and are recorded with the import, and the mismatch stays \
         flagged on the account"
    );
    assert_eq!(
        yes_explained(all),
        "the flagged rows stay out and are recorded with the import, the mismatch stays \
         flagged on the account, and each correction replaces its stored row, which stays \
         excluded as superseded until the import is undone"
    );
    assert!(needs_any(Confirmations {
        corrections: true,
        ..Confirmations::default()
    }));
}

/// `coffer undo-import` says what happens to the rows a correction
/// replaced: undoing the correcting import brings them back, and undoing
/// the earlier one takes them with it while the corrections stay.
#[test]
fn undo_says_what_happens_to_corrected_rows() {
    let mut scratch = Scratch::new();
    scratch.import(NEXT, false, true).unwrap();
    let undo = |db: &mut Database, batch: i64| {
        let args = super::super::UndoImportArgs { batch, yes: false };
        let mut out = Vec::new();
        super::super::undo::run(db, &args, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };

    let out = undo(&mut scratch.db, 2);
    assert!(
        out.contains(
            "3 earlier transactions replaced by this import's corrections come back as they \
             were.\n"
        ),
        "{out}"
    );
    let out = undo(&mut scratch.db, 1);
    assert!(
        out.contains(
            "3 rows a later import corrected go too; the corrections stay as ordinary \
             transactions.\n"
        ),
        "{out}"
    );
}
