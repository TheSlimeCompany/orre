# Migrations

SQL migrations for Orre's SQLite database, embedded into `orre-core` at
compile time by `refinery` and applied by `orre_core::db::Database::open` (the
app and the CLI). `Database::open_existing` (`orre mcp`) never applies them: it
refuses a database with pending migrations.

- Name files `V<n>__<snake_case_name>.sql`, with `<n>` one higher than the last
  file (`V1__accounts.sql`, `V2__…`). Only `V` (versioned) migrations are used.
- **Never edit a migration that has shipped.** `refinery` checksums every
  applied migration and refuses to open a database whose history differs from
  these files. A schema change is always a new file (`AGENTS.md` → Invariants).
- All pending migrations run in one transaction: a failure leaves the database
  at its previous version.
- A database at a newer version than this build knows, or whose applied migrations
  differ from these files, is refused before anything is written: its contents,
  permissions and journal mode stay as they were.
- Follow the conventions at the top of `V1__initial_schema.sql` (`STRICT` tables,
  checked dates, no `ON DELETE` actions, indexed foreign keys, cross-row rules as
  `BEFORE` triggers with indexed lookups), and add new tables to `TABLES` in
  `crates/core/src/db/schema_tests.rs`.
- Never write `"transaction"` with `REPLACE`, `INSERT OR REPLACE`, `UPDATE OR REPLACE`
  or an upsert (`ON CONFLICT`). REPLACE deletes the conflicting row and inserts a new
  one, which no `UPDATE` trigger sees, so it would get round the read-only source
  fields. The `transaction_insert_new` trigger refuses an insert that collides with a
  stored row, and `no_replace_on_transactions` in `schema_tests.rs` scans
  `crates/core/src` for such statements. A new table with read-only fields needs the
  same protection.
- Adding a column or an index is a plain `ALTER TABLE` / `CREATE INDEX`. Changing a
  `CHECK` (such as adding an enum key) means rebuilding the table (SQLite's "12-step"
  procedure). A table nothing references can be rebuilt with foreign keys on
  (`V17__change_item_links.sql`). One that other tables reference needs them off,
  which `PRAGMA foreign_keys` can't do inside the run's transaction, so the migration
  asks for it with this exact first line:

  ```sql
  -- orre: foreign_keys=off
  ```

  The marker, not the version number, is what counts, so renumbering a migration
  changes nothing. When any pending migration has it, `db::migrations::run` first makes
  the same history checks refinery makes (a newer or diverged database is still refused
  before anything is written), turns foreign keys off, runs every pending migration in
  its one transaction, and checks every foreign key in the database
  (`pragma_foreign_key_check`) inside that transaction, as the last migration is
  recorded: a reference to nothing fails the run and rolls every pending migration
  back. Foreign keys are on again afterwards, whether the run succeeded or not.
  `V21__superseded.sql` is the example. Such a migration still follows the 12-step
  procedure: build `<table>_new`, copy every row with its ID, carry the AUTOINCREMENT
  sequence over (`sqlite_sequence`) so a deleted ID is never handed out again, drop the
  old table, rename the new one, and recreate the table's indexes and triggers. Renaming
  re-parses every trigger and view that names the table, so drop those of other tables
  first and recreate them afterwards, exactly as they were (copy their SQL from
  `sqlite_schema` of a database at the previous version).
