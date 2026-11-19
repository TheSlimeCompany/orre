-- Category keys are never reused (ROADMAP R.11; `docs/ARCHITECTURE.md` →
-- "Decision record: category keys are never reused").
--
-- Keys are forever, but a deleted category's key used to be free again, so a
-- later category (a preset's key, a new default, another user category) could
-- take it and read as the deleted one to anything that still named it: a rule,
-- a preset, an MCP client, an old export.
--
-- `category_key_retired` keeps the key of every category that is deleted. The
-- trigger fires on every delete, from any code path. A key is taken while a
-- category has it or it is retired; the categories editor and a later
-- migration that adds a default both check the two. A category deleted before
-- this migration left no record and can't be recovered.

CREATE TABLE IF NOT EXISTS category_key_retired (
    key        TEXT NOT NULL PRIMARY KEY
                    CHECK (key <> ''
                           AND key NOT GLOB '*[^a-z0-9_.]*'
                           AND key NOT GLOB '.*'
                           AND key NOT GLOB '*.'
                           AND key NOT GLOB '*.*.*'),
    retired_at TEXT NOT NULL
                    DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                    CHECK (retired_at IS strftime('%Y-%m-%dT%H:%M:%fZ', retired_at))
) STRICT;

CREATE TRIGGER IF NOT EXISTS category_key_retire
AFTER DELETE ON category
BEGIN
    INSERT OR IGNORE INTO category_key_retired (key) VALUES (OLD.key);
END;
