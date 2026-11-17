# Coffer — Architecture: Settings, AI access and backups

The Settings view, AI clients and their access, local backups and restore.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Decision records

Oldest first.

### Decision record: Settings, AI access and backups (2026-11-10)

The open questions from the D6.8 designs (`DESIGN.md` → "Settings"), decided by the user
on the designer's recommendation:

1. **The kill switch is read on every call.** It's a `setting` that the server reads on
   every call, not only at start, so turning it off refuses the next call from a client
   that is already connected. Refused calls are logged: `mcp_access_log` gains `outcome
   [ok|refused_off|refused_not_enabled|refused_level]` (a new migration).
2. **Unknown clients are recorded as requested.** The first `coffer mcp --client X` with
   an unknown name creates an `mcp_client` row that isn't enabled and is marked
   `requested` (a new migration), and the call is refused. "Ignore" hides the row without
   enabling it. Only the app or CLI enables a client, never the server, and enabling one
   starts it at `aggregate`.
3. **A lower level applies at once.** The server checks the client's level on every call
   and refuses tools above it. It also sends `notifications/tools/list_changed`, so the
   client drops those tools from its list.
4. **Argument redaction keeps the structure.** `args_redacted_json` keeps keys, dates,
   enum values, numbers and IDs. Free-text values are stored only as their type and
   length.
5. **The access log is kept for 12 months.** That's a default, not an invariant; the rows
   are small, one per call.
