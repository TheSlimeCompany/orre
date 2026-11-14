//! Tests for what the Rules view (M3.13) adds: editing a rule in place, its
//! origin, drafts previewed and saved, the overview's "decides today" and
//! "never reached", and the rule proposed after a category edit.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::category::CategoryKey;
use crate::change::{self, ChangeKind};
use crate::db::Database;
use crate::id::{AccountId, RuleId, TransactionId};
use crate::import::{self, Committed};
use crate::inbox::own::recategorize;
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn rule_on(text: &str, category: &str) -> RuleDefinition {
    RuleDefinition::new(
        MatchField::Merchant,
        Pattern::new(MatchKind::Contains, text).unwrap(),
        AmountBounds::ANY,
        None,
        vec![RuleAction::SetCategory(key(category))],
    )
    .unwrap()
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn rule(&mut self, text: &str, category: &str) -> RuleId {
        let last = list(&self.db).unwrap().len();
        create_at(&mut self.db, &rule_on(text, category), last).unwrap()
    }

    fn import(&mut self, rows: &[(u32, i64, &str)]) -> Vec<TransactionId> {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (index, (d, minor, merchant)) in rows.iter().enumerate() {
            let mut line = StatementLine::new(
                day(*d),
                Money::new(*minor, Currency::GBP),
                format!("CARD {merchant}"),
            );
            line.merchant = Some((*merchant).to_owned());
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        let preview = import::preview(&self.db, self.account, "s.csv", statement).unwrap();
        let Committed::Batch(summary) =
            import::commit(&mut self.db, &preview, preview.needs_confirmation()).unwrap()
        else {
            panic!("expected a batch");
        };
        let mut statement = self
            .db
            .conn()
            .prepare("SELECT id FROM \"transaction\" WHERE batch_id = ?1 ORDER BY id")
            .unwrap();
        statement
            .query_map([summary.batch], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn category(&self, id: TransactionId) -> Option<String> {
        self.db
            .conn()
            .query_row(
                "SELECT c.key FROM \"transaction\" t LEFT JOIN category c \
                 ON c.id = t.category_id WHERE t.id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }
}

// Update and origin ----------------------------------------------------------------

#[test]
fn update_keeps_the_place_and_doesnt_touch_history() {
    let mut f = Fixture::new();
    let first = f.rule("TFL", "transport");
    let second = f.rule("UBER", "transport");
    let rows = f.import(&[(1, -500, "UBER TRIP")]);
    assert_eq!(f.category(rows[0]).as_deref(), Some("transport"));

    update(&mut f.db, second, &rule_on("UBER", "food")).unwrap();
    let rules = list(&f.db).unwrap();
    assert_eq!(
        rules.iter().map(StoredRule::id).collect::<Vec<_>>(),
        [first, second]
    );
    assert_eq!(rules[1].definition(), &rule_on("UBER", "food"));
    // Rule edits don't re-run on history (decision record "rules", point 4).
    assert_eq!(f.category(rows[0]).as_deref(), Some("transport"));

    assert!(matches!(
        update(&mut f.db, second, &rule_on("UBER", "no_such")),
        Err(StoreError::UnknownCategory(_))
    ));
    assert!(matches!(
        update(&mut f.db, RuleId::new(999), &rule_on("X", "food")),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn a_rule_records_where_it_came_from() {
    let mut f = Fixture::new();
    let plain = f.rule("TFL", "transport");
    let edit = create_from(
        &mut f.db,
        &rule_on("VINTED", "shopping"),
        0,
        RuleOrigin::Edit,
    )
    .unwrap();
    assert_eq!(get(&f.db, plain).unwrap().origin(), Some(RuleOrigin::New));
    let stored = get(&f.db, edit).unwrap();
    assert_eq!(stored.origin(), Some(RuleOrigin::Edit));
    assert!(stored.created_at().ends_with('Z'));
    assert_eq!(list(&f.db).unwrap()[0].id(), edit);
}

#[test]
fn an_apply_batch_names_its_rule() {
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "UBER TRIP")]);
    let rule = f.rule("UBER", "transport");
    let preview = preview_apply(&f.db, rule).unwrap();
    let applied = commit_apply(&mut f.db, &preview, &BTreeSet::new()).unwrap();
    let batch = change::get(&f.db, applied.batch.unwrap()).unwrap();
    assert_eq!(batch.kind, ChangeKind::RuleApply);
    assert_eq!(batch.rule, Some(rule));

    // A user's edit names none.
    let edit = recategorize(&mut f.db, &[(rows[0], key("food"))])
        .unwrap()
        .unwrap();
    assert_eq!(change::get(&f.db, edit).unwrap().rule, None);
}

// Drafts -------------------------------------------------------------------------

#[test]
fn a_draft_previews_without_saving_then_saves_and_applies() {
    let mut f = Fixture::new();
    let broad = f.rule("SHOP", "other");
    let rows = f.import(&[
        (1, -500, "VINTED *A1"),
        (2, -700, "VINTED *B2"),
        (3, -900, "TESCO"),
    ]);
    recategorize(&mut f.db, &[(rows[1], key("food"))]).unwrap();
    let draft = Draft {
        definition: rule_on("VINTED", "shopping"),
        position: 0,
        origin: RuleOrigin::Edit,
    };

    let preview = preview_draft(&f.db, &draft).unwrap();
    assert_eq!(preview.matches, 2);
    assert!(preview.overlaps.is_empty());
    assert_eq!(preview.preview.rows.len(), 2);
    assert_eq!(preview.preview.automatic_rows().count(), 1);
    assert_eq!(preview.preview.user_set_rows().count(), 1);
    // Nothing was stored.
    assert_eq!(list(&f.db).unwrap().len(), 1);

    let saved = save_draft(
        &mut f.db,
        &draft,
        Some((&preview.preview, &BTreeSet::from([rows[1]]))),
    )
    .unwrap();
    let rules = list(&f.db).unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].id(), saved.rule);
    assert_eq!(rules[1].id(), broad);
    assert_eq!(rules[0].origin(), Some(RuleOrigin::Edit));
    assert_eq!(saved.applied.unwrap().transactions, 2);
    assert_eq!(f.category(rows[0]).as_deref(), Some("shopping"));
    assert_eq!(f.category(rows[1]).as_deref(), Some("shopping"));
    assert_eq!(f.category(rows[2]), None);
    let batch = change::get(&f.db, saved.applied.unwrap().batch.unwrap()).unwrap();
    assert_eq!(batch.rule, Some(saved.rule));
}

#[test]
fn a_draft_can_be_saved_alone_and_lists_its_overlaps() {
    let mut f = Fixture::new();
    let uber = f.rule("UBER", "transport");
    let _tfl = f.rule("TFL", "transport");
    let rows = f.import(&[(1, -500, "UBER EATS"), (2, -300, "TFL")]);
    let draft = Draft {
        definition: rule_on("UBER EATS", "food"),
        position: 99,
        origin: RuleOrigin::New,
    };
    let preview = preview_draft(&f.db, &draft).unwrap();
    assert_eq!(preview.matches, 1);
    assert_eq!(preview.overlaps, [uber]);
    // Last, under UBER, it wins nothing.
    assert!(preview.preview.is_empty());

    let saved = save_draft(&mut f.db, &draft, None).unwrap();
    assert_eq!(saved.applied, None);
    assert_eq!(list(&f.db).unwrap().last().unwrap().id(), saved.rule);
    assert_eq!(f.category(rows[0]).as_deref(), Some("transport"));
}

#[test]
fn a_stale_draft_preview_saves_nothing() {
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "VINTED *A1")]);
    let draft = Draft {
        definition: rule_on("VINTED", "shopping"),
        position: 0,
        origin: RuleOrigin::New,
    };
    let preview = preview_draft(&f.db, &draft).unwrap();
    f.import(&[(2, -600, "VINTED *C3")]);
    assert!(matches!(
        save_draft(
            &mut f.db,
            &draft,
            Some((&preview.preview, &BTreeSet::new()))
        ),
        Err(ApplyError::Stale)
    ));
    assert!(list(&f.db).unwrap().is_empty());
    assert_eq!(f.category(rows[0]), None);
}

