//! Tests for the categories editor's core: keys from labels, adding,
//! renaming, hiding, deleting and the overview, on scratch databases.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::category::DEFAULT_CATEGORIES;
use crate::rule::{
    self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition, Share,
    SplitLine, SplitTemplate,
};

struct Scratch {
    _dir: TempDir,
    db: Database,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Self { _dir: dir, db }
    }

    fn tree(&self) -> CategoryTree {
        self.db.categories().unwrap()
    }

    fn id(&self, text: &str) -> i64 {
        self.tree().get(&key(text)).unwrap().id()
    }

    fn account(&self) -> i64 {
        self.db
            .conn()
            .execute(
                "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP')",
                [],
            )
            .unwrap();
        self.db.conn().last_insert_rowid()
    }

    /// A manual transaction of `minor` pence, with no category.
    fn row(&self, account: i64, minor: i64) -> i64 {
        self.db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-11-03', ?2, 'GBP', 'x', 'X', 'manual')",
                params![account, minor],
            )
            .unwrap();
        self.db.conn().last_insert_rowid()
    }

    fn add(&mut self, label: &str, placement: Placement) -> Category {
        add(
            &mut self.db,
            &NewCategory {
                label: label.to_owned(),
                placement,
            },
        )
        .unwrap()
    }
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn under(text: &str) -> Placement {
    Placement::Under(key(text))
}

#[test]
fn keys_come_from_labels() {
    let free = |_: &CategoryKey| false;
    let derived = |label: &str, parent: Option<&str>| {
        derive_key(label, parent.map(key).as_ref(), free)
            .as_str()
            .to_owned()
    };
    assert_eq!(derived("Coffee", Some("food")), "food.coffee");
    assert_eq!(derived("  Side  projects! ", None), "side_projects");
    assert_eq!(derived("Kids' clubs & camps", None), "kids_clubs_camps");
    assert_eq!(derived("2027 Trip", Some("travel")), "travel.2027_trip");
    assert_eq!(derived("Café", None), "caf");
    assert_eq!(derived("☕ ☕", None), "custom");
    assert_eq!(derived("—", Some("food")), "food.custom");

    // A numeric suffix while the key is taken.
    let taken = ["coffee", "coffee_2"].map(key);
    assert_eq!(
        derive_key("Coffee", None, |k| taken.contains(k)).as_str(),
        "coffee_3"
    );
}

#[test]
fn a_new_category_takes_a_free_key_and_its_groups_kind() {
    let mut scratch = Scratch::new();
    let coffee = scratch.add("Coffee", under("food"));
    assert_eq!(coffee.key().as_str(), "food.coffee");
    assert_eq!(coffee.kind(), CategoryKind::Spending);
    assert_eq!(coffee.parent_id(), Some(scratch.id("food")));
    assert!(!coffee.is_hidden());

    let bonus = scratch.add("Bonus", under("income"));
    assert_eq!(bonus.kind(), CategoryKind::Income, "the group's kind");

    // Keys are free in their own place: `groceries` as a group isn't
    // `food.groceries`.
    let pets = scratch.add("Family pets!", under("travel"));
    assert_eq!(pets.key().as_str(), "travel.family_pets");
    let groceries = scratch.add("Groceries", Placement::Group(CategoryKind::Spending));
    assert_eq!(groceries.key().as_str(), "groceries");
    let food = scratch.add("Food!", under("other"));
    assert_eq!(food.key().as_str(), "other.food");
    let isa = scratch.add("ISA", Placement::Group(CategoryKind::Transfer));
    assert_eq!(isa.key().as_str(), "isa");
    assert_eq!(isa.kind(), CategoryKind::Transfer);
    assert!(isa.is_group());
    // `transfer` is a default group's key ("Transfers"), so a new group
    // labelled "Transfer" gets a suffix.
    let transfer = scratch.add("Transfer", Placement::Group(CategoryKind::Transfer));
    assert_eq!(transfer.key().as_str(), "transfer_2");

    // A label may repeat in another group ("Hobbies" is under
    // Entertainment too).
    let hobbies = scratch.add("Hobbies", under("family"));
    assert_eq!(hobbies.key().as_str(), "family.hobbies");

    assert_eq!(scratch.tree().len(), DEFAULT_CATEGORIES.len() + 8);
}