6. **Backup metadata lives in the file name.**
   - A backup is also taken before every restore and on "Back up now". The reason and
     time live in the file name, so the list needs no table that a restore would roll
     back.
   - A backup with an older schema is restored and then migrated; a newer one is refused.
   - While a restore swaps the files, running MCP servers refuse calls. They reopen the
     database afterwards.
   - A restore doesn't roll the access log back (added 2026-11-13, user): the log
     records what AI clients did, which restoring data doesn't undo. Rows newer than the
     backup's newest are carried over from the database it replaced, after the restored
     file is migrated, and the 12-month retention applied (decision record "M2: MCP
     server", F.9).

### Decision record: local backups (2026-11-11)

Decided while building M1.28 (`coffer_core::backup`, `coffer backup`, `coffer restore`).
Retention and location were decided by the user.

1. **Where and how.** Backups are `VACUUM INTO` snapshots in `backups/` next to the
   database (`<data dir>/backups`, `0700`; files `0600`), derived from the database
   path, so `COFFER_DATA_DIR` moves them too. Each is written to a hidden temporary name,
   fsynced, then hard-linked to its final name (a rename after checking the name is free
   where hard links aren't supported), so a partial file never looks like a backup and no
   backup is overwritten.
2. **The name is the metadata.** `coffer-<UTC yyyymmddThhmmss.mmmZ>-<reason>.db`, reason
   `import`, `migration`, `restore` or `manual`. Only names that parse exactly are
   backups; nothing else in the directory is listed, rotated or deleted. The schema
   version is read from the file, not the name.
3. **Rotation.** Automatic backups (import, migration, restore) keep the 10 most recent
   plus the newest of each earlier calendar month (UTC). Manual backups (`coffer
   backup`, "Back up now") are never deleted automatically. A failed rotation is logged
   and doesn't fail the backup.
4. **When.** `import::commit` takes one before its DB transaction (`VACUUM INTO` can't run
   inside one), only when it will write: not for nothing new or a missing confirmation,
   and never for a preview or `--dry-run`. `Database::open` takes one before applying
   pending migrations to an existing database (not to a new one). A restore takes one of
   the current database first. If the backup fails, the import or migration doesn't
   happen.
5. **Restore checks a copy first.** The backup is copied next to the database and the
   copy is checked: `PRAGMA integrity_check`, a migration history matching this build's,
   and a schema no newer than it. The user's backup file is only read. An older schema is
   restored and then migrated by `Database::open` (which backs the restored file up as a
   `migration` backup first). `coffer restore` without `--yes` shows the check and
   changes nothing.
6. **Restore never skips its safety backup.** If the current database can't be backed up
   (it's damaged), the restore is refused, and the user moves the file aside first. Its
   `-wal`/`-shm` are checkpointed and removed before the rename, never after. A database
   still open elsewhere (its `-wal` survives our close) is refused. A refused restore
   leaves the current database untouched. No other Coffer process may have the database
   open during a restore; MCP servers refusing calls during the swap come with M2.

### Decision record: Settings, AI access and backups (built) (2026-11-14)

M3.15 and M3.16. Marked **(user)**: decided by the user on 2026-11-14 when asked before
the build. Everything else follows `DESIGN.md` → "Settings" or the records above
("Settings, AI access and backups (2026-11-10)", "Local backups", "M2: MCP server").

1. **No migration (user).** Clients have no display name: the `--client` name is the
   name in the app (the Enable dialog's Name field is left out). Nothing records whether
   a client is connected now, so the app shows the last call's time and no "Connected
   now" state.
2. **Off keeps the level; Remove forgets it (user).** The per-client switch is
   `mcp_client::turn_off` / `turn_on`: `enabled` changes, the level stays. `revoke`
   (the CLI's, which resets the level to `aggregate`, decision record "M2: MCP server",
   B.4) is unchanged. "Remove client…" is `mcp_client::remove`: the row is deleted; the
   log, suggestions and reports name the client as text (no foreign key), so they stay,
   and a later call from that name is recorded as `requested` again. Enabling from a
   request is `mcp_client::enable_at`, which takes the level the dialog chose (starting
   at `aggregate`) in the same transaction. A request (or an ignored client) can't be
   turned on, only enabled with a level (`Error::NeverEnabled`).
3. **The access log shows no more than `coffer ai-access log` (user).** Never the
   result hash. The purpose and argument keys are cleaned of invisible characters as the
   CLI cleans them; a row cuts the purpose at the CLI's 60 characters, the inspector shows
   it whole. Reading is `access_log::activity` (pages by client and time, counts, what
   each client did in a period, the redacted arguments as lines); which tools write is
   the MCP registry's (`ToolSpec::writes`), passed in by the app, since core doesn't
   know the tools. What a client left in the Inbox counts `suggestion` and `report` rows
   by `source_client`.
4. **Time comes from SQLite.** chrono is built without its clock; `coffer_core::clock`
   reads the time now and converts UTC times to the computer's zone through an
   in-memory SQLite connection (`'localtime'`, the offset of each date), as
   `schedule::local_today` already did.
5. **Backups are read without being touched (user).** `backup::detail::contents` opens
   a backup `SQLITE_OPEN_READ_ONLY` with `immutable=1`, so no lock, journal, `-wal` or
   `-shm` is written beside it (a test checks the directory is unchanged). A reason names
   the import's file when the current database has an import batch between that backup
   and the next newer one (each import commit takes its backup just before it); a
   migration's "schema 6 → 7" is the backup's version and the next newer one's (or the
   current database's). "Since this backup" counts committed imports, applied change
   batches and suggestions created after it. "Recent" and "One a month" follow
   rotation: an automatic backup outside the 10 most recent automatic ones is a month's.
6. **Restore in the app is the CLI's, with its checks shown one by one.**
   `backup::detail::assess` runs `backup::check` and sorts its answer into "the file is
   intact" (integrity) and "this version can read it" (history, schema); the dialog
   can't restore unless both pass and the current database is readable. Restoring is
   `backup::restore` on a background thread (checked again, the current database backed
   up first, MCP servers refused under the restore lock, the log carried forward). The
   app holds no connection (decision record "Accounts management", 6), so nothing of
   its own blocks the swap; afterwards Settings and the accounts read the database again.
   "Save a copy…" writes a `0600` copy where the user chooses, never over the database.
7. **Export (user).** `export::Options::include_excluded` (default on, so `coffer
   export` is unchanged) lets the dialog leave excluded rows out, as its toggle starts.
   The dialog's "Notes and tags · including private ones" sets both `include_notes` and
   `include_private_tags`. `export::count` gives the caption's transactions and rows, and
   `export::date_span` its starting dates. `export::to_file` now refuses the database
   and its `-wal`/`-shm`/`-journal` (`Error::IsDatabase`), for the CLI too. The columns
   are documented in `docs/EXPORT.md`, a copy of the module's docs. The file goes where
   the system's save dialog says; the app writes no export anywhere else.
8. **The status bar follows the kill switch (user).** "AI access off" with the
   `refused` icon while it's off, else the fixture's "1 agent connected"; either opens
   Settings → AI clients. The rest of the status bar is still fixture data (it reads the
   database since 2026-11-17: "the status bar reads the database").
