use crate::exclusion::ExclusionReason;
use crate::money::Currency;

use super::super::test_ledger::{TestLedger, date};
use super::super::{DateRange, Limit};
use super::*;

/// `(transaction, split line, link, category key, amount, personal)` of
/// each line.
type Summary = (
    TransactionId,
    Option<u32>,
    Option<LinkKind>,
    Option<String>,
    i64,
    i64,
);

fn summarize(lines: &[LedgerLine]) -> Vec<Summary> {
    lines
        .iter()
        .map(|line| {
            (
                line.transaction_id,
                line.line_no,
                line.link_kind,
                line.category.as_ref().map(|c| c.key.as_str().to_owned()),
                line.amount.minor(),
                line.personal.minor(),
            )
        })
        .collect()
}

fn key(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn a_split_purchase_counts_in_each_lines_category() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shop = ledger.row(current, "2026-11-03", -5_000, None);
    ledger.split(
        shop,
        &[
            (-3_000, Some("food.groceries")),
            (-1_500, Some("home")),
            (-500, None),
        ],
    );

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (shop, Some(1), None, key("food.groceries"), -3_000, -3_000),
            (shop, Some(2), None, key("home"), -1_500, -1_500),
            (shop, Some(3), None, None, -500, -500),
        ]
    );
    assert!(lines.iter().all(|line| line.split_id.is_some()));
    let groceries = Ledger::new()
        .categories([CategoryKey::new("food").unwrap()])
        .lines(&ledger.reader())
        .unwrap();
    assert_eq!(
        summarize(&groceries).len(),
        1,
        "a group includes its subcategories"
    );
}

#[test]
fn a_refund_nets_against_its_purchases_category_on_its_own_date() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_500, Some("income.gifts"));
    ledger.refund(back, shoes, 6_000);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (shoes, None, None, key("shopping.clothing"), -6_000, -6_000),
            // The unlinked rest keeps the inflow's own category.
            (back, None, None, key("income.gifts"), 500, 500),
            (
                back,
                None,
                Some(LinkKind::Refund),
                key("shopping.clothing"),
                6_000,
                6_000
            ),
        ]
    );
    let refund = &lines[2];
    assert!(refund.is_refund());
    assert_eq!(refund.target_id, Some(shoes));
    assert_eq!(refund.date, date("2026-11-04"));
    let november = Ledger::new()
        .within(DateRange::parse("2026-11-01", "2026-11-30").unwrap())
        .categories([CategoryKey::new("shopping").unwrap()])
        .lines(&ledger.reader())
        .unwrap();
    assert_eq!(summarize(&november).len(), 1);
}

#[test]
fn scoped_to_transactions_it_keeps_their_lines_and_the_parts_that_net_against_them() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_500, Some("income.gifts"));
    ledger.refund(back, shoes, 6_000);
    let coffee = ledger.row(current, "2026-11-05", -350, None);

    let reader = ledger.reader();
    let scoped = |ids: Vec<TransactionId>| {
        summarize(&Ledger::new().transactions(ids).lines(&reader).unwrap())
    };
    // The refund's part nets against the shoes, so it comes with them; the
    // refund's own rest doesn't.
    assert_eq!(
        scoped(vec![shoes]),
        [
            (shoes, None, None, key("shopping.clothing"), -6_000, -6_000),
            (
                back,
                None,
                Some(LinkKind::Refund),
                key("shopping.clothing"),
                6_000,
                6_000
            ),
        ]
    );
    assert_eq!(scoped(vec![back]).len(), 2);
    assert_eq!(
        scoped(vec![coffee]),
        [(coffee, None, None, None, -350, -350)]
    );
    assert!(scoped(Vec::new()).is_empty());
    assert!(scoped(vec![TransactionId::new(9_999)]).is_empty());
    // It goes with the other filters, and on the dry run's connection too.
    let november = Ledger::new()
        .transactions([shoes, coffee])
        .within(DateRange::parse("2026-11-01", "2026-11-30").unwrap());
    assert_eq!(
        summarize(&november.lines_in(ledger.db.conn()).unwrap()),
        summarize(&november.lines(&reader).unwrap())
    );
    assert_eq!(november.lines(&reader).unwrap().len(), 2);
}

