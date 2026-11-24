//! Tests for [`phrase`]: every text is searched for literally, and never
//! reaches FTS5 as syntax.

use tempfile::TempDir;

use super::*;
use crate::db::Database;

#[test]
fn text_becomes_one_quoted_string_with_quotes_doubled() {
    assert_eq!(phrase("coffee").as_deref(), Some("\"coffee\""));
    assert_eq!(phrase("  bean there ").as_deref(), Some("\"bean there\""));
    assert_eq!(phrase("a\" OR \"b").as_deref(), Some("\"a\"\" OR \"\"b\""));
    assert_eq!(phrase("\"\"\"").as_deref(), Some("\"\"\"\"\"\"\"\""));
}

#[test]
fn text_under_three_characters_has_no_phrase() {
    for text in ["", "   ", "a", " a ", "ab", "é", "éé", "\u{1F600}\u{1F600}"] {
        assert_eq!(phrase(text), None, "{text:?}");
    }
    // Characters, not bytes.
    assert!(phrase("ééé").is_some());
    assert!(phrase("abc").is_some());
}

/// FTS5 reads a query as a C string: a phrase cut at a NUL is unterminated
/// and refused, so text with one goes to `LIKE` instead.
#[test]
fn text_with_a_nul_has_no_phrase() {
    for text in ["abc\0def", "\0abc", "abc\0", "a\0b", "\0\0\0"] {
        assert_eq!(phrase(text), None, "{text:?}");
    }
}

/// A database with one row per text, as the raw description, returning
/// each text's row ID.
fn indexed(texts: &[&str]) -> (TempDir, Database, Vec<i64>) {
    let dir = TempDir::new().unwrap();
    let db = Database::open(&dir.path().join("orre.db")).unwrap();
    db.conn()
        .execute(
            "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP')",
            [],
        )
        .unwrap();
    let ids = texts
        .iter()
        .map(|text| {
            db.conn()
                .execute(
                    "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                     description_raw, description_normalized, origin) \
                     VALUES (1, '2026-11-02', -100, 'GBP', ?1, upper(?1), 'manual')",
                    [text],
                )
                .unwrap();
            db.conn().last_insert_rowid()
        })
        .collect();
    (dir, db, ids)
}

/// The rows the index finds for `text`; an FTS5 syntax error is a panic.
fn found(db: &Database, text: &str) -> Vec<i64> {
    let query = phrase(text).unwrap_or_else(|| panic!("{text:?} has no phrase"));
    let mut statement = db
        .conn()
        .prepare("SELECT rowid FROM transaction_text WHERE transaction_text MATCH ?1 ORDER BY 1")
        .unwrap();
    statement
        .query_map([query], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

#[test]
fn fts5_syntax_in_the_text_is_matched_literally() {
    let texts = [
        "COFFEE OR TEA",       // 0
        "COFFEE",              // 1
        "TEA",                 // 2
        "NEAR(a b)",           // 3
        "a b",                 // 4
        "STAR*",               // 5
        "STAR",                // 6
        "say \"hello\" there", // 7
        "a\" OR \"b",          // 8
        "merchant:foo",        // 9
        "foo",                 // 10
        "(one two)",           // 11
        "one two",             // 12
        "non-stop",            // 13
        "nonstop",             // 14
        "+plus ^caret",        // 15
        "AND NOT",             // 16
        "ÉCLAIR CAFÉ",         // 17
    ];
    let (_dir, db, ids) = indexed(&texts);
    let at = |indexes: &[usize]| -> Vec<i64> { indexes.iter().map(|&i| ids[i]).collect() };

    assert_eq!(found(&db, "COFFEE OR TEA"), at(&[0]));
    assert_eq!(found(&db, "coffee or tea"), at(&[0]));
    assert_eq!(found(&db, "NEAR(a b)"), at(&[3]));
    assert_eq!(found(&db, "NEAR"), at(&[3]));
    assert_eq!(found(&db, "STAR*"), at(&[5]));
    assert_eq!(found(&db, "ST*R"), at(&[]));
    assert_eq!(found(&db, "\"hello\""), at(&[7]));
    assert_eq!(found(&db, "say \"hello\" there"), at(&[7]));
    assert_eq!(found(&db, "\"\"\""), at(&[]));
    assert_eq!(found(&db, "a\" OR \"b"), at(&[8]));
    assert_eq!(found(&db, "merchant:foo"), at(&[9]));
    assert_eq!(found(&db, "description_raw:foo"), at(&[]));
    assert_eq!(found(&db, "(one two)"), at(&[11]));
    assert_eq!(found(&db, "one two"), at(&[11, 12]));
    assert_eq!(found(&db, "non-stop"), at(&[13]));
    assert_eq!(found(&db, "-stop"), at(&[13]));
    assert_eq!(found(&db, "+plus"), at(&[15]));
    assert_eq!(found(&db, "^caret"), at(&[15]));
    assert_eq!(found(&db, "AND NOT"), at(&[16]));
    assert_eq!(found(&db, "NOT"), at(&[16]));
    // The whole text is one phrase, so words that don't touch don't match.
    assert_eq!(found(&db, "coffee tea"), at(&[]));
}

#[test]
fn non_ascii_text_is_matched_with_its_case_folded() {
    let (_dir, db, ids) = indexed(&["ÉCLAIR CAFÉ", "eclair cafe", "naïve"]);

    assert_eq!(found(&db, "éclair"), [ids[0]]);
    assert_eq!(found(&db, "ÉCLAIR"), [ids[0]]);
    assert_eq!(found(&db, "café"), [ids[0]]);
    // Diacritics aren't removed.
    assert_eq!(found(&db, "eclair"), [ids[1]]);
    assert_eq!(found(&db, "NAÏ"), [ids[2]]);
}

#[test]
fn text_of_exactly_three_characters_is_found() {
    let (_dir, db, ids) = indexed(&["xx abc yy", "ab", "éèê"]);

    assert_eq!(found(&db, "abc"), [ids[0]]);
    assert_eq!(found(&db, " ABC "), [ids[0]]);
    assert_eq!(found(&db, "ÉÈÊ"), [ids[2]]);
}
