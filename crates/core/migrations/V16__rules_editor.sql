-- What the Rules view (ROADMAP M3.13; `docs/DESIGN.md` → "Rules", D6.7) shows
-- that nothing stored before: where a rule came from, which rule a
-- `rule_apply` change batch applied, and the "Create a rule from this edit?"
-- prompts the user turned down (`docs/ARCHITECTURE.md` → "Decision record:
-- the Rules view (built)").

-- Where a rule came from, for the inspector's footer ("Created from an edit
-- on 12 Mar"): `edit` from the Transactions prompt after a category edit,
-- `inbox` from an approved suggestion, `new` from New rule. NULL for rules
-- stored before this migration, whose origin wasn't recorded.
ALTER TABLE rule ADD COLUMN origin TEXT
    CHECK (origin IS NULL OR origin IN ('new', 'edit', 'inbox'));

-- The rule a `rule_apply` batch applied, so Change history can name it. Not a
-- foreign key: a batch is a record that outlives its rule (deleting a rule
-- leaves its batches, and an undone batch can't be updated, V8), and rule IDs
-- are never reused (AUTOINCREMENT), so an ID whose rule is gone reads as "a
-- deleted rule". NULL for other kinds, and for batches from before this
-- migration.
ALTER TABLE change_batch ADD COLUMN rule_id INTEGER
    CHECK (rule_id IS NULL OR kind = 'rule_apply');

-- "Not now" on the Transactions prompt, remembered per proposed pattern so the
-- prompt doesn't come back for it. The prompt always proposes `contains`, so a
-- pattern is its field and its text folded as rule matching folds it
-- (`orre_core::rule`): what matches the same rows is turned down once. The
-- text is the user's bank text, like a rule's pattern: never logged.
CREATE TABLE rule_prompt_dismissed (
    match_field  TEXT NOT NULL CHECK (match_field IN (
                          'merchant', 'description', 'source_category', 'source_type')),
    pattern      TEXT NOT NULL CHECK (pattern <> ''),
    dismissed_at TEXT NOT NULL
                      DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                      CHECK (dismissed_at IS strftime('%Y-%m-%dT%H:%M:%fZ', dismissed_at)),

    PRIMARY KEY (match_field, pattern)
) STRICT, WITHOUT ROWID;