#[test]
fn a_fully_linked_inflow_leaves_no_zero_row() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_000, None);
    ledger.refund(back, shoes, 6_000);
    let lines = Ledger::new()
        .uncategorized()
        .lines(&ledger.reader())
        .unwrap();
    assert!(lines.is_empty(), "{lines:#?}");
}

#[test]
fn a_shared_cost_counts_as_your_share_and_a_claim_as_nothing() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let dinner = ledger.row(current, "2026-11-05", -9_000, Some("food.eating_out"));
    ledger.share(dinner, -3_000);
    let hotel = ledger.row(current, "2026-11-06", -20_000, Some("travel"));
    ledger.claim_back(hotel);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (dinner, None, None, key("food.eating_out"), -9_000, -3_000),
            (hotel, None, None, key("travel"), -20_000, 0),
        ]
    );
    assert_eq!(lines[0].counted(Basis::Personal).minor(), -3_000);
    assert_eq!(lines[0].counted(Basis::Gross).minor(), -9_000);
}

#[test]
fn excluded_rows_and_confirmed_transfers_are_left_out() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let kept = ledger.row(current, "2026-11-01", -1_000, Some("food.groceries"));
    let disputed = ledger.row(current, "2026-11-02", -2_000, Some("food.groceries"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    let out = ledger.row(current, "2026-11-03", -5_000, None);
    let into = ledger.row(savings, "2026-11-03", 5_000, None);
    ledger.transfer(out, into);
    // A refund of an excluded double charge leaves reports with the charge.
    let double = ledger.row(current, "2026-11-04", -700, Some("shopping"));
    let back = ledger.row(current, "2026-11-06", 700, None);
    ledger.refund(back, double, 700);
    ledger.exclude(double, ExclusionReason::BankError);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [(kept, None, None, key("food.groceries"), -1_000, -1_000)]
    );
}

#[test]
fn every_line_keeps_its_own_currency() {
    let mut ledger = TestLedger::new();
    let gbp = ledger.account("Current", Currency::GBP);
    let eur = ledger.account("Euro", Currency::EUR);
    ledger.row(gbp, "2026-11-01", -1_000, Some("food.groceries"));
    ledger.row(eur, "2026-11-01", -1_000, Some("food.groceries"));

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    let currencies: Vec<String> = lines
        .iter()
        .map(|l| l.amount.currency().code().to_owned())
        .collect();
    assert_eq!(currencies, ["GBP", "EUR"]);
    let mut totals = super::super::Totals::new();
    for line in &lines {
        totals.add(line.counted(Basis::Personal)).unwrap();
    }
    assert_eq!(totals.len(), 2);
    assert_eq!(totals.get(Currency::GBP).unwrap().minor(), -1_000);
    assert_eq!(totals.get(Currency::EUR).unwrap().minor(), -1_000);
}