// Overview -----------------------------------------------------------------------

#[test]
fn the_overview_counts_what_each_rule_decides() {
    let mut f = Fixture::new();
    let uber = f.rule("UBER", "transport");
    let trip = f.rule("UBER TRIP", "travel");
    let tfl = f.rule("TFL", "transport");
    let unused = f.rule("NEVER SEEN", "other");
    f.import(&[
        (1, -500, "UBER TRIP"),
        (4, -600, "UBER TRIP"),
        (2, -700, "UBER EATS"),
        (3, -300, "TFL TRAVEL"),
    ]);

    let rules = overview(&f.db).unwrap();
    assert_eq!(
        rules.iter().map(|r| r.rule.id()).collect::<Vec<_>>(),
        [uber, trip, tfl, unused]
    );
    assert_eq!((rules[0].matches, rules[0].decides), (3, 3));
    assert_eq!(rules[0].last, Some(day(4)));
    assert!(!rules[0].never_reached());

    // UBER TRIP always loses to UBER above it.
    assert_eq!((rules[1].matches, rules[1].decides), (2, 0));
    assert!(rules[1].never_reached());
    assert_eq!(
        rules[1].caught_by,
        Some(Catch {
            rule: uber,
            rows: 2
        })
    );
    assert_eq!(rules[1].last, None);

    assert_eq!((rules[2].decides, rules[2].last), (1, Some(day(3))));
    assert_eq!((rules[3].matches, rules[3].decides), (0, 0));
    assert!(!rules[3].never_reached());
    assert_eq!(rules[3].caught_by, None);
}

