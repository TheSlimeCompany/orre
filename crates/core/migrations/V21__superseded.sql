-- coffer: foreign_keys=off
--
-- A bank's correction supersedes the stored row (ROADMAP R.4, L.21;
-- `docs/architecture/import.md` → "Decision record: a bank's correction
-- supersedes the stored row").
--
-- When a file has a source ID the account already holds, but for another
-- date or amount (a tip added, a hold settled, pending → posted), the import
-- can now take the bank's new version: the stored row is excluded with the
-- new reason `superseded` and points at the row that replaced it
-- (`superseded_by`), and the new row is inserted beside it with the same
-- source ID. Undoing that import deletes the new row and clears the old
-- row's exclusion, which restores it exactly.
--
-- - `excluded_reason` takes `superseded`. Like `duplicate`, a superseded row
--   leaves the computed balance as well as spending, income, budgets and
--   reports: the bank's new version replaces it.
-- - `superseded_by` names the replacing row, and is set exactly when the
--   reason is `superseded`. The import points the stored row at the new
--   row's ID before inserting it (the source ID index refuses the new row
--   while the stored one is live), so the foreign key is deferred to the end
--   of the transaction. Triggers check that the two rows share an account
--   and a source ID.
-- - Superseded rows step aside: the source ID index and
--   `transaction_insert_new` count only rows that aren't superseded. A
--   superseded row keeps its source ID, so an account holds one live row
--   and any number of superseded ones per source ID.
-- - `transaction_source_fields_read_only` also refuses pointing a
--   superseded row at another row: `superseded_by` changes only together
--   with the reason.
--
-- Changing the CHECK means rebuilding `"transaction"` (SQLite's 12-step
-- procedure). Other tables reference it, so foreign keys are off around
-- this migration: its first line asks `db::migrations::run` for that, and
-- the runner checks every foreign key before the transaction commits. The
-- triggers and the view of other tables that name `"transaction"` are
-- dropped first, because renaming the new table re-parses them, and
-- recreated afterwards exactly as they were (V1, V5, V9, V10, V12), as are
-- the table's own indexes and triggers, but for the source ID index and the
-- two triggers above. Every row, ID and the AUTOINCREMENT sequence carry
-- over.

DROP TRIGGER import_batch_status;
DROP TRIGGER account_pot_parent;
DROP TRIGGER account_kind_and_currency;
DROP TRIGGER transaction_split_insert;
DROP TRIGGER transaction_split_update;
DROP TRIGGER transfer_group_split;
DROP TRIGGER transaction_link_insert;
DROP TRIGGER transaction_link_update;
DROP VIEW ledger_line;
DROP TRIGGER duplicate_candidate_insert;
DROP TRIGGER schedule_match_insert;