#[test]
fn adding_is_refused_for_bad_labels_and_places() {
    let mut scratch = Scratch::new();
    let refused = |scratch: &mut Scratch, label: &str, placement: Placement| {
        add(
            &mut scratch.db,
            &NewCategory {
                label: label.to_owned(),
                placement,
            },
        )
        .unwrap_err()
    };
    assert!(matches!(
        refused(&mut scratch, "   ", under("food")),
        Error::BlankLabel
    ));
    assert!(matches!(
        refused(&mut scratch, "groceries", under("food")),
        Error::LabelTaken { .. }
    ));
    assert!(matches!(
        refused(
            &mut scratch,
            " FOOD ",
            Placement::Group(CategoryKind::Spending)
        ),
        Error::LabelTaken { .. }
    ));
    assert!(matches!(
        refused(&mut scratch, "Fruit", under("food.groceries")),
        Error::NotAGroup { .. }
    ));
    assert!(matches!(
        refused(&mut scratch, "Fruit", under("pets")),
        Error::NotFound(_)
    ));
    assert_eq!(scratch.tree().len(), DEFAULT_CATEGORIES.len());
}

#[test]
fn renaming_changes_the_label_only() {
    let mut scratch = Scratch::new();
    let before = scratch.tree().get(&key("food.groceries")).cloned().unwrap();
    rename(&mut scratch.db, &key("food.groceries"), "  Food shop ").unwrap();
    let after = scratch.tree().get(&key("food.groceries")).cloned().unwrap();
    assert_eq!(after.label(), "Food shop");
    assert_eq!(after.id(), before.id());
    assert_eq!(after.kind(), before.kind());

    // Its own label again, or another case of it, is fine.
    rename(&mut scratch.db, &key("food.groceries"), "food SHOP").unwrap();
    assert_eq!(
        scratch.tree().get(&key("food.groceries")).unwrap().label(),
        "food SHOP"
    );
    assert!(matches!(
        rename(&mut scratch.db, &key("food.groceries"), "takeaways"),
        Err(Error::LabelTaken { .. })
    ));
    assert!(matches!(
        rename(&mut scratch.db, &key("food.groceries"), ""),
        Err(Error::BlankLabel)
    ));
    assert!(matches!(
        rename(&mut scratch.db, &key("food.fruit"), "Fruit"),
        Err(Error::NotFound(_))
    ));
    // A subcategory may share a label with a category in another group.
    rename(&mut scratch.db, &key("food.groceries"), "Gifts").unwrap();
}

