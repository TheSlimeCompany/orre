# Orre — Architecture: Settings, AI access and backups

The Settings view, AI clients and their access, local backups and restore.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Settings, AI access and backups

The rules in force, drawn from the records below. Where a later record changed an
earlier one, only the current rule is stated, with the record that set it.

### The Settings view

`orre_app::settings` (M3.15, M3.16; `DESIGN.md` → "Settings"). The title bar's
Settings button or ⌘, opens it. Its panel has AI ACCESS (Clients, Access log) and DATA
(Backups, Export…), with the data file's path at its foot. `SettingsView` makes every
change through `settings::store::SettingsStore`, which opens the database per call
through `backup::gate::open` (see `app.md` → "The app"); the rules are core's, and the
CLI's `ai-access`, `backup`, `restore` and `export` are the reference behaviour.
Reads run on the UI thread; backing up, checking a backup and restoring run on a
background thread. The Enable, Remove client, Delete backup, Restore and Export dialogs
are opened by the shell from `SettingsEvent`s. Times are read and made local through
SQLite (`orre_core::clock`; "Settings, AI access and backups (built)", point 4).
Export… writes the file the system's save dialog names, through `export::to_file`,
which refuses the database and its sidecar files (`Error::IsDatabase`); the columns are
in `docs/EXPORT.md` (built, point 7).

### AI clients and access levels

- **The kill switch** is the `setting` `mcp.enabled`, on unless set off, read by the
  server on every call (`orre_mcp::access`); off refuses the next call
  (`refused_off`). The status bar shows "AI access off" while it is
  ("Settings, AI access and backups (2026-11-10)", point 1).
- **Clients** are `mcp_client` rows keyed by the `--client` name, which is also the
  name shown: there is no display name and no "connected now" state, only the last
  call's time ("Settings, AI access and backups (built)", point 1). An unknown name is
  recorded as `requested` and refused, even with the kill switch off ("M2: MCP server",
  B.5). Only the app or CLI enables a client: `mcp_client::enable_at` takes the level
  the Enable dialog chose, starting at `aggregate`; `ignore` hides a request and
  revokes.
- **Off, revoke and remove.** The per-client switch is `mcp_client::turn_off` /
  `turn_on`, which keep the level; a request or an ignored client can't be turned on,
  only enabled (`Error::NeverEnabled`). The CLI's `revoke` resets the level to
  `aggregate`. "Remove client…" is `mcp_client::remove`: the row goes, its log,
  suggestions and reports stay (they name the client as text), and a later call is
  `requested` again (built, point 2).
- **Levels** are `aggregate`, `pseudonymous` and `standard`; the tools at each and what
  each withholds are `mcp.md` → "MCP surface". Below `standard` accounts are labels
  (`current 1`, `pot 2`) and the Clients screen says `pseudonymous` isn't anonymous
  (`mcp.md`, "merchant names are untrusted, and what `pseudonymous` protects", R.7).
  A lower level applies to the next call, with `notifications/tools/list_changed`.
- **Widening access needs the user's terminal in the CLI**: `ai-access enable`, `on`,
  and raising a level; revoking, ignoring, `off` and lowering don't (`mcp.md`,
  "approval needs the user's terminal", points 3 and 4).
- **The access log** (`mcp_access_log`, one row per call) is kept for 12 months
  (`access_log::RETENTION_MONTHS`, deleted by `access_log::expire` when a server
  starts). Arguments are redacted to their structure (point 4 of the 2026-11-10
  record; `mcp.md` → "MCP surface"). The Access log screen shows no more than `orre
  ai-access log`, never the result hash, reading through `access_log::activity`; which
  tools write comes from the MCP registry (`ToolSpec::writes`) (built, point 3).

### Local backups

`orre_core::backup` ("local backups", points 1, 2 and 4).

- **What.** `VACUUM INTO` snapshots in `<data dir>/backups` (`backup::backups_dir`,
  `0700`; files `0600`), written to a hidden temporary file, synced, then given their
  name, never overwriting one.
