//! Sample data for the Rules view's snapshots and tests: D6.7's fixture
//! (Paper `B1J-0`, `EFT-0`, `G2O-0`, `GZT-0`), written into a database
//! through core's own API, so what the screens show comes from the real
//! read models. Every merchant, amount and ID is invented.
//!
//! - **Rules**: fourteen, in `B1J-0`'s order: TFL TRAVEL, UBER, UBER TRIP
//!   (never reached: UBER catches it), PRET A MANGER, SAINSBURY, WAITROSE on
//!   the joint account with a tag, NETFLIX.COM, the joint account's bills,
//!   a pattern for clothes shops, APPLE.COM/BILL with a name, JUSTGIVING
//!   with only a tag, DISHOOM, and the rent over £1,000 with a share.
//! - **Transactions**: rows the rules match, imported before the rules (so
//!   the import didn't apply them), then applied one rule at a time; six
//!   VINTED rows, four uncategorised and two the user categorised, for
//!   "Create rule…" (`EFT-0`).
//! - **Merchant names**: Amazon Marketplace, Apple, Costa Coffee and more.
//! - **Change history**: the applies, a bulk edit and an undone apply.
//! - **Categories** ([`seed_categories`]): two of the user's own, a group of
//!   their own, and two hidden.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::category::CategoryKey;
use orre_core::change;
use orre_core::db::Database;
use orre_core::id::{AccountId, RuleId, TransactionId};
use orre_core::import::{self, Committed};
use orre_core::inbox::own::recategorize;
use orre_core::merchant::{self, AliasDefinition, AliasKind, DisplayName};
use orre_core::money::{Currency, Money};
use orre_core::reporting::register::Register;
use orre_core::rule::{
    self, ActionText, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
    RuleOrigin, Share, TagList,
};
use orre_core::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use orre_core::tag::TagName;

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).expect("a fixture date")
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).expect("a fixture category")
}

