-- The access level a report was saved at (ROADMAP R.2; `docs/ARCHITECTURE.md`
-- → "Decision record: a client reads only its own reports").
--
-- `list_reports` returns a client only its own reports, and only those saved
-- at a level no higher than the client's current one, so a client lowered
-- from `standard` to `aggregate` doesn't read back the review it wrote while
-- it could see merchant names. `save_report` records the caller's level here.
--
-- NULL is a report saved before this column existed. Its level is unknown, so
-- it is read as `standard`, the strictest reading. The app and the CLI show
-- every report whatever its level.

ALTER TABLE report ADD COLUMN access_level TEXT
    CHECK (access_level IN ('aggregate', 'pseudonymous', 'standard'));