CREATE TABLE transaction_new (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id             INTEGER NOT NULL REFERENCES account (id),
    batch_id               INTEGER REFERENCES import_batch (id),

    -- Source fields, as imported.
    date                   TEXT    NOT NULL CHECK (date IS date(date)),
    -- `24:00:00` round-trips through time() but isn't a time of day.
    time                   TEXT    CHECK (time IS time(time) AND time < '24:00:00'),
    amount_minor           INTEGER NOT NULL,
    currency               TEXT    NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    -- The bank's merchant name, when the export has one (Monzo's "Name").
    merchant               TEXT,
    description_raw        TEXT    NOT NULL,
    -- `description_raw` with whitespace collapsed and uppercased.
    description_normalized TEXT    NOT NULL,
    -- The file's own ID for the row (OFX `FITID`, a CSV ID column).
    external_id            TEXT    CHECK (external_id <> ''),
    -- Dedupe key when there's no external ID: account, date, amount,
    -- normalized description and occurrence index. Required for imported
    -- rows; manual rows have neither key.
    fingerprint            TEXT    CHECK (fingerprint <> ''),
    source_category        TEXT,
    source_type            TEXT,
    -- The amount in the currency spent, when it differs (card abroad).
    original_amount_minor  INTEGER,
    original_currency      TEXT    CHECK (original_currency GLOB '[A-Z][A-Z][A-Z]'),
    -- The note from the bank export; read-only.
    note                   TEXT,
    origin                 TEXT    NOT NULL CHECK (origin IN ('imported', 'mirrored', 'manual')),
    created_at             TEXT    NOT NULL
                                   DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                                   CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- User-facing fields.
    -- Derived: cleaned bank text, then merchant aliases, then rule actions.
    merchant_display       TEXT,
    -- NULL when uncategorised or split.
    category_id            INTEGER REFERENCES category (id),
    category_source        TEXT    CHECK (category_source IN (
                                       'rule', 'preset_map', 'user', 'ai_approved')),
    -- The rule that set the category. Cleared when the rule is deleted;
    -- `category_source` stays `rule`.
    rule_id                INTEGER REFERENCES rule (id),
    transfer_group_id      INTEGER REFERENCES transfer_group (id),
    user_note              TEXT,
    -- The user's part of the amount, same sign; NULL means all of it.
    my_share_minor         INTEGER,
    -- A local label for who shares the cost; never leaves the app.
    share_label            TEXT    CHECK (share_label <> ''),
    reimbursable           INTEGER NOT NULL DEFAULT 0 CHECK (reimbursable IN (0, 1)),
    -- Excluded rows leave spending, income, budgets and reports;
    -- `duplicate` and `superseded` also leave the computed balance.
    excluded_reason        TEXT    CHECK (excluded_reason IN (
                                       'duplicate', 'bank_error', 'disputed', 'other',
                                       'superseded')),
    excluded_note          TEXT,
    excluded_at            TEXT    CHECK (excluded_at IS strftime('%Y-%m-%dT%H:%M:%fZ', excluded_at)),
    -- Who set `merchant_display` (V7): NULL when derived.
    merchant_display_source TEXT
    CHECK (merchant_display_source IS NULL
           OR (merchant_display_source IN ('rule', 'user')
               AND coalesce(trim(merchant_display), '') <> '')),
    -- The row that replaced this one: the bank's correction, imported with
    -- the same source ID. Deferred: the import sets it just before it
    -- inserts that row.
    superseded_by          INTEGER REFERENCES "transaction" (id) DEFERRABLE INITIALLY DEFERRED,

    CHECK ((original_amount_minor IS NULL) = (original_currency IS NULL)),
    -- Manual rows have no batch; imported and mirrored rows always do.
    CHECK ((origin = 'manual') = (batch_id IS NULL)),
    CHECK (origin <> 'imported' OR fingerprint IS NOT NULL),
    -- Manual rows aren't deduplicated. Without keys, an edit to one can't
    -- collide with (and so REPLACE) another row.
    CHECK (origin <> 'manual' OR (external_id IS NULL AND fingerprint IS NULL)),
    CHECK ((category_id IS NULL) = (category_source IS NULL)),
    CHECK (my_share_minor IS NULL
           OR (amount_minor >= 0 AND my_share_minor BETWEEN 0 AND amount_minor)
           OR (amount_minor < 0 AND my_share_minor BETWEEN amount_minor AND 0)),
    -- The exclusion fields are set and cleared together, and `other` needs
    -- a note.
    CHECK ((excluded_reason IS NULL) = (excluded_at IS NULL)),
    CHECK (excluded_reason IS NOT NULL OR excluded_note IS NULL),
    CHECK (excluded_reason IS NOT 'other' OR coalesce(trim(excluded_note), '') <> ''),
    -- A superseded row names the row that replaced it, and only a row with
    -- a source ID is superseded (by another row, with that ID).
    CHECK ((excluded_reason IS 'superseded') = (superseded_by IS NOT NULL)),
    CHECK (superseded_by IS NULL OR (external_id IS NOT NULL AND superseded_by <> id))
) STRICT;

INSERT INTO transaction_new (
    id, account_id, batch_id, date, time, amount_minor, currency, merchant,
    description_raw, description_normalized, external_id, fingerprint,
    source_category, source_type, original_amount_minor, original_currency, note,
    origin, created_at, merchant_display, category_id, category_source, rule_id,
    transfer_group_id, user_note, my_share_minor, share_label, reimbursable,
    excluded_reason, excluded_note, excluded_at, merchant_display_source)
