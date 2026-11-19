-- The account number an import file states for its own account, as a keyed
-- hash, so a file imported into the wrong account can be warned about
-- (ROADMAP R.13; `docs/architecture/import.md` → "Decision record: a file's
-- account number is checked, never stored").
--
-- The number itself is never stored. `identity_hash` is HMAC-SHA256 of the
-- normalised number under the per-database key in `setting`
-- (`import.account_key`), as 64 lowercase hex digits. A number a bank masks
-- (`XXXX-XXXX-XXXX-9876`) is hashed as it is written: the masked text is
-- still an identity.
--
-- One row per committed batch whose file stated a number, so a hash is known
-- to an account for as long as one live batch has it. Undoing a batch
-- deletes its row (core does it, as for flags and balances), and only then
-- does the account forget a hash no other batch brought. `account_id` is the
-- batch's account, repeated so the lookups don't join; deleting an account
-- takes its hashes with it, the one `ON DELETE` here (chosen while building
-- R.13: a hash means nothing without its account).
-- `coffer mcp` never reads this table.

CREATE TABLE account_source_identity (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id      INTEGER NOT NULL UNIQUE REFERENCES import_batch (id),
    account_id    INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    identity_hash TEXT    NOT NULL CHECK (length(identity_hash) = 64
                                          AND identity_hash NOT GLOB '*[^0-9a-f]*')
) STRICT;

CREATE INDEX account_source_identity_account
    ON account_source_identity (account_id, identity_hash);

CREATE INDEX account_source_identity_hash
    ON account_source_identity (identity_hash);
