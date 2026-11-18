-- A third kind of duplicate candidate: `unmatched_source_id` (ROADMAP R.3;
-- `docs/architecture/links-and-checks.md`, "a source ID is checked against
-- the row's content").
--
-- A source ID is trusted with no content check, so a bank that regenerates
-- its IDs on each download, or a file imported again through a mapping
-- without its ID column, stores every overlapping row twice. Such a pair is
-- two imported rows from different imports with the same date, amount and
-- text, where at least one has a source ID and the IDs differ. Detection
-- (`coffer_core::duplicate`) records it with this reason.
--
-- The reason's CHECK has to accept the new key, and SQLite can't change a
-- CHECK in place, so the table is rebuilt: the same columns, constraints,
-- indexes and triggers as `V10__duplicate_candidate.sql`, with the one list
-- longer. No table or view references `duplicate_candidate`, so the rebuild
-- is safe with foreign keys on, and needs no `foreign_keys=off` marker (it
-- runs with them off anyway when a later pending migration asks). The rows
-- keep their IDs, and the ID counter keeps its place.

CREATE TABLE duplicate_candidate_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The lower transaction ID of the pair.
    transaction_a   INTEGER NOT NULL REFERENCES "transaction" (id),
    -- The higher one.
    transaction_b   INTEGER NOT NULL REFERENCES "transaction" (id),
    -- Which kind of match it is: two imported rows from different imports
    -- (`overlapping_import`), a manual row and an imported one
    -- (`manual_entry`), or two imported rows with the same date, amount and
    -- text whose source IDs differ (`unmatched_source_id`).
    reason          TEXT    NOT NULL CHECK (reason IN (
                                'overlapping_import', 'manual_entry', 'unmatched_source_id')),
    status          TEXT    NOT NULL DEFAULT 'pending'
                            CHECK (status IN ('pending', 'merged', 'dismissed')),
    -- The side that stayed when merged: `a` or `b`. The other is excluded.
    kept            TEXT    CHECK (kept IN ('a', 'b')),
    -- The `duplicate_merge` batch that merged it. One batch can merge many.
    change_batch_id INTEGER REFERENCES change_batch (id),
    created_at      TEXT    NOT NULL
                            DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                            CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- Also the index for the `transaction_a` foreign key.
    UNIQUE (transaction_a, transaction_b),
    CHECK (transaction_a < transaction_b),
    CHECK ((status = 'merged') = (kept IS NOT NULL)),
    CHECK ((status = 'merged') = (change_batch_id IS NOT NULL))
) STRICT;

INSERT INTO duplicate_candidate_new
    (id, transaction_a, transaction_b, reason, status, kept, change_batch_id, created_at)
SELECT id, transaction_a, transaction_b, reason, status, kept, change_batch_id, created_at
FROM duplicate_candidate;

-- AUTOINCREMENT's counter: a deleted candidate's ID isn't handed out again.
DELETE FROM sqlite_sequence WHERE name = 'duplicate_candidate_new';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'duplicate_candidate_new', seq FROM sqlite_sequence WHERE name = 'duplicate_candidate';

-- Takes the old table's indexes and triggers with it.
DROP TABLE duplicate_candidate;
ALTER TABLE duplicate_candidate_new RENAME TO duplicate_candidate;

CREATE INDEX duplicate_candidate_transaction_b ON duplicate_candidate (transaction_b);
CREATE INDEX duplicate_candidate_change_batch ON duplicate_candidate (change_batch_id);
CREATE INDEX duplicate_candidate_status ON duplicate_candidate (status);

-- Both rows are on one account: a duplicate is the same transaction stored
-- twice in one account. (A manual row can move to another account later;
-- merging checks the pair again.)
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

-- The pair, its reason and date never change. Only the status moves:
-- pending → merged → pending (undo), pending → dismissed → pending.
CREATE TRIGGER duplicate_candidate_update
BEFORE UPDATE ON duplicate_candidate
BEGIN
    SELECT RAISE(ABORT, 'a duplicate candidate''s pair, reason and date are read-only')
    WHERE NEW.id IS NOT OLD.id
       OR NEW.transaction_a IS NOT OLD.transaction_a
       OR NEW.transaction_b IS NOT OLD.transaction_b
       OR NEW.reason IS NOT OLD.reason
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'only a pending duplicate candidate is merged or dismissed')
    WHERE NEW.status IN ('merged', 'dismissed') AND OLD.status IS NOT 'pending';
    SELECT RAISE(ABORT, 'a duplicate candidate is merged by a duplicate_merge batch')
    FROM change_batch
    WHERE id = NEW.change_batch_id AND kind <> 'duplicate_merge';
END;