#[test]
fn hiding_takes_a_category_out_of_pickers_only() {
    let mut scratch = Scratch::new();
    let visible = |scratch: &Scratch| scratch.tree().visible().count();
    let all = DEFAULT_CATEGORIES.len();
    assert_eq!(visible(&scratch), all);

    set_hidden(&mut scratch.db, &key("food.takeaway"), true).unwrap();
    let tree = scratch.tree();
    assert!(tree.get(&key("food.takeaway")).unwrap().is_hidden());
    assert!(!tree.is_visible(&key("food.takeaway")));
    assert!(tree.is_visible(&key("food.groceries")));
    assert_eq!(tree.iter().count(), all, "still in the tree");
    assert_eq!(visible(&scratch), all - 1);

    // A hidden group hides its subcategories; each keeps its own flag.
    set_hidden(&mut scratch.db, &key("food"), true).unwrap();
    let tree = scratch.tree();
    let food = tree.group_of(&key("food")).unwrap();
    assert!(!tree.is_visible(&key("food.groceries")));
    assert!(!tree.get(&key("food.groceries")).unwrap().is_hidden());
    assert_eq!(visible(&scratch), all - 1 - food.children().len());

    set_hidden(&mut scratch.db, &key("food"), false).unwrap();
    let tree = scratch.tree();
    assert!(tree.is_visible(&key("food.groceries")));
    assert!(!tree.is_visible(&key("food.takeaway")), "kept as it was");
    set_hidden(&mut scratch.db, &key("food.takeaway"), false).unwrap();
    assert_eq!(visible(&scratch), all);

    assert!(matches!(
        set_hidden(&mut scratch.db, &key("food.fruit"), true),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn deleting_is_refused_while_anything_uses_the_category() {
    let mut scratch = Scratch::new();
    let account = scratch.account();
    let in_use = |scratch: &mut Scratch, text: &str| match delete(&mut scratch.db, &key(text)) {
        Err(Error::InUse { usage, .. }) => usage,
        other => panic!("{text} wasn't refused: {other:?}"),
    };

    // A transaction.
    scratch.add("Coffee", under("food"));
    let row = scratch.row(account, -350);
    scratch
        .db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user' WHERE id = ?2",
            params![scratch.id("food.coffee"), row],
        )
        .unwrap();
    let usage = in_use(&mut scratch, "food.coffee");
    assert_eq!(
        usage,
        CategoryUsage {
            transactions: 1,
            ..CategoryUsage::default()
        }
    );
    assert_eq!(usage.to_string(), "1 transaction");

    // A split line.
    scratch.add("Snacks", under("food"));
    let split = scratch.row(account, -1_000);
    for (line, minor, category) in [(1, -600, "food.snacks"), (2, -400, "food.groceries")] {
        scratch
            .db
            .conn()
            .execute(
                "INSERT INTO transaction_split (transaction_id, line_no, amount_minor, category_id) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![split, line, minor, scratch.id(category)],
            )
            .unwrap();
    }
    assert_eq!(in_use(&mut scratch, "food.snacks").split_lines, 1);

    // A rule: by `set_category`, and by a split template's line.
    scratch.add("Lunch", under("food"));
    scratch.add("Brunch", under("food"));
    let pattern = |text: &str| Pattern::new(MatchKind::Contains, text).unwrap();
    rule::create(
        &mut scratch.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            pattern("PRET"),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SetCategory(key("food.lunch"))],
        )
        .unwrap(),
    )
    .unwrap();
    rule::create(
        &mut scratch.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            pattern("CAFE"),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SplitTemplate(
                SplitTemplate::new(vec![
                    SplitLine::new(key("food.brunch"), Share::new(5_000).unwrap()),
                    SplitLine::new(key("food.lunch"), Share::new(5_000).unwrap()),
                ])
                .unwrap(),
            )],
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(in_use(&mut scratch, "food.lunch").rules, 2);
    assert_eq!(in_use(&mut scratch, "food.brunch").rules, 1);

    // A budget.
    scratch.add("Gym", under("health"));
    scratch
        .db
        .conn()
        .execute(
            "INSERT INTO budget (category_id, amount_minor, currency, rollover, group_kind) \
             VALUES (?1, 3000, 'GBP', 0, 'fixed')",
            [scratch.id("health.gym")],
        )
        .unwrap();
    assert_eq!(in_use(&mut scratch, "health.gym").budgets, 1);

    // A schedule.
    scratch.add("Streaming", under("bills"));
    scratch
        .db
        .conn()
        .execute(
            "INSERT INTO schedule (account_id, merchant_display, category_id, cadence, \
             start_date, amount_minor, currency, tolerance_days) \
             VALUES (?1, 'Netflix', ?2, 'monthly', '2026-11-06', -1299, 'GBP', 3)",
            params![account, scratch.id("bills.streaming")],
        )
        .unwrap();
    let usage = in_use(&mut scratch, "bills.streaming");
    assert_eq!(usage.schedules, 1);

    // Each refusal says why, naming the label, not a transaction.
    let error = delete(&mut scratch.db, &key("food.lunch")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "“Lunch” is used by 2 rules, so it can't be deleted; hide it instead"
    );

    // Hidden or not, nothing was deleted.
    assert_eq!(scratch.tree().len(), DEFAULT_CATEGORIES.len() + 6);

    // `usage` agrees with the refusals.
    assert_eq!(
        usage_of(&scratch, "food.groceries"),
        CategoryUsage {
            split_lines: 1,
            ..CategoryUsage::default()
        }
    );
}

#[test]
fn pending_inbox_suggestions_that_name_a_category_keep_it() {
    use crate::inbox::{self, NewSuggestion, SuggestionId, SuggestionKind};
    use crate::mcp_client::ClientName;

    let mut scratch = Scratch::new();
    let account = scratch.account();
    let row = scratch.row(account, -350);
    scratch.add("Coffee", under("food"));
    scratch.add("Tea", under("food"));
    let client = ClientName::new("claude-code").unwrap();
    let file = |scratch: &mut Scratch, kind, payload| {
        SuggestionId::new(
            inbox::insert_suggestion(
                &mut scratch.db,
                &client,
                &NewSuggestion {
                    kind,
                    payload,
                    rationale: None,
                },
            )
            .unwrap(),
        )
    };

    // A categorisation names Coffee; a rule names both, once by
    // `set_category`… and once by a split template's lines.
    let categorisation = file(
        &mut scratch,
        SuggestionKind::Categorization,
        serde_json::json!({"items": [{"transaction_id": row, "category": "food.coffee"}]}),
    );
    let rule = file(
        &mut scratch,
        SuggestionKind::Rule,
        serde_json::json!({
            "match_field": "merchant", "match_kind": "contains", "pattern": "CAFE",
            "actions": [{"kind": "split_template", "value": [
                {"category": "food.coffee", "basis_points": 5000},
                {"category": "food.tea", "basis_points": 5000},
            ]}],
        }),
    );
    assert_eq!(usage_of(&scratch, "food.coffee").suggestions, 2);
    assert_eq!(
        usage_of(&scratch, "food.tea"),
        CategoryUsage {
            suggestions: 1,
            ..CategoryUsage::default()
        }
    );
    assert!(usage_of(&scratch, "food.tea").only_suggestions());

    // Deleting is refused, saying where to decide them, and nothing goes.
    let error = delete(&mut scratch.db, &key("food.tea")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "“Tea” is used by 1 pending Inbox suggestion, so it can't be deleted; approve or reject \
         them in the Inbox first"
    );
    assert!(scratch.tree().get(&key("food.tea")).is_some());

    // Decided suggestions don't count: once both are rejected, Tea goes.
    inbox::reject(&mut scratch.db, rule, None).unwrap();
    assert_eq!(usage_of(&scratch, "food.coffee").suggestions, 1);
    delete(&mut scratch.db, &key("food.tea")).unwrap();
    inbox::reject(&mut scratch.db, categorisation, Some("Not coffee")).unwrap();
    assert!(usage_of(&scratch, "food.coffee").is_unused());
    delete(&mut scratch.db, &key("food.coffee")).unwrap();
}