#[test]
fn filters_and_together() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let other = ledger.account("Other", Currency::GBP);
    let coffee = ledger.row_at(
        current,
        "2026-11-01",
        -350,
        Some("food.eating_out"),
        Some("BEAN THERE 100%"),
        "CARD PAYMENT BEAN THERE",
    );
    let lunch = ledger.row_at(
        current,
        "2026-11-02",
        -1_200,
        None,
        Some("Noodle Bar"),
        "NOODLE_BAR LONDON",
    );
    let elsewhere = ledger.row_at(other, "2026-11-03", -900, None, None, "BEAN THERE KIOSK");
    ledger.tag(coffee, "Work", false);
    ledger.tag(lunch, "secret-fixture", true);
    let reader = ledger.reader();
    let ids = |query: Ledger| -> Vec<TransactionId> {
        query
            .lines(&reader)
            .unwrap()
            .iter()
            .map(|l| l.transaction_id)
            .collect()
    };

    assert_eq!(
        ids(Ledger::new().matching("bean there")),
        [coffee, elsewhere]
    );
    assert_eq!(
        ids(Ledger::new().matching("  ")),
        [coffee, lunch, elsewhere]
    );
    // `%` and `_` are literal.
    assert_eq!(ids(Ledger::new().matching("100%")), [coffee]);
    assert_eq!(
        ids(Ledger::new().matching("n_t")),
        Vec::<TransactionId>::new()
    );
    assert_eq!(ids(Ledger::new().matching("noodle_bar")), [lunch]);
    assert_eq!(ids(Ledger::new().merchant(" noodle bar ")), [lunch]);
    assert_eq!(
        ids(Ledger::new().merchant("noodle")),
        Vec::<TransactionId>::new()
    );
    assert_eq!(ids(Ledger::new().accounts([other])), [elsewhere]);
    assert_eq!(ids(Ledger::new().uncategorized()), [lunch, elsewhere]);
    assert_eq!(
        ids(Ledger::new().amount_between(Some(-1_000), Some(-500))),
        [elsewhere]
    );
    assert_eq!(
        ids(Ledger::new().tags([TagName::new("work").unwrap()])),
        [coffee]
    );
    assert_eq!(
        ids(Ledger::new().newest_first()),
        [elsewhere, lunch, coffee]
    );
    assert_eq!(
        ids(Ledger::new()
            .matching("bean")
            .within(DateRange::parse("2026-11-02", "2026-11-30").unwrap())),
        [elsewhere]
    );
    assert!(ledger_lines_have_no_bank_text(&reader));
    let with_text = Ledger::new().with_bank_text().lines(&reader).unwrap();
    assert_eq!(
        with_text[1].bank_text.as_ref().unwrap().description,
        "NOODLE_BAR LONDON"
    );
}

fn ledger_lines_have_no_bank_text(reader: &Reader) -> bool {
    Ledger::new()
        .lines(reader)
        .unwrap()
        .iter()
        .all(|line| line.bank_text.is_none())
}

#[test]
fn unknown_and_private_filters_are_refused() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let row = ledger.row(current, "2026-11-01", -100, None);
    ledger.tag(row, "secret-fixture", true);
    let reader = ledger.reader();

    let error = Ledger::new()
        .tags([TagName::new("Secret-Fixture").unwrap()])
        .lines(&reader)
        .unwrap_err();
    assert!(matches!(error, Error::UnknownTag));
    assert!(!error.to_string().to_lowercase().contains("secret"));
    assert!(matches!(
        Ledger::new()
            .tags([TagName::new("nope").unwrap()])
            .lines(&reader),
        Err(Error::UnknownTag)
    ));
    assert!(matches!(
        Ledger::new()
            .categories([CategoryKey::new("no.such").unwrap()])
            .lines(&reader),
        Err(Error::UnknownCategory(_))
    ));
    assert!(matches!(
        Ledger::new().accounts([AccountId::new(999)]).lines(&reader),
        Err(Error::UnknownAccount(_))
    ));
    let error = Ledger::new()
        .amount_between(Some(10), Some(-10))
        .lines(&reader)
        .unwrap_err();
    assert!(matches!(error, Error::InvalidAmountRange));
    assert!(error.is_invalid_input());
}

#[test]
fn capped_lines_say_when_more_matched() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    for day in 1..=5 {
        ledger.row(current, &format!("2026-11-0{day}"), -100 * day, None);
    }
    let reader = ledger.reader();
    let cut = Ledger::new()
        .newest_first()
        .lines_capped(&reader, Limit::new(Some(2)))
        .unwrap();
    assert!(cut.truncated);
    let amounts: Vec<i64> = cut.items.iter().map(|l| l.amount.minor()).collect();
    assert_eq!(amounts, [-500, -400]);
    let all = Ledger::new()
        .lines_capped(&reader, Limit::new(Some(5)))
        .unwrap();
    assert!(!all.truncated);
    assert_eq!(all.len(), 5);
}

