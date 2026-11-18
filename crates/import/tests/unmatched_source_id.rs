//! A source ID is checked against the row's content (ROADMAP R.3): a bank
//! that regenerates its IDs on each download, or a file imported again
//! through a mapping without its ID column, stores every overlapping row
//! twice, and the import records each pair as an `unmatched_source_id`
//! duplicate candidate.

mod common;

use coffer_core::duplicate::{self, Candidate, CandidateReason, CandidateStatus};
use coffer_core::money::Currency;
use coffer_core::statement::SourceLocation;
use coffer_import::AnyImporter;
use coffer_import::preset::Preset;

use common::*;

const FIRST_DOWNLOAD: &str = "regenerated-fitids-first.ofx";
const SECOND_DOWNLOAD: &str = "regenerated-fitids-second.ofx";
const CSV_WITH_IDS: &str = "csv/reimport-without-id.csv";

/// The pending candidates, as the stored rows of each pair, oldest first.
fn pairs(scratch: &Scratch, account: coffer_core::id::AccountId) -> Vec<(Stored, Stored)> {
    let stored = scratch.stored(account);
    let find = |id: coffer_core::id::TransactionId| {
        stored
            .iter()
            .find(|row| row.id == id.get())
            .unwrap()
            .clone()
    };
    duplicate::pending(&scratch.db, Some(account))
        .unwrap()
        .iter()
        .map(|candidate: &Candidate| {
            assert_eq!(candidate.reason, CandidateReason::UnmatchedSourceId);
            assert_eq!(candidate.status, CandidateStatus::Pending);
            (find(candidate.transaction_a), find(candidate.transaction_b))
        })
        .collect()
}

/// The second download carries the same four rows under new `FITID`s and
/// one new row: each regenerated row pairs with its first copy (the two
/// same-day coffees first with first), and the new row pairs with nothing.
#[test]
fn an_ofx_download_with_regenerated_fitids_pairs_every_overlapping_row() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);

    let first = scratch.import_batch(account, FIRST_DOWNLOAD, read_ofx(FIRST_DOWNLOAD));
    assert_eq!((first.transactions, first.duplicates), (4, 0));

    let preview = scratch.preview(account, SECOND_DOWNLOAD, read_ofx(SECOND_DOWNLOAD));
    assert!(
        outcome_kinds(&preview)
            .iter()
            .all(|(_, kind)| kind == "new"),
        "no regenerated ID matches: {:?}",
        outcome_kinds(&preview)
    );
    let second = scratch.import_batch(account, SECOND_DOWNLOAD, read_ofx(SECOND_DOWNLOAD));
    assert_eq!((second.transactions, second.duplicates), (5, 4));

    let found: Vec<(String, String)> = pairs(&scratch, account)
        .into_iter()
        .map(|(a, b)| (a.external_id.unwrap(), b.external_id.unwrap()))
        .collect();
    assert_eq!(
        found,
        [
            ("RG1-0001", "RG2-7741"),
            ("RG1-0002", "RG2-7742"),
            ("RG1-0003", "RG2-7743"),
            ("RG1-0004", "RG2-7744"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    let streaming = scratch.by_external_id(account, "RG2-7745").unwrap();
    assert!(
        duplicate::of_transaction(
            &scratch.db,
            coffer_core::id::TransactionId::new(streaming.id)
        )
        .unwrap()
        .is_empty()
    );
}

/// A CSV imported with its ID column, then again through a mapping without
/// it: the second import's rows have no ID, so nothing matches them, and
/// each pairs with its first copy.
#[test]
fn a_csv_imported_again_through_a_mapping_without_its_id_column_pairs_every_row() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);

    let with_ids = read_with("uk-example-current", CSV_WITH_IDS);
    let first = scratch.import_batch(account, "reimport-without-id.csv", with_ids);
    assert_eq!((first.transactions, first.duplicates), (4, 0));

    let without_id = Preset::from_toml(
        &mapping_toml("uk/example-current.toml")
            .replace("external_id = { column = \"Transaction ID\" }\n", "")
            .replace("uk-example-current", "uk-example-current-no-id"),
    )
    .unwrap();
    let statement = without_id
        .importer()
        .import(&fixture(CSV_WITH_IDS))
        .unwrap();
    assert!(
        statement
            .lines()
            .all(|(_, line)| line.external_id.is_none())
    );
    let second = scratch.import_batch(account, "reimport-without-id.csv", statement);
    assert_eq!((second.transactions, second.duplicates), (4, 4));

    let found: Vec<(String, Option<String>)> = pairs(&scratch, account)
        .into_iter()
        .map(|(a, b)| (a.external_id.unwrap(), b.external_id))
        .collect();
    assert_eq!(
        found,
        ["rw_0001", "rw_0002", "rw_0003", "rw_0004"].map(|id| (id.to_owned(), None))
    );
}

/// The next download repeats the two coffees under their own IDs and adds a
/// genuine third one. The repeated coffees are matched by source ID, so the
/// third pairs with neither.
#[test]
fn a_new_row_beside_rows_matched_by_source_id_pairs_with_none_of_them() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    scratch.import_batch(account, FIRST_DOWNLOAD, read_ofx(FIRST_DOWNLOAD));

    let mut next = read_ofx(FIRST_DOWNLOAD);
    let mut third_coffee = next.rows()[0].outcome.clone().unwrap();
    third_coffee.external_id = Some("RG1-0005".to_owned());
    next.push(SourceLocation::Entry(5), Ok(third_coffee));

    let summary = scratch.import_batch(account, "next.ofx", next);
    assert_eq!((summary.transactions, summary.duplicates), (1, 0));
    assert!(pairs(&scratch, account).is_empty());
}
