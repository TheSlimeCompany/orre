//! Text for the full-text index over transactions (`transaction_text`, V24).
//!
//! The index is an FTS5 table with the `trigram` tokenizer. A query to it is
//! FTS5 query syntax, where `OR`, `NEAR`, `*`, `^`, `:`, parentheses, `-`, `+`
//! and quotes all mean something. User and MCP text must never mean
//! anything but itself, so [`phrase`] turns it into one double-quoted string:
//! inside quotes FTS5 reads only the text, with each `"` doubled to stand for
//! itself.
//!
//! Trigrams are three characters long, so text shorter than that can't use the
//! index: [`phrase`] returns `None`, and the caller matches it with `LIKE`.
//! Text with a NUL can't either: FTS5 reads its query as a C string, so the
//! phrase would end at the NUL, unterminated, and be refused as a syntax
//! error. [`phrase`] returns `None` for it too, and `LIKE` answers as before.
//!
//! The index finds candidates; it doesn't decide. Its case folding covers all
//! of Unicode and `LIKE`'s only ASCII, so a caller that needs `LIKE`'s answer
//! keeps `LIKE` as the final check (`reporting::ledger`).

pub mod similar;

/// The fewest characters (Unicode scalar values) the trigram tokenizer can
/// match.
const MIN_CHARS: usize = 3;

/// `text`, trimmed, as an FTS5 query for the literal phrase, or `None` when
/// the index can't answer it: fewer than three characters, or a NUL.
pub fn phrase(text: &str) -> Option<String> {
    let text = text.trim();
    if text.chars().count() < MIN_CHARS || text.contains('\0') {
        return None;
    }
    Some(format!("\"{}\"", text.replace('"', "\"\"")))
}

#[cfg(test)]
mod index_tests;
#[cfg(test)]
mod tests;