SELECT
    id, account_id, batch_id, date, time, amount_minor, currency, merchant,
    description_raw, description_normalized, external_id, fingerprint,
    source_category, source_type, original_amount_minor, original_currency, note,
    origin, created_at, merchant_display, category_id, category_source, rule_id,
    transfer_group_id, user_note, my_share_minor, share_label, reimbursable,
    excluded_reason, excluded_note, excluded_at, merchant_display_source
FROM "transaction";

-- Transaction IDs deleted since (undone imports, deleted manual rows)
-- aren't handed out again.
UPDATE sqlite_sequence
SET seq = max(seq, (SELECT seq FROM sqlite_sequence WHERE name = 'transaction'))
WHERE name = 'transaction_new'
  AND EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'transaction');
INSERT INTO sqlite_sequence (name, seq)
SELECT 'transaction_new', seq FROM sqlite_sequence
WHERE name = 'transaction'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'transaction_new');

DROP TABLE "transaction";
ALTER TABLE transaction_new RENAME TO "transaction";

-- The table's indexes as V1 and V9 made them, but for the source ID index,
-- which counts only rows that aren't superseded.
CREATE INDEX transaction_account_date ON "transaction" (account_id, date);

CREATE INDEX transaction_batch ON "transaction" (batch_id);

CREATE INDEX transaction_category ON "transaction" (category_id);

CREATE INDEX transaction_rule ON "transaction" (rule_id);

CREATE INDEX transaction_transfer_group ON "transaction" (transfer_group_id);
CREATE UNIQUE INDEX transaction_external_id
    ON "transaction" (account_id, external_id)
    WHERE external_id IS NOT NULL AND superseded_by IS NULL;

CREATE UNIQUE INDEX transaction_fingerprint
    ON "transaction" (account_id, fingerprint)
    WHERE external_id IS NULL AND fingerprint IS NOT NULL;
CREATE INDEX transaction_reimbursable ON "transaction" (date) WHERE reimbursable = 1;

-- Also the index for the `superseded_by` foreign key. A row replaces at
-- most one other.
CREATE UNIQUE INDEX transaction_superseded_by
    ON "transaction" (superseded_by) WHERE superseded_by IS NOT NULL;

-- The table's triggers as V1, V5 and V9 made them, but for two.
-- `transaction_insert_new`: a superseded row doesn't hold its source ID
-- against a new row, and a new row isn't superseded.
CREATE TRIGGER transaction_insert_new
BEFORE INSERT ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'a transaction with this ID already exists')
    WHERE EXISTS (SELECT 1 FROM "transaction" WHERE id = NEW.id);
    SELECT RAISE(ABORT, 'a transaction with this source ID already exists on the account')
    WHERE NEW.external_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM "transaction"
                  WHERE account_id = NEW.account_id AND external_id = NEW.external_id
                    AND superseded_by IS NULL);
    SELECT RAISE(ABORT, 'a transaction with this fingerprint already exists on the account')
    WHERE NEW.external_id IS NULL AND NEW.fingerprint IS NOT NULL
      AND EXISTS (SELECT 1 FROM "transaction"
                  WHERE account_id = NEW.account_id AND external_id IS NULL
                    AND fingerprint = NEW.fingerprint);
    SELECT RAISE(ABORT, 'a new transaction isn''t superseded')
    WHERE NEW.superseded_by IS NOT NULL;
END;

CREATE TRIGGER transaction_account_insert
BEFORE INSERT ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch takes no transactions')
    FROM import_batch WHERE id = NEW.batch_id AND status = 'undone';
    SELECT RAISE(ABORT, 'asset and liability accounts hold valuations, not transactions')
    FROM account WHERE id = NEW.account_id AND kind IN ('asset', 'liability');
    SELECT RAISE(ABORT, 'a transaction is in its account''s currency')
    FROM account WHERE id = NEW.account_id AND currency IS NOT NEW.currency;
    SELECT RAISE(ABORT, 'an imported row belongs to its batch''s account')
    FROM import_batch
    WHERE NEW.origin = 'imported' AND id = NEW.batch_id AND account_id IS NOT NEW.account_id;
    SELECT RAISE(ABORT, 'a mirrored row belongs to a pot of its batch''s account')
    FROM import_batch
    WHERE NEW.origin = 'mirrored' AND id = NEW.batch_id
      AND account_id IS NOT (SELECT parent_id FROM account WHERE id = NEW.account_id);
