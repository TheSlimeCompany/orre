//! Noticing a file imported into the wrong account (ROADMAP R.13;
//! `docs/architecture/import.md` → "Decision record: a file's account number
//! is checked, never stored").
//!
//! An OFX file states the number of its own account. It is kept in memory
//! on the [`Statement`] ([`crate::statement::SourceAccountNumber`]) and only
//! its HMAC-SHA256 under the per-database key
//! ([`crate::setting::IMPORT_ACCOUNT_KEY`]) is ever stored, in
//! `account_source_identity` (V23): one row per committed batch whose file
//! had a number.
//!
//! - **Learn on commit**: committing a batch records its file's hash for the
//!   account, so the account knows every number the user has imported into
//!   it (a replacement card adds a second). Undoing the batch deletes the
//!   row; the account forgets the hash only if no other batch has it.
//! - **Warn in the preview**: when the file's hash isn't one the account
//!   knows, the preview carries a [`SourceAccountMismatch`] if the account
//!   knows other hashes, or the hash is known to another account (which it
//!   names). An account with no hash yet gets a file known to no other
//!   account without a word, as do a file without a number, a CSV and a
//!   database with no key. Importing anyway is allowed (and learns the
//!   hash); nothing here needs a confirmation.
//!
//! The preview only reads, so it never creates the key: without one no
//! hash has been stored and there is nothing to compare. The commit creates
//! it in its own transaction. Nothing here logs the number, the hash or the
//! key.

use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::Sha256;

use super::Error;
use crate::id::{AccountId, BatchId};
use crate::setting::{self, SourceIdentityKey};
use crate::statement::{SourceAccountNumber, Statement};

/// Another account that the file's number is known to belong to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherAccount {
    pub id: AccountId,
    pub name: String,
}

/// What the preview says when the file's account number isn't one the
/// chosen account has had before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAccountMismatch {
    /// The account the number is known to belong to, when it is known to
    /// one other than the chosen account; the most recently imported into
    /// if several.
    pub other_account: Option<OtherAccount>,
    /// Whether the chosen account has had files with other numbers. When
    /// it has had none, the warning is only that `other_account` has had
    /// this one.
    pub account_has_others: bool,
}

/// HMAC-SHA256 of the number under `key`, as 64 lowercase hex digits.
pub(super) fn hash(key: &SourceIdentityKey, number: &SourceAccountNumber) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.bytes()).expect("HMAC takes any key length");
    mac.update(number.as_str().as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The warning for importing `statement` into `account`, if any. Reads
/// only.
pub(super) fn check(
    conn: &Connection,
    account: AccountId,
    statement: &Statement,
) -> Result<Option<SourceAccountMismatch>, Error> {
    let Some(number) = statement.source_account() else {
        return Ok(None);
    };
    let Some(key) = setting::source_identity_key_on(conn)? else {
        return Ok(None);
    };
    let file = hash(&key, number);

    let known: i64 = conn.query_row(
        "SELECT COUNT(*) FROM account_source_identity WHERE account_id = ?1",
        [account],
        |row| row.get(0),
    )?;
    let seen: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM account_source_identity \
         WHERE account_id = ?1 AND identity_hash = ?2)",
        params![account, file],
        |row| row.get(0),
    )?;
    if seen {
        return Ok(None);
    }
    let other_account = conn
        .query_row(
            "SELECT a.id, a.name FROM account_source_identity i \
             JOIN account a ON a.id = i.account_id \
             WHERE i.identity_hash = ?1 AND i.account_id <> ?2 \
             ORDER BY i.batch_id DESC LIMIT 1",
            params![file, account],
            |row| {
                Ok(OtherAccount {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            },
        )
        .optional()?;
    if known == 0 && other_account.is_none() {
        return Ok(None);
    }
    Ok(Some(SourceAccountMismatch {
        other_account,
        account_has_others: known > 0,
    }))
}

/// Records the file's hash for `batch`, creating the key if this is the
/// first need of it. Does nothing for a statement without a number.
pub(super) fn learn(
    conn: &Connection,
    batch: BatchId,
    account: AccountId,
    statement: &Statement,
) -> Result<(), Error> {
    let Some(number) = statement.source_account() else {
        return Ok(());
    };
    let key = setting::ensure_source_identity_key_on(conn)?;
    conn.execute(
        "INSERT INTO account_source_identity (batch_id, account_id, identity_hash) \
         VALUES (?1, ?2, ?3)",
        params![batch, account, hash(&key, number)],
    )?;
    Ok(())
}

/// Forgets what `batch` taught its account.
pub(super) fn forget(conn: &Connection, batch: BatchId) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM account_source_identity WHERE batch_id = ?1",
        [batch],
    )?;
    Ok(())
}