fn usage_of(scratch: &Scratch, text: &str) -> CategoryUsage {
    usage(&scratch.db, &key(text)).unwrap()
}

#[test]
fn an_unused_category_is_deleted() {
    let mut scratch = Scratch::new();
    scratch.add("Coffee", under("food"));
    set_hidden(&mut scratch.db, &key("food.coffee"), true).unwrap();
    delete(&mut scratch.db, &key("food.coffee")).unwrap();
    assert!(scratch.tree().get(&key("food.coffee")).is_none());

    // A default nothing uses goes too, and its key is free again.
    delete(&mut scratch.db, &key("food.takeaway")).unwrap();
    let again = scratch.add("Takeaway", under("food"));
    assert_eq!(again.key().as_str(), "food.takeaway");

    // A group goes once its subcategories have.
    let side = scratch.add("Side projects", Placement::Group(CategoryKind::Spending));
    scratch.add("Tools", under(side.key().as_str()));
    assert!(matches!(
        delete(&mut scratch.db, side.key()),
        Err(Error::HasSubcategories { count: 1, .. })
    ));
    delete(&mut scratch.db, &key("side_projects.tools")).unwrap();
    delete(&mut scratch.db, side.key()).unwrap();
    assert!(scratch.tree().get(side.key()).is_none());
    assert!(matches!(
        delete(&mut scratch.db, side.key()),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn the_overview_lists_every_group_with_its_usage() {
    let mut scratch = Scratch::new();
    let account = scratch.account();
    let row = scratch.row(account, -500);
    scratch
        .db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user' WHERE id = ?2",
            params![scratch.id("food.groceries"), row],
        )
        .unwrap();
    set_hidden(&mut scratch.db, &key("home.mortgage"), true).unwrap();

    let overview = overview(&scratch.db).unwrap();
    let tree = scratch.tree();
    assert_eq!(overview.len(), tree.groups().len());
    let keys: Vec<&str> = overview
        .iter()
        .flat_map(|group| std::iter::once(&group.group).chain(&group.children))
        .map(|summary| summary.category.key().as_str())
        .collect();
    let expected: Vec<&str> = tree.iter().map(|c| c.key().as_str()).collect();
    assert_eq!(keys, expected);

    let find = |text: &str| {
        overview
            .iter()
            .flat_map(|group| std::iter::once(&group.group).chain(&group.children))
            .find(|summary| summary.category.key().as_str() == text)
            .unwrap()
    };
    assert_eq!(find("food.groceries").usage.transactions, 1);
    assert!(find("food.takeaway").usage.is_unused());
    assert!(find("home.mortgage").category.is_hidden());
}

/// V18 adds the flag to a database made before it, keeping every category
/// shown.
#[test]
fn the_hidden_flag_applies_to_an_existing_database() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("coffer.db");
    {
        let db = crate::db::open_at_version(&path, 17).unwrap();
        db.conn()
            .execute(
                "INSERT INTO category (parent_id, key, label) \
                 SELECT id, 'food.coffee', 'Coffee' FROM category WHERE key = 'food'",
                [],
            )
            .unwrap();
    }
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        db.schema_version().unwrap(),
        Database::supported_schema_version()
    );
    let tree = db.categories().unwrap();
    assert_eq!(tree.len(), DEFAULT_CATEGORIES.len() + 1);
    assert_eq!(tree.visible().count(), tree.len());
    set_hidden(&mut db, &key("food.coffee"), true).unwrap();
    assert!(!db.categories().unwrap().is_visible(&key("food.coffee")));
    // The schema refuses anything but 0 and 1.
    assert!(
        db.conn()
            .execute("UPDATE category SET hidden = 2 WHERE key = 'food'", [])
            .is_err()
    );
}