END;

CREATE TRIGGER transaction_account_update
BEFORE UPDATE OF account_id, currency ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'asset and liability accounts hold valuations, not transactions')
    FROM account WHERE id = NEW.account_id AND kind IN ('asset', 'liability');
    SELECT RAISE(ABORT, 'a transaction is in its account''s currency')
    FROM account WHERE id = NEW.account_id AND currency IS NOT NEW.currency;
END;

-- `transaction_source_fields_read_only`: as before, and `superseded_by`
-- changes only together with the exclusion reason, so a superseded row is
-- never pointed at another row.
CREATE TRIGGER transaction_source_fields_read_only
BEFORE UPDATE ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'imported source fields are read-only')
    WHERE NEW.id IS NOT OLD.id
      OR NEW.origin IS NOT OLD.origin
      OR NEW.created_at IS NOT OLD.created_at
      OR (OLD.origin <> 'manual' AND (
             NEW.account_id IS NOT OLD.account_id
          OR NEW.batch_id IS NOT OLD.batch_id
          OR NEW.date IS NOT OLD.date
          OR NEW.time IS NOT OLD.time
          OR NEW.amount_minor IS NOT OLD.amount_minor
          OR NEW.currency IS NOT OLD.currency
          OR NEW.merchant IS NOT OLD.merchant
          OR NEW.description_raw IS NOT OLD.description_raw
          OR NEW.description_normalized IS NOT OLD.description_normalized
          OR NEW.external_id IS NOT OLD.external_id
          OR NEW.fingerprint IS NOT OLD.fingerprint
          OR NEW.source_category IS NOT OLD.source_category
          OR NEW.source_type IS NOT OLD.source_type
          OR NEW.original_amount_minor IS NOT OLD.original_amount_minor
          OR NEW.original_currency IS NOT OLD.original_currency
          OR NEW.note IS NOT OLD.note));
    SELECT RAISE(ABORT, 'a row is superseded, or included again, only with its exclusion reason')
    WHERE NEW.superseded_by IS NOT OLD.superseded_by
      AND NEW.excluded_reason IS OLD.excluded_reason;
END;

CREATE TRIGGER transaction_split_parent
BEFORE UPDATE OF category_id, my_share_minor, share_label, amount_minor, currency,
                 transfer_group_id ON "transaction"
WHEN EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'a split transaction has no category of its own: its lines do')
    WHERE NEW.category_id IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction has no share of its own: its lines do')
    WHERE NEW.my_share_minor IS NOT NULL OR NEW.share_label IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction''s amount and currency can''t change; remove the split first')
    WHERE NEW.amount_minor IS NOT OLD.amount_minor OR NEW.currency IS NOT OLD.currency;
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    FROM transfer_group
    WHERE NEW.transfer_group_id IS NOT OLD.transfer_group_id
      AND id = NEW.transfer_group_id AND status <> 'rejected';
END;

CREATE TRIGGER transaction_link_amount
BEFORE UPDATE OF amount_minor, currency ON "transaction"
WHEN (NEW.amount_minor IS NOT OLD.amount_minor OR NEW.currency IS NOT OLD.currency)
 AND (EXISTS (SELECT 1 FROM transaction_link WHERE inflow_id = OLD.id AND status <> 'rejected')
      OR EXISTS (SELECT 1 FROM transaction_link
                 WHERE target_id = OLD.id AND status <> 'rejected'))
BEGIN
    SELECT RAISE(ABORT, 'a linked transaction''s amount and currency can''t change; unlink it first');
END;

CREATE TRIGGER transaction_link_inflow_share
BEFORE UPDATE OF my_share_minor, share_label ON "transaction"
WHEN (NEW.my_share_minor IS NOT NULL OR NEW.share_label IS NOT NULL)
 AND EXISTS (SELECT 1 FROM transaction_link WHERE inflow_id = OLD.id AND status <> 'rejected')
BEGIN
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own');
END;

