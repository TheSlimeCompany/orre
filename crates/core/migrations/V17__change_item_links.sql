-- Link edits as change batches (ROADMAP M3.11; `docs/ARCHITECTURE.md` →
-- "Decision record: refunds, reimbursements and settlements (built)").
--
-- The inspector's link edits (link, unlink, "Not a refund", a reimbursement
-- confirmed from Owed to you, renaming a person) are each one undoable
-- change batch, like M3.9's edits. A `change_item` gains a field, `links`:
-- the links whose money in is the item's transaction (`transaction_link`
-- rows with that `inflow_id`, in any status). Every link has exactly one
-- inflow, so each is recorded once, on it.
--
-- Adding the key changes `change_item.field`'s CHECK, so the table is
-- rebuilt. Nothing references `change_item` (its foreign keys point out),
-- so foreign keys can stay on: the new table is filled from the old one,
-- the old one dropped (its index and triggers go with it) and the new one
-- renamed. Every row, ID and the AUTOINCREMENT sequence carry over; the
-- index and triggers are V8's, unchanged.

CREATE TABLE change_item_new (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id       INTEGER NOT NULL REFERENCES change_batch (id),
    transaction_id INTEGER NOT NULL REFERENCES "transaction" (id),
    field          TEXT    NOT NULL CHECK (field IN (
                               'category', 'merchant', 'user_note', 'tags', 'exclusion',
                               'split', 'share', 'reimbursable', 'links')),
    old_value      TEXT    NOT NULL CHECK (json_valid(old_value)),
    new_value      TEXT    NOT NULL CHECK (json_valid(new_value)),

    -- Also the index for the `batch_id` foreign key.
    UNIQUE (batch_id, transaction_id, field),
    CHECK (old_value IS NOT new_value)
) STRICT;

INSERT INTO change_item_new (id, batch_id, transaction_id, field, old_value, new_value)
SELECT id, batch_id, transaction_id, field, old_value, new_value FROM change_item;

-- Item IDs deleted since (with their transactions) aren't handed out again.
UPDATE sqlite_sequence
SET seq = max(seq, (SELECT seq FROM sqlite_sequence WHERE name = 'change_item'))
WHERE name = 'change_item_new'
  AND EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'change_item');
INSERT INTO sqlite_sequence (name, seq)
SELECT 'change_item_new', seq FROM sqlite_sequence
WHERE name = 'change_item'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'change_item_new');

DROP TABLE change_item;
ALTER TABLE change_item_new RENAME TO change_item;

CREATE INDEX change_item_transaction ON change_item (transaction_id);

-- A batch's items are a record (V8).
CREATE TRIGGER change_item_read_only
BEFORE UPDATE ON change_item
BEGIN
    SELECT RAISE(ABORT, 'a change item is read-only');
END;

CREATE TRIGGER change_item_insert
BEFORE INSERT ON change_item
BEGIN
    SELECT RAISE(ABORT, 'an undone change batch takes no items')
    FROM change_batch WHERE id = NEW.batch_id AND status = 'undone';
END;
