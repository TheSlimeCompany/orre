//! `orre categorize` against scratch data dirs with synthetic rows.
//! Nothing here touches the real data dir.

use std::path::PathBuf;

use clap::Parser;
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::category::CategoryKey;
use orre_core::change;
use orre_core::db::Database;
use orre_core::id::{AccountId, TransactionId};
use orre_core::inbox::own::RULE_SUGGESTION_THRESHOLD;
use orre_core::inbox::{
    self, ApproveOptions, Filter, NewSuggestion, ORRE_SOURCE, SuggestionKind, SuggestionStatus,
};
use orre_core::link::{self, LinkKind, LinkTarget, NewLink};
use orre_core::manual::{self, ManualTransaction};
use orre_core::mcp_client::ClientName;
use orre_core::money::{Currency, Money};
use orre_core::split::{self, SplitLine};
use tempfile::TempDir;

use super::*;

struct Scratch {
    _dir: TempDir,
    path: PathBuf,
    account: AccountId,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Scratch {
            _dir: dir,
            path,
            account,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("orre").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let mut out = Vec::new();
        let result = super::run(cli.command.expect("a subcommand"), &self.path, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        self.run(args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err(&self, args: &[&str]) -> String {
        match self.run(args) {
            Ok(out) => panic!("{args:?} succeeded:\n{out}"),
            Err(error) => error,
        }
    }

    /// A manual row of `minor` pence at `merchant` on day `d`, uncategorised.
    fn row(&self, d: u32, minor: i64, merchant: &str) -> TransactionId {
        let mut entry = ManualTransaction::new(
            self.account,
            chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap(),
            Money::new(minor, Currency::GBP),
            merchant,
        );
        entry.merchant = Some(merchant.to_owned());
        manual::create(&mut self.db(), &entry).unwrap()
    }

    fn category(&self, row: TransactionId) -> Option<String> {
        manual::get(&self.db(), row)
            .unwrap()
            .category
            .map(|key| key.as_str().to_owned())
    }

    fn batches(&self) -> usize {
        change::list(&self.db()).unwrap().len()
    }

    /// Orre's own `rule` suggestions, whatever their status.
    fn orre_rules(&self) -> Vec<inbox::Suggestion> {
        inbox::list(
            &self.db(),
            &Filter {
                status: None,
                kind: Some(SuggestionKind::Rule),
            },
        )
        .unwrap()
        .into_iter()
        .filter(|s| s.source_client == ORRE_SOURCE)
        .collect()
    }
}

#[test]
fn parses_categorize() {
    let cli = Cli::try_parse_from(["orre", "categorize", "12", "13", "food.groceries"]).unwrap();
    let Some(Command::Categorize(args)) = cli.command else {
        panic!("not categorize");
    };
    assert_eq!(args.transactions, [12, 13]);
    assert_eq!(args.category, "food.groceries");
    let cli = Cli::try_parse_from(["orre", "categorize", "7", "food.groceries"]).unwrap();
    let Some(Command::Categorize(args)) = cli.command else {
        panic!("not categorize");
    };
    assert_eq!(args.transactions, [7]);

    for bad in [
        &["orre", "categorize"][..],
        &["orre", "categorize", "12"],
        &["orre", "categorize", "food.groceries"],
        &["orre", "categorize", "twelve", "food.groceries"],
    ] {
        assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn sets_the_category_as_one_change_batch() {
    let scratch = Scratch::new();
    let tesco = scratch.row(1, -2000, "Tesco");
    let aldi = scratch.row(2, -1500, "Aldi");

    let out = scratch.ok(&[
        "categorize",
        &tesco.to_string(),
        &aldi.to_string(),
        "food.groceries",
    ]);
    let batch = change::list(&scratch.db()).unwrap()[0].id;
    assert_eq!(
        out,
        format!(
            "Set the category of 2 transactions to food.groceries (change batch {batch}):\n  \
             transaction {tesco}: none -> food.groceries\n  \
             transaction {aldi}: none -> food.groceries\n"
        )
    );
    assert_eq!(scratch.category(tesco).as_deref(), Some("food.groceries"));
    assert_eq!(scratch.category(aldi).as_deref(), Some("food.groceries"));
    // No transaction text in the output.
    assert!(!out.contains("Tesco") && !out.contains("Aldi"), "{out}");

    // The batch undoes like any other.
    change::undo(&mut scratch.db(), batch).unwrap();
    assert_eq!(scratch.category(tesco), None);

    // Again, with one row already in the category.
    let out = scratch.ok(&["categorize", &tesco.to_string(), "food.eating_out"]);
    assert!(out.contains(&format!("transaction {tesco}: none -> food.eating_out")));
    let out = scratch.ok(&[
        "categorize",
        &tesco.to_string(),
        &aldi.to_string(),
        "food.eating_out",
    ]);
    assert!(
        out.contains(&format!("transaction {aldi}: none -> food.eating_out\n")),
        "{out}"
    );
    assert!(
        out.ends_with("1 other transaction already had it, set by you.\n"),
        "{out}"
    );
    let batches = scratch.batches();
    let out = scratch.ok(&["categorize", &tesco.to_string(), "food.eating_out"]);
    assert_eq!(
        out,
        "Nothing changed: the transaction already had category food.eating_out, set by you.\n"
    );
    assert_eq!(scratch.batches(), batches);
}

#[test]
fn refuses_unknown_ids_and_keys_and_writes_nothing() {
    let scratch = Scratch::new();
    let tesco = scratch.row(1, -2000, "Tesco");
    let split_row = scratch.row(2, -1000, "Market");
    split::set(
        &mut scratch.db(),
        split_row,
        &[
            SplitLine::new(Money::new(-600, Currency::GBP), None),
            SplitLine::new(Money::new(-400, Currency::GBP), None),
        ],
    )
    .unwrap();
    let tesco_id = tesco.to_string();
    let split_id = split_row.to_string();

    for (args, message) in [
        (
            vec!["categorize", &tesco_id, "9999", "food.groceries"],
            "there is no transaction 9999; nothing was changed".to_owned(),
        ),
        (
            vec!["categorize", &tesco_id, "food.nonsense"],
            "there is no category food.nonsense; nothing was changed".to_owned(),
        ),
        (
            vec!["categorize", &tesco_id, &tesco_id, "food.groceries"],
            format!("transaction {tesco} is named twice; nothing was changed"),
        ),
    ] {
        assert_eq!(scratch.err(&args), message, "{args:?}");
    }
    let bad_key = scratch.err(&["categorize", &tesco_id, "Food Groceries"]);
    assert!(
        bad_key.starts_with("\"Food Groceries\" isn't a category key")
            && bad_key.ends_with("; nothing was changed"),
        "{bad_key}"
    );
    let split = scratch.err(&["categorize", &tesco_id, &split_id, "food.groceries"]);
    assert!(split.ends_with("; nothing was changed"), "{split}");
    assert!(split.contains(&split_id), "{split}");
    assert_eq!(
        split,
        format!(
            "transaction {split_row} can't take a category: it is split, so its lines take \
             categories; nothing was changed"
        )
    );

    assert_eq!(scratch.category(tesco), None);
    assert_eq!(scratch.batches(), 0);

    // Without a database, it creates none.
    let empty = tempfile::tempdir().unwrap();
    let path = empty.path().join("orre.db");
    let cli = Cli::try_parse_from(["orre", "categorize", "1", "food.groceries"]).unwrap();
    let error = super::run(cli.command.unwrap(), &path, &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("there is no Orre database"));
    assert!(!path.exists());
}

/// Pairs `out` and `into` as a confirmed transfer, the way an approved
/// suggestion does.
fn confirm_transfer(scratch: &Scratch, out: TransactionId, into: TransactionId) {
    let mut db = scratch.db();
    let filed = inbox::file_suggestion(
        &mut db,
        &ClientName::new("claude-code").unwrap(),
        &NewSuggestion {
            kind: SuggestionKind::TransferPair,
            payload: serde_json::json!({"outflow_id": out.get(), "inflow_id": into.get()}),
            rationale: None,
        },
    )
    .unwrap();
    inbox::approve(&mut db, filed.id, ApproveOptions::default()).unwrap();
}

#[test]
fn refuses_a_transfer_leg_or_a_linked_refund_and_writes_nothing() {
    let scratch = Scratch::new();
    let tesco = scratch.row(1, -2000, "Tesco");
    // A confirmed transfer to savings.
    let savings = account::create(
        &mut scratch.db(),
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let out = scratch.row(2, -5000, "To savings");
    let mut entry = ManualTransaction::new(
        savings,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
        Money::new(5000, Currency::GBP),
        "From current",
    );
    entry.merchant = Some("From current".to_owned());
    let into = manual::create(&mut scratch.db(), &entry).unwrap();
    confirm_transfer(&scratch, out, into);
    // A confirmed refund of part of a purchase.
    let purchase = scratch.row(3, -3000, "Shop");
    let refund = scratch.row(4, 1000, "Shop");
    link::create(
        &mut scratch.db(),
        &NewLink {
            kind: LinkKind::Refund,
            inflow: refund,
            target: LinkTarget::Purchase(purchase),
            amount: Money::new(1000, Currency::GBP),
        },
    )
    .unwrap();
    let (tesco_id, out_id, into_id, refund_id) = (
        tesco.to_string(),
        out.to_string(),
        into.to_string(),
        refund.to_string(),
    );

    for (args, message) in [
        (
            vec!["categorize", &tesco_id, &out_id, "food.groceries"],
            format!(
                "transaction {out} can't take a category: it is in a confirmed transfer, \
                 which takes no category; nothing was changed"
            ),
        ),
        (
            vec!["categorize", &into_id, "income"],
            format!(
                "transaction {into} can't take a category: it is in a confirmed transfer, \
                 which takes no category; nothing was changed"
            ),
        ),
        (
            vec!["categorize", &tesco_id, &refund_id, "shopping"],
            format!(
                "transaction {refund} can't take a category: it is a linked refund, which \
                 counts in its purchase's category; nothing was changed"
            ),
        ),
    ] {
        assert_eq!(scratch.err(&args), message, "{args:?}");
    }
    assert_eq!(scratch.category(tesco), None);
    assert_eq!(scratch.category(out), None);
    assert_eq!(scratch.category(refund), None);
    assert_eq!(scratch.batches(), 0);

    // The purchase side takes one.
    let out = scratch.ok(&["categorize", &purchase.to_string(), "shopping"]);
    assert!(
        out.contains(&format!("transaction {purchase}: none -> shopping")),
        "{out}"
    );
    // No transaction text in the output.
    assert!(!out.contains("Shop"), "{out}");
}

#[test]
fn the_third_identical_edit_files_orres_rule_suggestion() {
    assert_eq!(RULE_SUGGESTION_THRESHOLD, 3);
    let scratch = Scratch::new();
    let rows: Vec<TransactionId> = (1..=3)
        .map(|d| scratch.row(d, -350, "Bean There"))
        .collect();

    for row in &rows[..2] {
        let out = scratch.ok(&["categorize", &row.to_string(), "food.eating_out"]);
        assert!(!out.contains("Orre suggested"), "{out}");
        assert!(scratch.orre_rules().is_empty());
    }

    let out = scratch.ok(&["categorize", &rows[2].to_string(), "food.eating_out"]);
    let filed = scratch.orre_rules();
    assert_eq!(filed.len(), 1);
    let suggestion = &filed[0];
    assert_eq!(suggestion.status, SuggestionStatus::Pending);
    assert_eq!(suggestion.kind, SuggestionKind::Rule);
    assert!(
        out.ends_with(&format!(
            "Orre suggested a rule for this merchant (suggestion {id}): \
             `orre inbox show {id}` shows it.\n",
            id = suggestion.id
        )),
        "{out}"
    );
    assert!(!out.contains("Bean"), "{out}");
    let inbox::Payload::Rule(rule) = &suggestion.payload else {
        panic!("not a rule: {:?}", suggestion.payload);
    };
    assert_eq!(rule.pattern().as_str(), "Bean There");
    assert_eq!(
        rule.actions(),
        [orre_core::rule::RuleAction::SetCategory(
            CategoryKey::new("food.eating_out").unwrap()
        )]
    );

    // It shows in the inbox, from Orre.
    let list = scratch.ok(&["inbox"]);
    assert!(list.contains("Orre"), "{list}");
}