-- A superseded row and the row that replaced it share an account and a
-- source ID. Checked from both sides, since either can come first: when a
-- row is pointed at one that exists, and when a row is inserted under an ID
-- a stored row already points at. The second is an AFTER trigger: before
-- the insert, `NEW.id` isn't known when SQLite picks it.
CREATE TRIGGER transaction_superseded_by_update
BEFORE UPDATE OF superseded_by ON "transaction"
WHEN NEW.superseded_by IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a row is superseded by a row with its account and source ID')
    FROM "transaction"
    WHERE id = NEW.superseded_by
      AND (account_id IS NOT NEW.account_id OR external_id IS NOT NEW.external_id);
END;

CREATE TRIGGER transaction_superseded_by_insert
AFTER INSERT ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'a row is superseded by a row with its account and source ID')
    FROM "transaction"
    WHERE superseded_by = NEW.id
      AND (account_id IS NOT NEW.account_id OR external_id IS NOT NEW.external_id);
END;

-- The triggers and the view of other tables that name `"transaction"`,
-- exactly as they were.

CREATE TRIGGER import_batch_status
BEFORE UPDATE OF status ON import_batch
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch can''t be committed again')
    WHERE OLD.status = 'undone' AND NEW.status IS NOT 'undone';
    SELECT RAISE(ABORT, 'an import batch is undone only once its transactions and balances are deleted')
    WHERE OLD.status = 'committed' AND NEW.status = 'undone'
      AND (EXISTS (SELECT 1 FROM "transaction" WHERE batch_id = OLD.id)
           OR EXISTS (SELECT 1 FROM balance_snapshot WHERE batch_id = OLD.id));
END;

CREATE TRIGGER account_pot_parent
BEFORE UPDATE OF parent_id ON account
WHEN OLD.parent_id IS NOT NULL AND NEW.parent_id IS NOT OLD.parent_id
BEGIN
    SELECT RAISE(ABORT, 'a pot with transactions or balances can''t change parent')
    WHERE EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id)
       OR EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id);
END;

CREATE TRIGGER account_kind_and_currency
BEFORE UPDATE OF kind, currency ON account
BEGIN
    SELECT RAISE(ABORT, 'an account''s currency can''t change once it has transactions, balances, pots or rules with amounts')
    WHERE NEW.currency IS NOT OLD.currency
      AND (EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id)
           OR EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id)
           OR EXISTS (SELECT 1 FROM account WHERE parent_id = OLD.id)
           OR EXISTS (SELECT 1 FROM rule
                      WHERE account_id = OLD.id
                        AND (amount_min IS NOT NULL OR amount_max IS NOT NULL)));
    SELECT RAISE(ABORT, 'an account with transactions can''t become an asset or liability')
    WHERE NEW.kind IN ('asset', 'liability')
      AND EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id);
    SELECT RAISE(ABORT, 'an account with balances can''t move in or out of asset and liability')
    WHERE (NEW.kind IN ('asset', 'liability')) <> (OLD.kind IN ('asset', 'liability'))
      AND EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id);
    SELECT RAISE(ABORT, 'an account with pots must stay a current or savings account')
    WHERE NEW.kind NOT IN ('current', 'savings')
      AND EXISTS (SELECT 1 FROM account WHERE parent_id = OLD.id);
END;

CREATE TRIGGER transaction_split_insert
BEFORE INSERT ON transaction_split
BEGIN
    SELECT RAISE(ABORT, 'a split line has its transaction''s sign')
    FROM "transaction"
    WHERE id = NEW.transaction_id
      AND (amount_minor = 0 OR (amount_minor > 0) <> (NEW.amount_minor > 0));
    SELECT RAISE(ABORT, 'a split transaction has no category of its own: its lines do')
    FROM "transaction"
    WHERE id = NEW.transaction_id AND category_id IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction has no share of its own: its lines do')
    FROM "transaction"
    WHERE id = NEW.transaction_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    FROM "transaction" t JOIN transfer_group g ON g.id = t.transfer_group_id
    WHERE t.id = NEW.transaction_id AND g.status <> 'rejected';
END;