/// Rows for the text-filter tests: `(display merchant, bank text)`.
const TEXTS: &[(Option<&str>, &str)] = &[
    (Some("Bean There"), "CARD BEAN THERE LONDON"),
    (Some("Noodle Bar"), "NOODLE_BAR 100% FRESH"),
    (None, "Back\\slash \"Quoted\" shop"),
    (Some("Éclair Café"), "ÉCLAIR CAFÉ PARIS"),
    (Some("eclair cafe"), "plain ascii"),
    (None, "COFFEE OR TEA NEAR(a b) STAR*"),
    (None, "merchant:foo (x) -y +z ^w"),
    (Some("ÄÖÜ Werk"), "straße ÄÖÜ"),
    (None, "ab"),
    (Some("Straße"), "STRASSE ß"),
];

/// A ledger of [`TEXTS`], and each row's ID. Every kind of ledger line is
/// there: the last row is split, and refunded in part by a row of its own;
/// the one before it is refunded whole. A line's display merchant is its own
/// transaction's, so the index, keyed by transaction, finds every line.
fn text_ledger() -> (TestLedger, Vec<TransactionId>) {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let ids: Vec<TransactionId> = TEXTS
        .iter()
        .enumerate()
        .map(|(index, (merchant, text))| {
            let day = format!("2026-11-{:02}", index + 1);
            ledger.row_at(current, &day, -100 - index as i64, None, *merchant, text)
        })
        .collect();
    let werk = ids[TEXTS.len() - 3];
    let strasse = ids[TEXTS.len() - 1];
    ledger.split(strasse, &[(-60, Some("food.groceries")), (-49, None)]);
    let back = ledger.row_at(
        current,
        "2026-11-20",
        30,
        None,
        Some("Straße Refund"),
        "REFUND STRASSE ß",
    );
    ledger.refund(back, strasse, 30);
    let whole = ledger.row_at(
        current,
        "2026-11-21",
        107,
        None,
        Some("ÄÖÜ Werk"),
        "RETURN ÄÖÜ WERK",
    );
    ledger.refund(whole, werk, 107);
    (ledger, ids)
}