- **The name is the metadata.** `orre-<UTC yyyymmddThhmmss.mmmZ>-<reason>.db`, reason
  `import`, `migration`, `restore` or `manual` (`backup::Reason`). Only names that
  `backup::parse_file_name` accepts are backups; nothing else there is listed, rotated
  or deleted. The schema version is read from the file.
- **When.** `import::commit` takes one before its transaction when it will write;
  `Database::open` takes one before pending migrations on an existing database;
  `backup::restore` takes one of the current database first. If it fails, the import,
  migration or restore doesn't happen. A manual one is `orre backup` or "Back up now".
- **Reading one** (`backup::detail`) opens it read-only and `immutable=1`, so nothing
  is written beside it ("Settings, AI access and backups (built)", point 5).

### Retention

Set by "backup retention and presets" (2026-11-19), points 1, 2, 3, 4, 5 and 8, which
supersedes "local backups", point 3.

- **Automatic backups** keep the 10 most recent (`KEEP_RECENT`) plus the newest of each
  of the 12 calendar months (UTC) before the current one (`KEEP_MONTHS`).
  `backup::rotate` (selection in `backup::to_delete`) runs after each automatic backup;
  a failed rotation is logged and doesn't fail the backup. Nothing is deleted when the
  app starts. Settings groups them as Recent and One a month (`backup::detail::monthly`).
- **Manual backups** are never selected or counted by rotation: they stay until the
  user deletes them. Backups are unencrypted, so they hold deleted reports and undone
  imports until deleted.
- **Deleting one.** `backup::delete` deletes only a regular file in the backups
  directory whose name parses; anything else is `Error::NotABackup`. In Settings,
  "Delete backup…" in the inspector, for any backup; in the CLI, `orre backup
  --delete <FILE>`, which only says what it would delete without `--yes` and needs an
  interactive terminal with it.
- **Size.** `backup::total_size`; shown under the Backups table and after `orre
  backup --list`.

### Restore

`backup::restore` ("local backups", points 5 and 6; "M2: MCP server", F.1 and F.9).

- **A copy is checked first:** `PRAGMA integrity_check`, a migration history matching
  this build's, a schema no newer. The user's file is only read. An older schema is
  restored, then migrated by `Database::open`; a newer one is refused. `backup::check`
  runs the checks alone (`orre restore` without `--yes`), and the app's dialog shows
  them as "intact" and "this version can read it" (`backup::detail::assess`), restoring
  only when both pass and the current database is readable (built, point 6).
- **Never without a safety backup.** The current database is backed up (`restore`)
  first; if it can't be, or another connection holds it, nothing is restored. Its
  `-wal`/`-shm` are checkpointed and removed before the rename.
- **The restore lock.** From the checked copy until the restored database is open,
  `restore` holds the lock on `<database>-restoring` (`backup::restore_lock_path`).
  The MCP server and the app open the database through `backup::gate::open` and are
  refused meanwhile; a second restore is refused.
- **The access log isn't rolled back:** rows newer than the backup's newest are
  carried over from the pre-restore backup, then the 12-month retention applied
  (`access_log::carry_forward`), then refused calls moved in from the pending file.