// Proposal -----------------------------------------------------------------------

#[test]
fn the_stem_cuts_the_reference() {
    assert_eq!(stem("VINTED *KX8F2Q").as_deref(), Some("VINTED"));
    assert_eq!(stem("TESCO STORES 3297").as_deref(), Some("TESCO STORES"));
    assert_eq!(stem("Pret A Manger").as_deref(), Some("Pret A Manger"));
    assert_eq!(
        stem("  AMZN   MKTP UK*2X4 ").as_deref(),
        Some("AMZN MKTP UK")
    );
    assert_eq!(stem("AB *123"), None);
    assert_eq!(stem("3UK"), None);
    assert_eq!(stem(""), None);
}

#[test]
fn a_category_edit_proposes_a_rule_with_its_counts() {
    let mut f = Fixture::new();
    let rows = f.import(&[
        (1, -500, "VINTED *A1"),
        (2, -700, "VINTED *B2"),
        (3, -900, "VINTED *C3"),
        (4, -100, "TESCO"),
    ]);
    recategorize(&mut f.db, &[(rows[1], key("food"))]).unwrap();
    // Before the edit there's nothing to propose: the row's category isn't
    // the user's.
    assert_eq!(propose(&f.db, rows[2]).unwrap(), None);
    recategorize(&mut f.db, &[(rows[2], key("shopping"))]).unwrap();

    let proposal = propose(&f.db, rows[2]).unwrap().unwrap();
    assert_eq!(proposal.definition, rule_on("VINTED", "shopping"));
    assert_eq!(proposal.category, key("shopping"));
    assert_eq!(proposal.matches, 2);
    assert_eq!(proposal.uncategorized, 1);
    assert_eq!(proposal.user_set, 1);

    // "Not now" is remembered for the pattern, in any case.
    dismiss(&mut f.db, &rule_on("vinted", "food")).unwrap();
    dismiss(&mut f.db, &rule_on("vinted", "food")).unwrap();
    assert_eq!(propose(&f.db, rows[2]).unwrap(), None);
}

#[test]
fn no_proposal_when_a_rule_decides_or_nothing_else_matches() {
    let mut f = Fixture::new();
    let rows = f.import(&[
        (1, -500, "UBER TRIP"),
        (2, -300, "UBER EATS"),
        (3, -5, "ZEN"),
    ]);
    recategorize(&mut f.db, &[(rows[2], key("food"))]).unwrap();
    // Nothing else matches ZEN.
    assert_eq!(propose(&f.db, rows[2]).unwrap(), None);

    f.rule("UBER", "transport");
    recategorize(&mut f.db, &[(rows[0], key("travel"))]).unwrap();
    // A rule decides the row's category today.
    assert_eq!(propose(&f.db, rows[0]).unwrap(), None);

    // The same rule already stored proposes nothing either.
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "VINTED *A"), (2, -300, "VINTED *B")]);
    recategorize(&mut f.db, &[(rows[0], key("shopping"))]).unwrap();
    assert!(propose(&f.db, rows[0]).unwrap().is_some());
    let last = list(&f.db).unwrap().len();
    create_at(&mut f.db, &rule_on("VINTED", "shopping"), last).unwrap();
    assert_eq!(propose(&f.db, rows[0]).unwrap(), None);
}

// Change history -----------------------------------------------------------------