/// The text filter reads the full-text index and then `LIKE`: the
/// index narrows, `LIKE` decides, so the rows are exactly what `LIKE` alone
/// returns, whatever the text (FT.3).
#[test]
fn matching_through_the_index_returns_the_rows_like_alone_does() {
    let (ledger, ids) = text_ledger();
    let reader = ledger.reader();
    let lines = |query: Ledger| -> Vec<TransactionId> {
        query
            .lines(&reader)
            .unwrap()
            .iter()
            .map(|line| line.transaction_id)
            .collect()
    };
    let every_line = |query: Ledger| -> Vec<Summary> { summarize(&query.lines(&reader).unwrap()) };
    let queries = [
        // A NUL can't reach FTS5 (`search::phrase`).
        "abc\0def",
        "bean\0there",
        "bean there\0",
        "\0",
        // Lines of splits and refunds.
        "refund",
        "werk",
        // ASCII case variants and word boundaries.
        "bean there",
        "BEAN THERE",
        "BeAn ThErE",
        "n there lon",
        "bean  there",
        " bean there ",
        "e",
        "be",
        "bea",
        // `LIKE` wildcards and escapes are literal.
        "%",
        "_",
        "100%",
        "100",
        "n_t",
        "noodle_bar",
        "noodle%bar",
        "\\",
        "k\\s",
        "\\s",
        // Quotes and FTS5 syntax are literal.
        "\"",
        "\"Quoted\"",
        "k\\slash \"Quoted\" sh",
        "\" OR \"",
        "COFFEE OR TEA",
        "coffee tea",
        "NEAR(a b)",
        "NEAR",
        "STAR*",
        "ST*",
        "*",
        "merchant:foo",
        "(x)",
        "-y +z",
        "+z ^w",
        "^w",
        // Non-ASCII: `LIKE` folds ASCII only and the index folds Unicode.
        "éclair",
        "ÉCLAIR",
        "éclair café",
        "Éclair Café",
        "café",
        "CAFÉ",
        "cafe",
        "eclair",
        "ÄÖÜ",
        "äöü",
        "straße",
        "STRASSE",
        "ß",
        "é",
        "ÉC",
        "ab",
        "abc",
        "a",
        "zzz",
        "",
        "   ",
    ];
    for query in queries {
        let through_index = every_line(Ledger::new().matching(query));
        let like_alone = every_line(Ledger::new().like_only().matching(query));
        assert_eq!(through_index, like_alone, "matching({query:?})");
    }

    // Spot checks, so the two can't agree by both being wrong.
    let at =
        |indexes: &[usize]| -> Vec<TransactionId> { indexes.iter().map(|&i| ids[i]).collect() };
    assert_eq!(lines(Ledger::new().matching("n there lon")), at(&[0]));
    assert_eq!(lines(Ledger::new().matching("100%")), at(&[1]));
    assert_eq!(
        lines(Ledger::new().matching("k\\slash \"Quoted\" sh")),
        at(&[2])
    );
    assert_eq!(lines(Ledger::new().matching("coffee or tea")), at(&[5]));
    assert_eq!(lines(Ledger::new().matching("merchant:foo")), at(&[6]));
    assert_eq!(lines(Ledger::new().matching("ab")), at(&[8]));
    assert_eq!(lines(Ledger::new().matching("ÉCLAIR")), at(&[3]));
    assert_eq!(lines(Ledger::new().matching("eclair")), at(&[4]));
    // The index finds "éclair" in the rows with "É", which `LIKE` doesn't:
    // `LIKE` decides, so they aren't returned.
    assert_eq!(lines(Ledger::new().matching("éclair")), at(&[]));
    assert_eq!(
        every_line(Ledger::new().matching("   ")),
        every_line(Ledger::new()),
        "blank text matches everything"
    );
    // Split lines and refund parts are found by their own transaction's text.
    let strasse = ids[TEXTS.len() - 1];
    let werk = ids[TEXTS.len() - 3];
    let found = every_line(Ledger::new().matching("refund"));
    assert!(
        found.iter().any(|line| line.2 == Some(LinkKind::Refund)),
        "{found:?}"
    );
    let found = every_line(Ledger::new().matching("straße"));
    assert_eq!(
        found
            .iter()
            .filter(|line| line.0 == strasse && line.1.is_some())
            .count(),
        2,
        "both split lines: {found:?}"
    );
    let found = every_line(Ledger::new().matching("return ÄÖÜ"));
    assert!(
        found
            .iter()
            .all(|line| line.0 != werk && line.2 == Some(LinkKind::Refund)),
        "only the whole refund's part: {found:?}"
    );
    assert!(!found.is_empty());
}

/// The text filter runs the full-text query once, as a list subquery. SQLite
/// writes a full-text lookup as `SCAN transaction_text VIRTUAL TABLE INDEX
/// 0:M<column>`: the `M` is the `MATCH`, and without it the plan would read
/// every row of the index.
#[test]
fn the_text_filter_looks_the_index_up_instead_of_scanning() {
    let (ledger, _) = text_ledger();
    let reader = ledger.reader();
    let plan = |query: &Ledger| -> Vec<String> {
        let (sql, values) = query.statement(reader.conn(), None).unwrap();
        let mut statement = reader
            .conn()
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        statement
            .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };

    let indexed = plan(&Ledger::new().matching("bean there"));
    assert!(
        indexed
            .iter()
            .any(|step| step.contains("transaction_text VIRTUAL TABLE INDEX 0:M")),
        "{indexed:#?}"
    );
    let short = plan(&Ledger::new().matching("be"));
    assert!(
        short.iter().all(|step| !step.contains("transaction_text")),
        "{short:#?}"
    );
}