- `orre restore --yes` needs an interactive terminal; so does `orre restore`
  without it, since its check stages a copy in the data dir (`mcp.md`, "approval needs
  the user's terminal", point 4). "Save a copy…" writes a `0600` copy where the user
  chooses, never over the database.

### Presets and backups

User presets (`<data dir>/presets`, `paths::presets_dir`) are files outside the
database: no backup holds them and a restore doesn't touch them. The Restore dialog
(`settings::restore::PRESETS_NOT_IN_BACKUPS`) and `orre restore`'s check output say
so. An account whose preset this computer doesn't have shows "`<id>` · not on this
computer" in the accounts inspector, and the app's import treats it as an account with
no preset ("backup retention and presets", points 6 and 7).

## Decision records

Oldest first.

### Decision record: Settings, AI access and backups (2026-11-10)

The open questions from the D6.8 designs (`DESIGN.md` → "Settings"), decided by the user
on the designer's recommendation:

1. **The kill switch is read on every call.** It's a `setting` that the server reads on
   every call, not only at start, so turning it off refuses the next call from a client
   that is already connected. Refused calls are logged: `mcp_access_log` gains `outcome
   [ok|refused_off|refused_not_enabled|refused_level]` (a new migration).
2. **Unknown clients are recorded as requested.** The first `orre mcp --client X` with
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

Decided while building M1.28 (`orre_core::backup`, `orre backup`, `orre restore`).
Retention and location were decided by the user.

1. **Where and how.** Backups are `VACUUM INTO` snapshots in `backups/` next to the
   database (`<data dir>/backups`, `0700`; files `0600`), derived from the database
   path, so `ORRE_DATA_DIR` moves them too. Each is written to a hidden temporary name,
   fsynced, then hard-linked to its final name (a rename after checking the name is free
   where hard links aren't supported), so a partial file never looks like a backup and no
   backup is overwritten.
2. **The name is the metadata.** `orre-<UTC yyyymmddThhmmss.mmmZ>-<reason>.db`, reason
   `import`, `migration`, `restore` or `manual`. Only names that parse exactly are
   backups; nothing else in the directory is listed, rotated or deleted. The schema
   version is read from the file, not the name.
3. **Rotation.** Automatic backups (import, migration, restore) keep the 10 most recent
   plus the newest of each earlier calendar month (UTC). Manual backups (`orre
   backup`, "Back up now") are never deleted automatically. A failed rotation is logged
   and doesn't fail the backup. *Changed 2026-11-19 (R.12):* the monthly ones are kept
   for the 12 calendar months before the current one, not for ever, and a backup can be
   deleted by the user ("Decision record: backup retention and presets").
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
   `migration` backup first). `orre restore` without `--yes` shows the check and
   changes nothing.
6. **Restore never skips its safety backup.** If the current database can't be backed up
   (it's damaged), the restore is refused, and the user moves the file aside first. Its
   `-wal`/`-shm` are checkpointed and removed before the rename, never after. A database
   still open elsewhere (its `-wal` survives our close) is refused. A refused restore
   leaves the current database untouched. No other Orre process may have the database
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
3. **The access log shows no more than `orre ai-access log` (user).** Never the
   result hash. The purpose and argument keys are cleaned of invisible characters as the
   CLI cleans them; a row cuts the purpose at the CLI's 60 characters, the inspector shows
   it whole. Reading is `access_log::activity` (pages by client and time, counts, what
   each client did in a period, the redacted arguments as lines); which tools write is
   the MCP registry's (`ToolSpec::writes`), passed in by the app, since core doesn't
   know the tools. What a client left in the Inbox counts `suggestion` and `report` rows
   by `source_client`.
4. **Time comes from SQLite.** chrono is built without its clock; `orre_core::clock`
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
   *Changed 2026-11-19 (R.12):* the Backups screen also shows the total size and has
   Delete backup… ("Decision record: backup retention and presets").
6. **Restore in the app is the CLI's, with its checks shown one by one.**
   `backup::detail::assess` runs `backup::check` and sorts its answer into "the file is
   intact" (integrity) and "this version can read it" (history, schema); the dialog
   can't restore unless both pass and the current database is readable. Restoring is
   `backup::restore` on a background thread (checked again, the current database backed
   up first, MCP servers refused under the restore lock, the log carried forward). The
   app holds no connection (decision record "Accounts management", 6), so nothing of
   its own blocks the swap; afterwards Settings and the accounts read the database again.
   "Save a copy…" writes a `0600` copy where the user chooses, never over the database.
7. **Export (user).** `export::Options::include_excluded` (default on, so `orre
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

### Decision record: backup retention and presets (2026-11-19)

R.12 (the architecture review: backups kept deleted data with no end, and user presets
aren't in them). Marked **(user)**: decided by the user on 2026-11-19; the rest is
**(composed)** from the existing specs and records. Supersedes "Decision record: local
backups" point 3 (rotation) and adds to point 5 of "Settings, AI access and backups
(built)" (the Backups screen).

1. **Monthly backups are kept for 12 months (user).** Automatic backups (import,
   migration, restore) keep the 10 most recent (`KEEP_RECENT`) plus the newest of each of
   the 12 calendar months (UTC) before the current one (`KEEP_MONTHS`: at 19 Nov 2026,
   November 2025 to October 2026). Older monthly backups are deleted at the next
   rotation, which runs after each automatic backup; the current month's backups count
   only among the 10. Months are counted from the file name's time, so a year with gaps
   keeps what falls in the window and nothing else.
2. **Manual backups are kept until the user deletes them (user).** Rotation never
   selects one and never counts one. So a deleted report, an undone import or a
   removed account can stay on disk in a manual backup, and in an automatic one for up to
   12 months plus; `docs/architecture/mcp.md` F.5 says so. Backups are unencrypted
   (unchanged), so deleting is the way to get rid of what they hold.
3. **Settings shows the total size, and a backup can be deleted (user).** The Backups
   screen shows "13 backups · 4.2 MB in total" under the table (**composed**: a mono
   line in `text-secondary` above the existing caption), and the toolbar caption now
   reads "Taken before every import, restore and update · the last 10 and one a month for
   a year are kept" (**composed**), with "Backups you take yourself stay until you delete
   them." first in the caption under the table. The inspector ends
   with "Delete backup…" (`DestructiveCompact`, as "Remove client…"; **composed**) for any
   backup, manual or automatic, which opens a destructive confirmation (**composed**
   from the dialog spec, as the Remove client dialog): "Delete the backup from 10 Nov,
   14:02?", "The file is deleted for good, not moved to the trash, and can't be undone."
   (a manual one adds that Orre would have kept it), Cancel / "Delete backup"
   (`DestructiveSolid`). A notification says "Deleted the backup · 4.2 MB". The shell opens it
   (`SettingsEvent::DeleteRequested`, `settings/delete.rs`), as it opens Settings' other
   dialogs.
4. **`backup::delete` only deletes a backup (composed).** `backup::delete(db_path,
   name_or_path)` accepts a file name, or a path whose folder is the backups directory;
   the name must parse as a backup (`parse_file_name`) and the entry be a regular file
   (a directory or a symlink with a backup name is refused). Anything else, including a
   backup-named file in another folder or `..`, is `Error::NotABackup` and nothing is
   touched. `backup::total_size` adds the sizes.
5. **`orre backup --delete <FILE>` (composed).** It lists nothing new: `orre backup
   --list` shows the total after the table, and `--delete` names a backup by file name or
   path. Without `--yes` it says which backup and deletes nothing; with `--yes` it deletes
   it. With `--yes` it needs an interactive terminal, like the other commands that act as
   the user (`cli/terminal.rs`; `mcp.md`, "Decision record: approval needs the user's
   terminal"), so a client with a shell can't remove the user's backups; the preview
   doesn't.
6. **User presets aren't in backups, and the app says so (user).** User presets
   (`<data dir>/presets`, `paths::presets_dir`) are files outside the database, so a
   backup holds neither them nor the preset a restore needs, and a restore doesn't touch
   them; moving to another computer with a backup leaves `account.preset_id` naming
   nothing there. Said in three places: this record, the Restore dialog (a caption: "Your
   own bank presets aren't in backups, so restoring doesn't change them. An account whose
   preset isn't on this computer asks for its columns again at the next import.") and
   `orre restore`'s check output (the same, with the folder). Not added to backups: the
   backup is one SQLite file, and copying the presets folder is a file copy the user can
   do.
7. **An account's missing preset shows as missing (user).** The accounts inspector's
   Preset row shows a preset this computer has neither bundled nor saved as "`<id>` ·
   not on this computer" (**composed**), not the bare ID. The import already copes: the
   app's import (`import::session`) uses an account's preset only when `Presets::get`
   finds it, else detects one from the file's header or suggests a mapping from it, so
   an account with a missing preset imports as one with none. `orre import` doesn't
   read `account.preset_id` at all; it takes `--preset` or detects. Nothing in the
   import was changed.
8. **Existing old monthly backups** go at the next automatic backup's rotation, not at
   once; nothing deletes a backup when the app starts.