#[test]
fn history_describes_each_batch() {
    use crate::change::{Field, Shown, Summary, history, history_items};

    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "VINTED *A1"), (2, -700, "VINTED *B2")]);
    let edit = recategorize(&mut f.db, &[(rows[0], key("food")), (rows[1], key("food"))])
        .unwrap()
        .unwrap();
    let rule = create_from(
        &mut f.db,
        &rule_on("VINTED", "shopping"),
        0,
        RuleOrigin::New,
    )
    .unwrap();
    let preview = preview_apply(&f.db, rule).unwrap();
    let applied = commit_apply(&mut f.db, &preview, &BTreeSet::from([rows[0]]))
        .unwrap()
        .batch
        .unwrap();

    let entries = history(&f.db).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].batch.id, applied);
    assert_eq!(
        entries[0].summary,
        Summary::Rule {
            rule: Some(rule),
            definition: Some(rule_on("VINTED", "shopping")),
        }
    );
    assert_eq!(entries[1].batch.id, edit);
    assert_eq!(entries[1].summary, Summary::Category(Some(key("food"))));

    let items = history_items(&f.db, applied).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].transaction, rows[0]);
    assert_eq!(items[0].name, "VINTED *A1");
    assert_eq!(items[0].date, day(1));
    assert_eq!(items[0].field, Field::Category);
    assert_eq!(items[0].old, Shown::Category(Some(key("food"))));
    assert_eq!(items[0].new, Shown::Category(Some(key("shopping"))));

    // A deleted rule's batch keeps its ID but loses its text.
    delete(&mut f.db, rule).unwrap();
    assert_eq!(
        history(&f.db).unwrap()[0].summary,
        Summary::Rule {
            rule: Some(rule),
            definition: None,
        }
    );
    change::undo(&mut f.db, applied).unwrap();
    assert_eq!(
        history(&f.db).unwrap()[0].batch.status,
        change::BatchStatus::Undone
    );
}

// Merchant names -----------------------------------------------------------------

#[test]
fn the_names_overview_counts_derived_names_only() {
    use crate::merchant::{self, AliasDefinition, AliasKind, DisplayName};

    let mut f = Fixture::new();
    let rows = f.import(&[
        (1, -500, "AMZN MKTP UK*2X4"),
        (3, -700, "AMZN MKTP DE*7Q1"),
        (2, -900, "AMZN MKTP UK*2X4"),
        (4, -100, "TESCO"),
    ]);
    let (amazon, _) = merchant::add(
        &mut f.db,
        &AliasDefinition::new(
            AliasKind::Prefix,
            "AMZN MKTP",
            DisplayName::new("Amazon Marketplace").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    merchant::add(
        &mut f.db,
        &AliasDefinition::new(AliasKind::Exact, "NOWHERE", DisplayName::new("A").unwrap()).unwrap(),
    )
    .unwrap();
    // A name the user typed isn't the alias's.
    merchant::set_user_name(&mut f.db, rows[2], &DisplayName::new("Mine").unwrap()).unwrap();

    let names = merchant::overview(&f.db).unwrap();
    assert_eq!(names.len(), 2);
    // Sorted by the name they give.
    assert_eq!(names[0].alias.definition().name().as_str(), "A");
    assert_eq!(names[0].matches, 0);
    let summary = &names[1];
    assert_eq!(summary.alias.id(), amazon);
    assert_eq!(summary.matches, 2);
    assert_eq!(summary.last, Some(day(3)));
    assert!(summary.created_at.ends_with('Z'));
    let texts: Vec<_> = summary
        .catches
        .iter()
        .map(|c| (c.text.as_str(), c.transactions, c.last))
        .collect();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0].2, day(3));
    assert_eq!(texts[1].2, day(1));
}

#[test]
fn preview_rows_say_now_and_after_newest_first() {
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "VINTED *A1"), (3, -700, "VINTED *B2")]);
    recategorize(&mut f.db, &[(rows[0], key("food"))]).unwrap();
    let draft = Draft {
        definition: rule_on("VINTED", "shopping"),
        position: 0,
        origin: RuleOrigin::New,
    };
    let preview = preview_draft(&f.db, &draft).unwrap();
    let listed = preview_rows(&f.db, &preview.preview).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].transaction, rows[1]);
    assert_eq!(listed[0].name, "VINTED *B2");
    assert_eq!(listed[0].date, day(3));
    assert_eq!(listed[0].amount, Money::new(-700, Currency::GBP));
    assert_eq!(listed[0].now, None);
    assert_eq!(listed[0].after, PreviewAfter::Category(key("shopping")));
    assert!(!listed[0].user_set);
    assert_eq!(listed[1].now, Some(key("food")));
    assert!(listed[1].user_set);
    assert_eq!(listed[1].account, f.account);
}

#[test]
fn rule_from_needs_only_a_category() {
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -500, "VINTED *A1"), (2, -5, "ZEN")]);
    assert_eq!(rule_from(&f.db, rows[0]).unwrap(), None);
    f.rule("VINTED", "shopping");
    // Imported after the rule: a rule set its category, and it still offers one.
    let rows = f.import(&[(3, -700, "VINTED *B2")]);
    assert_eq!(
        rule_from(&f.db, rows[0]).unwrap(),
        Some(rule_on("VINTED", "shopping"))
    );
}