CREATE TRIGGER transaction_split_update
BEFORE UPDATE ON transaction_split
BEGIN
    SELECT RAISE(ABORT, 'a split line''s ID and transaction are read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.transaction_id IS NOT OLD.transaction_id;
    SELECT RAISE(ABORT, 'a split line has its transaction''s sign')
    FROM "transaction"
    WHERE NEW.amount_minor IS NOT OLD.amount_minor
      AND id = NEW.transaction_id
      AND (amount_minor > 0) <> (NEW.amount_minor > 0);
END;

CREATE TRIGGER transfer_group_split
BEFORE UPDATE OF status ON transfer_group
WHEN NEW.status <> 'rejected'
BEGIN
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    WHERE EXISTS (SELECT 1 FROM "transaction" t
                  JOIN transaction_split s ON s.transaction_id = t.id
                  WHERE t.transfer_group_id = NEW.id);
END;

CREATE TRIGGER transaction_link_insert
BEFORE INSERT ON transaction_link
BEGIN
    SELECT RAISE(ABORT, 'a link''s inflow is money in')
    FROM "transaction" WHERE id = NEW.inflow_id AND amount_minor <= 0;
    SELECT RAISE(ABORT, 'a refund or reimbursement pays back money out')
    FROM "transaction" WHERE id = NEW.target_id AND amount_minor >= 0;
    SELECT RAISE(ABORT, 'both sides of a link are in one currency')
    FROM "transaction" r, "transaction" p
    WHERE r.id = NEW.inflow_id AND p.id = NEW.target_id AND r.currency IS NOT p.currency;
    SELECT RAISE(ABORT, 'a linked purchase is at most 3037000499 minor units')
    FROM "transaction" WHERE id = NEW.target_id AND amount_minor < -3037000499;
    SELECT RAISE(ABORT, 'a link is at most the money in')
    FROM "transaction" WHERE id = NEW.inflow_id AND NEW.amount_minor > amount_minor;
    SELECT RAISE(ABORT, 'a link is at most the purchase')
    FROM "transaction" WHERE id = NEW.target_id AND NEW.amount_minor > -amount_minor;
    SELECT RAISE(ABORT, 'a reimbursement repays a purchase marked to claim back')
    FROM "transaction"
    WHERE NEW.kind = 'reimbursement' AND NEW.status <> 'rejected'
      AND id = NEW.target_id AND reimbursable = 0;
    SELECT RAISE(ABORT, 'a linked inflow can''t be split')
    WHERE NEW.status <> 'rejected'
      AND EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = NEW.inflow_id);
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'confirmed links take more than the money in')
    FROM "transaction" r
    WHERE NEW.status = 'confirmed' AND r.id = NEW.inflow_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE inflow_id = NEW.inflow_id AND status = 'confirmed')
          > r.amount_minor;
    SELECT RAISE(ABORT, 'confirmed links take more than the purchase')
    FROM "transaction" p
    WHERE NEW.status = 'confirmed' AND p.id = NEW.target_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE target_id = NEW.target_id AND status = 'confirmed')
          > -p.amount_minor;
END;

CREATE TRIGGER transaction_link_update
BEFORE UPDATE ON transaction_link
BEGIN
    SELECT RAISE(ABORT, 'a link''s kind, sides and date are read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.kind IS NOT OLD.kind
       OR NEW.inflow_id IS NOT OLD.inflow_id OR NEW.target_id IS NOT OLD.target_id
       OR NEW.share_label IS NOT OLD.share_label OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'a rejected link isn''t suggested again')
    WHERE OLD.status = 'rejected' AND NEW.status = 'suggested';
    -- The same checks as on insert, for a link that counts or may.
    SELECT RAISE(ABORT, 'a link''s inflow is money in')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id AND amount_minor <= 0;
    SELECT RAISE(ABORT, 'a refund or reimbursement pays back money out')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND amount_minor >= 0;
    SELECT RAISE(ABORT, 'both sides of a link are in one currency')
    FROM "transaction" r, "transaction" p
    WHERE NEW.status <> 'rejected' AND r.id = NEW.inflow_id AND p.id = NEW.target_id
      AND r.currency IS NOT p.currency;
    SELECT RAISE(ABORT, 'a linked purchase is at most 3037000499 minor units')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND amount_minor < -3037000499;
    SELECT RAISE(ABORT, 'a link is at most the money in')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id AND NEW.amount_minor > amount_minor;
    SELECT RAISE(ABORT, 'a link is at most the purchase')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND NEW.amount_minor > -amount_minor;
    SELECT RAISE(ABORT, 'a reimbursement repays a purchase marked to claim back')
    FROM "transaction"
    WHERE NEW.kind = 'reimbursement' AND NEW.status <> 'rejected'
      AND id = NEW.target_id AND reimbursable = 0;
    SELECT RAISE(ABORT, 'a linked inflow can''t be split')
    WHERE NEW.status <> 'rejected'
      AND EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = NEW.inflow_id);
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'confirmed links take more than the money in')
    FROM "transaction" r
    WHERE NEW.status = 'confirmed' AND r.id = NEW.inflow_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE inflow_id = NEW.inflow_id AND status = 'confirmed'
                                AND id IS NOT OLD.id)
          > r.amount_minor;
    SELECT RAISE(ABORT, 'confirmed links take more than the purchase')
    FROM "transaction" p
    WHERE NEW.status = 'confirmed' AND p.id = NEW.target_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE target_id = NEW.target_id AND status = 'confirmed'
                                AND id IS NOT OLD.id)
          > -p.amount_minor;