/// One imported row: date, amount, merchant (also the bank text).
type Row = ((u32, u32), i64, &'static str);

fn import(db: &mut Database, account: AccountId, file: &str, rows: &[Row]) -> Vec<TransactionId> {
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for (index, ((month, day), minor, merchant)) in rows.iter().enumerate() {
        let mut line = StatementLine::new(
            date(*month, *day),
            Money::new(*minor, Currency::GBP),
            format!("CARD PAYMENT {merchant}"),
        );
        line.merchant = Some((*merchant).to_owned());
        statement.push(
            SourceLocation::Line(u64::try_from(index).unwrap_or(0) + 2),
            Ok(line),
        );
    }
    let preview = import::preview(db, account, file, statement).expect("a fixture preview");
    let Committed::Batch(_) =
        import::commit(db, &preview, preview.needs_confirmation()).expect("a fixture import")
    else {
        panic!("the fixture import made no batch");
    };
    // The rows' IDs, in the order given: each row is unique by its date,
    // amount and merchant.
    let register = Register::load(&db.reader().expect("a reader"), date(11, 14))
        .expect("the fixture's register");
    rows.iter()
        .map(|((month, day), minor, merchant)| {
            register
                .rows()
                .iter()
                .find(|row| {
                    row.account == account
                        && row.date == date(*month, *day)
                        && row.amount.minor() == *minor
                        && row.bank_merchant.as_deref() == Some(*merchant)
                })
                .map(|row| row.id)
                .expect("an imported fixture row")
        })
        .collect()
}

fn rule(
    field: MatchField,
    kind: MatchKind,
    pattern: &str,
    amount: AmountBounds,
    account: Option<AccountId>,
    actions: Vec<RuleAction>,
) -> RuleDefinition {
    RuleDefinition::new(
        field,
        Pattern::new(kind, pattern).expect("a fixture pattern"),
        amount,
        account.map(|account| account.get()),
        actions,
    )
    .expect("a fixture rule")
}

fn category(text: &str) -> RuleAction {
    RuleAction::SetCategory(key(text))
}

fn tag(name: &str) -> RuleAction {
    RuleAction::AddTag(TagList::new(vec![TagName::new(name).expect("a tag")]).expect("tags"))
}

/// What [`seed`] made, for tests and snapshots to find.
pub struct Seeded {
    pub current: AccountId,
    pub joint: AccountId,
    /// The rules in order.
    pub rules: Vec<RuleId>,
    /// The VINTED row the user just categorised as Shopping (`3WT-0`).
    pub vinted: TransactionId,
}

/// Seeds `db` with D6.7's sample rules, transactions, names and changes.
pub fn seed(db: &mut Database) -> Seeded {
    let current = account::create(
        db,
        &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
    )
    .expect("an account");
    let joint = account::create(
        db,
        &NewAccount::new("Joint account", AccountKind::Current, Currency::GBP),
    )
    .expect("an account");

    let current_rows = import(
        db,
        current,
        "monzo-november.csv",
        &[
            ((11, 2), -310, "TFL TRAVEL CH"),
            ((11, 9), -280, "TFL TRAVEL CH"),
            ((11, 4), -1_430, "UBER TRIP"),
            ((11, 1), -980, "UBER TRIP"),
            ((10, 28), -2_150, "UBER *EATS"),
            ((11, 9), -645, "PRET A MANGER"),
            ((11, 5), -715, "PRET A MANGER"),
            ((11, 8), -1_862, "SAINSBURYS LOCAL"),
            ((11, 6), -1_299, "NETFLIX.COM"),
            ((11, 3), -3_400, "ASOS.COM"),
            ((11, 2), -899, "APPLE.COM/BILL"),
            ((10, 30), -2_000, "JUSTGIVING"),
            ((11, 7), -4_230, "DISHOOM"),
            ((11, 1), -125_000, "FLAT 2B RENT"),
            ((11, 6), -1_675, "VINTED *KX8F2Q"),
            ((10, 28), -1_240, "VINTED *J2M9RA"),
            ((10, 19), -895, "VINTED *Q7TB0C"),
            ((9, 14), -650, "VINTED *4LC8DN"),
            ((9, 3), -1_500, "VINTED *M0R2KE"),
            ((8, 21), -1_875, "VINTED *B8YQ5T"),
            ((11, 9), -1_399, "AMZN MKTP UK*2X4LK91T5"),
            ((10, 24), -2_599, "AMZN MKTP UK*HR3N02QW4"),
            ((10, 11), -799, "AMZN MKTP DE*7Q1ZP04KM"),
            ((11, 3), -320, "COSTA COFFEE"),
        ],
    );
    import(
        db,
        joint,
        "monzo-joint-november.csv",
        &[
            ((11, 2), -2_100, "VINTED *ZP31HW"),
            ((11, 7), -5_216, "WAITROSE"),
            ((11, 7), -8_640, "OCTOPUS ENERGY"),
            ((11, 6), -3_800, "THAMES WATER"),
        ],
    );
    let vinted = current_rows[14];

    // The rules, in B1J-0's order: each stored last.
    let definitions = [
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "TFL TRAVEL",
            AmountBounds::ANY,
            None,
            vec![category("transport")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "UBER",
            AmountBounds::ANY,
            None,
            vec![category("transport")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "UBER TRIP",
            AmountBounds::ANY,
            None,
            vec![category("transport")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "PRET A MANGER",
            AmountBounds::ANY,
            None,
            vec![category("food.eating_out")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::StartsWith,
            "SAINSBURY",
            AmountBounds::ANY,
            None,
            vec![category("food.groceries")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "WAITROSE",
            AmountBounds::ANY,
            Some(joint),
            vec![category("food.groceries"), tag("Household")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Is,
            "NETFLIX.COM",
            AmountBounds::ANY,
            None,
            vec![category("entertainment")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "OCTOPUS ENERGY",
            AmountBounds::ANY,
            Some(joint),
            vec![category("bills")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "THAMES WATER",
            AmountBounds::ANY,
            Some(joint),
            vec![category("bills")],
        ),
        rule(
            MatchField::Description,
            MatchKind::Regex,
            "ASOS|ZARA|UNIQLO",
            AmountBounds::ANY,
            None,
            vec![category("shopping")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "APPLE.COM/BILL",
            AmountBounds::ANY,
            None,
            vec![
                category("entertainment"),
                RuleAction::SetMerchant(ActionText::new("Apple").expect("a name")),
            ],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::Contains,
            "JUSTGIVING",
            AmountBounds::ANY,
            None,
            vec![tag("Gift Aid")],
        ),
        rule(
            MatchField::Merchant,
            MatchKind::StartsWith,
            "DISHOOM",
            AmountBounds::ANY,
            None,
            vec![category("food.eating_out")],
        ),
        rule(
            MatchField::Description,
            MatchKind::Contains,
            "FLAT 2B RENT",
            AmountBounds::new(None, Some(-100_000)).expect("bounds"),
            Some(current),
            vec![
                category("bills"),
                RuleAction::MyShare(Share::new(5_000).expect("a share")),
            ],
        ),
    ];
    let mut rules = Vec::new();
    for (at, definition) in definitions.iter().enumerate() {
        let origin = if at == 1 {
            RuleOrigin::Edit
        } else {
            RuleOrigin::New
        };
        rules.push(rule::create_from(db, definition, at, origin).expect("a fixture rule"));
    }

    // The user's own categories on two VINTED rows (EFT-0's "Category you
    // set"), and the edit that prompts "Create a rule from this edit?".
    recategorize(
        db,
        &[
            (current_rows[18], key("other")),
            (current_rows[19], key("entertainment")),
        ],
    )
    .expect("a fixture edit");

    // History: TFL TRAVEL applied then undone, PRET applied, UBER applied.
    let apply = |db: &mut Database, rule: RuleId| {
        let preview = rule::preview_apply(db, rule).expect("a fixture preview");
        rule::commit_apply(db, &preview, &BTreeSet::new())
            .expect("a fixture apply")
            .batch
    };
    if let Some(batch) = apply(db, rules[3]) {
        change::undo(db, batch).expect("a fixture undo");
    }
    apply(db, rules[0]);
    apply(db, rules[1]);
    apply(db, rules[3]);
    apply(db, rules[11]);
    recategorize(db, &[(vinted, key("shopping"))]).expect("a fixture edit");

    for (kind, pattern, name) in [
        (AliasKind::Prefix, "AMZN MKTP", "Amazon Marketplace"),
        (AliasKind::Prefix, "APPLE.COM/BILL", "Apple"),
        (AliasKind::Prefix, "COSTA COFFEE", "Costa Coffee"),
        (AliasKind::Exact, "OCTOPUS ENERGY", "Octopus Energy"),
        (AliasKind::Prefix, "PRET A MANGER", "Pret A Manger"),
        (AliasKind::Exact, "THAMES WATER", "Thames Water"),
        (AliasKind::Prefix, "TFL TRAVEL CH", "Transport for London"),
        (AliasKind::Regex, r"^UBER \*?(TRIP|BV)", "Uber"),
    ] {
        merchant::add(
            db,
            &AliasDefinition::new(kind, pattern, DisplayName::new(name).expect("a name"))
                .expect("a fixture name"),
        )
        .expect("a fixture name");
    }
    Seeded {
        current,
        joint,
        rules,
        vinted,
    }
}

/// What [`seed_categories`] made.
pub struct SeededCategories {
    /// Coffee, under Food: nothing uses it, so it can be deleted.
    pub coffee: i64,
    /// Side projects, a new spending group, with Tools under it.
    pub side_projects: i64,
}

/// The categories editor's sample (composed, not drawn): the user's own
/// Coffee under Food and a Side projects group with Tools, Mortgage hidden
/// and the Travel group hidden with its subcategories. Run after [`seed`].
pub fn seed_categories(db: &mut Database) -> SeededCategories {
    use orre_core::category::{self, CategoryKind, NewCategory, Placement};
    let mut add = |label: &str, placement: Placement| {
        category::add(
            db,
            &NewCategory {
                label: label.to_owned(),
                placement,
            },
        )
        .expect("a fixture category")
    };
    let coffee = add("Coffee", Placement::Under(key("food")));
    let side_projects = add("Side projects", Placement::Group(CategoryKind::Spending));
    add("Tools", Placement::Under(side_projects.key().clone()));
    for hidden in ["home.mortgage", "travel"] {
        category::set_hidden(db, &key(hidden), true).expect("a fixture category hidden");
    }
    SeededCategories {
        coffee: coffee.id(),
        side_projects: side_projects.id(),
    }
}