END;

CREATE VIEW ledger_line AS
WITH
confirmed AS (
    SELECT id, kind, inflow_id, target_id, share_label, amount_minor
    FROM transaction_link
    WHERE status = 'confirmed'
),
inflow_linked AS (
    SELECT inflow_id, sum(amount_minor) AS linked_minor
    FROM confirmed
    GROUP BY inflow_id
),
split_part AS (
    SELECT l.id         AS link_id,
           s.id         AS split_id,
           s.amount_minor AS line_minor,
           s.category_id  AS category_id,
           s.my_share_minor AS line_share_minor,
           s.share_label  AS share_label,
           l.amount_minor * -s.amount_minor / -p.amount_minor
             + (row_number() OVER (
                    PARTITION BY l.id
                    ORDER BY (l.amount_minor * -s.amount_minor) % -p.amount_minor DESC,
                             s.line_no)
                <= l.amount_minor
                   - sum(l.amount_minor * -s.amount_minor / -p.amount_minor)
                         OVER (PARTITION BY l.id))
                AS part_minor
    FROM confirmed l
    JOIN "transaction" p ON p.id = l.target_id
    JOIN transaction_split s ON s.transaction_id = p.id
    WHERE l.kind IN ('refund', 'reimbursement')
),
line AS (
    -- 1. A transaction's own row.
    SELECT t.id                AS transaction_id,
           NULL                AS split_id,
           NULL                AS line_no,
           t.account_id        AS account_id,
           t.batch_id          AS batch_id,
           t.date              AS date,
           t.amount_minor - coalesce(i.linked_minor, 0) AS amount_minor,
           t.currency          AS currency,
           t.category_id       AS category_id,
           t.my_share_minor    AS my_share_minor,
           coalesce(t.my_share_minor, t.amount_minor - coalesce(i.linked_minor, 0))
                               AS my_amount_minor,
           t.share_label       AS share_label,
           t.reimbursable      AS reimbursable,
           t.merchant          AS merchant,
           t.merchant_display  AS merchant_display,
           t.excluded_reason   AS excluded_reason,
           t.transfer_group_id AS transfer_group_id,
           g.status            AS transfer_status,
           t.origin            AS origin,
           NULL                AS link_id,
           NULL                AS link_kind,
           NULL                AS target_id,
           NULL                AS target_split_id,
           (t.excluded_reason IS NULL AND g.status IS NOT 'confirmed') AS in_reports
    FROM "transaction" t
    LEFT JOIN transfer_group g ON g.id = t.transfer_group_id
    LEFT JOIN inflow_linked i ON i.inflow_id = t.id
    WHERE NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id)
    UNION ALL
    -- 2. Split lines.
    SELECT t.id,
           s.id,
           s.line_no,
           t.account_id,
           t.batch_id,
           t.date,
           s.amount_minor,
           t.currency,
           s.category_id,
           s.my_share_minor,
           coalesce(s.my_share_minor, s.amount_minor),
           s.share_label,
           t.reimbursable,
           t.merchant,
           t.merchant_display,
           t.excluded_reason,
           t.transfer_group_id,
           g.status,
           t.origin,
           NULL,
           NULL,
           NULL,
           NULL,
           (t.excluded_reason IS NULL AND g.status IS NOT 'confirmed')
    FROM transaction_split s
    JOIN "transaction" t ON t.id = s.transaction_id
    LEFT JOIN transfer_group g ON g.id = t.transfer_group_id
    UNION ALL
    -- 3. A refund or reimbursement against a purchase without lines.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           l.amount_minor,
           r.currency,
           p.category_id,
           l.amount_minor * p.my_share_minor / p.amount_minor,
           coalesce(l.amount_minor * p.my_share_minor / p.amount_minor, l.amount_minor),
           p.share_label,
           p.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           p.id,
           NULL,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed'
            AND p.excluded_reason IS NULL AND pg.status IS NOT 'confirmed')
    FROM confirmed l
    JOIN "transaction" r ON r.id = l.inflow_id
    JOIN "transaction" p ON p.id = l.target_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    LEFT JOIN transfer_group pg ON pg.id = p.transfer_group_id
    WHERE l.kind IN ('refund', 'reimbursement')
      AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = p.id)
    UNION ALL
    -- 4. A refund or reimbursement against a split purchase, per line.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           sp.part_minor,
           r.currency,
           sp.category_id,
           sp.part_minor * sp.line_share_minor / sp.line_minor,
           coalesce(sp.part_minor * sp.line_share_minor / sp.line_minor, sp.part_minor),
           sp.share_label,
           p.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           p.id,
           sp.split_id,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed'
            AND p.excluded_reason IS NULL AND pg.status IS NOT 'confirmed')
    FROM split_part sp
    JOIN confirmed l ON l.id = sp.link_id
    JOIN "transaction" r ON r.id = l.inflow_id
    JOIN "transaction" p ON p.id = l.target_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    LEFT JOIN transfer_group pg ON pg.id = p.transfer_group_id
    WHERE sp.part_minor <> 0
    UNION ALL
    -- 5. A settlement.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           l.amount_minor,
           r.currency,
           NULL,
           0,
           0,
           l.share_label,
           r.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           NULL,
           NULL,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed')
    FROM confirmed l
    JOIN "transaction" r ON r.id = l.inflow_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    WHERE l.kind = 'settlement'
)
SELECT transaction_id,
       split_id,
       line_no,
       account_id,
       batch_id,
       date,
       amount_minor,
       currency,
       category_id,
       my_share_minor,
       my_amount_minor,
       share_label,
       reimbursable,
       merchant,
       merchant_display,
       excluded_reason,
       transfer_group_id,
       transfer_status,
       origin,
       link_id,
       link_kind,
       target_id,
       target_split_id,
       in_reports,
       CASE WHEN in_reports AND reimbursable = 0 THEN my_amount_minor ELSE 0 END
           AS personal_minor
FROM line;

CREATE TRIGGER duplicate_candidate_insert
BEFORE INSERT ON duplicate_candidate
BEGIN
    SELECT RAISE(ABORT, 'a duplicate candidate''s rows are on one account')
    FROM "transaction" a, "transaction" b
    WHERE a.id = NEW.transaction_a AND b.id = NEW.transaction_b
      AND a.account_id IS NOT b.account_id;
    SELECT RAISE(ABORT, 'a new duplicate candidate is pending')
    WHERE NEW.status IS NOT 'pending';
    SELECT RAISE(ABORT, 'a duplicate candidate is merged by a duplicate_merge batch')
    FROM change_batch
    WHERE id = NEW.change_batch_id AND kind <> 'duplicate_merge';
END;

CREATE TRIGGER schedule_match_insert
BEFORE INSERT ON schedule_match
BEGIN
    SELECT RAISE(ABORT, 'a schedule matches transactions on its own account')
    FROM schedule s, "transaction" t
    WHERE s.id = NEW.schedule_id AND t.id = NEW.transaction_id
      AND s.account_id IS NOT t.account_id;
    SELECT RAISE(ABORT, 'a skipped occurrence isn''t matched')
    FROM schedule_skip
    WHERE schedule_id = NEW.schedule_id AND occurrence_date = NEW.occurrence_date;
END;
