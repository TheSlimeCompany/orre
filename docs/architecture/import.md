# Coffer — Architecture: Import

How a file becomes rows: the importers, presets, dedupe, reconciliation, the commit and its undo, and the import dialog.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Import pipeline

1. **Detect** format (extension + sniffing) and encoding (a BOM, else the stated
   encoding: UTF-8, UTF-16, Windows-1252/Latin-1, Latin-9; else UTF-8 only).
2. **Map** (CSV): match a preset or use the mapping screen — date column + format,
   optional separate time column, amount as signed column or debit/credit pair
   (prefer a signed column when both exist — e.g. Monzo puts some refunds as
   positive values in "Money Out"), merchant/display name, raw description,
   optional source ID, source category, transaction type, original (local)
   amount + currency, optional balance column, decimal separator
   (`1,234.56` vs `1.234,56`), date order (UK/EU `DD/MM` vs US `MM/DD`).
   A mapping can be saved/exported as a preset (M3.5: "Decision record: the import
   flow", points 6 and 7).
3. **Normalize** to `{account, date, time?, amount_minor, currency, merchant?,
   description_raw, description_normalized (whitespace-collapsed, uppercased),
   external_id?, source_category?, source_type?, original_amount_minor?,
   original_currency?, balance_after?}`.
4. **Dedupe**: by source ID when the file has one (OFX `FITID`, CSV ID column such as
   Monzo's `Transaction ID`); otherwise by fingerprint
   `(account, date, amount, normalized description, occurrence index)` so identical
   same-day transactions both survive.
5. **Reconcile**: when opening/closing or running balances exist, verify
   `opening + Σ = closing`; failures are flagged, never silently committed. Many CSV
   exports (e.g. Monzo) have no balances; then nothing is reconciled and net worth
   anchors on a user-entered balance for that account.
6. **Preview → commit**: show new / already-imported / flagged counts, and flag rows dated
   on or before the account's `reconciled_through` lock (and leave out the file's
   balances dated then); take a backup (M1.28); commit in one DB transaction; record an
   `import_batch` so an import can be undone.
7. **Post-process** (deterministic, no model, inside the commit's DB transaction): map
   source categories via the preset; mirror sub-account transfers onto their pots
   (M1.17); clean merchant text and apply merchant aliases into `merchant_display`
   (M1.23); apply rules (which can match the cleaned merchant, raw description, source
   category, source type, amount, account) and their actions, a rule's category
   overriding the preset's and its merchant overriding the alias; run transfer-pair
   detection; match schedule occurrences; queue near-duplicate candidates; record
   balances and check assertions. All of these run today (schedule matching since
   M2.10)
   ("Decision record: sub-accounts"; "Decision record: rules engine, transfer detection
   and the CLI"; "Decision record: splits, tags and notes, manual rows, merchant
   names").

### The `Importer` trait

M1.6, decided 2026-11-10. Steps 1–3 above are one importer each (CSV, OFX, later QIF,
CAMT.053, MT940); steps 4–7 are core's and don't depend on the format.

- **The normalized form lives in core.** `coffer_core::statement::Statement` is what every
  importer produces and what dedupe, reconciliation and the commit consume, so it sits in
  `core` (which can't depend on `import`). It has the format, the preset ID if any, one
  currency, the file's rows in order and any statement-level balances (`LEDGERBAL`). A
  statement isn't tied to an account: the user picks it at import, and the commit checks
  the currencies agree.
- **Rows are never dropped.** Each row is a `StatementLine` or a `RowProblem`, at its
  `SourceLocation` (line in a text file, entry in a structured one), kept in file order
  for the fingerprint's occurrence index and running balances. A line whose amount or
  running balance isn't in the statement's currency is flagged as it's added; a
  statement-level balance in another currency fails the file. Only a file that can't be
  read as a whole is an `ImportError`. Neither kind of error contains the file's text.
- **`description_normalized` is computed, not carried**
  (`statement::normalize_description`: whitespace collapsed, trimmed, Unicode uppercase),
  so it can't disagree with the raw text. Changing it changes the fingerprint.
- **Three traits in `coffer-import`.** `Detect` (`name`, `format`, `detect`) says what an
  importer is and how well a file fits: `No < Extension < Format < Source` (`Source` is a
  preset's own header row, `Format` is merely valid CSV or OFX). `Importer: Detect` adds
  `parse` (into the format's own `Parsed` type) and `normalize` (into a `Statement`).
  `AnyImporter: Detect`, implemented for every `Importer`, hides `Parsed` so importers
  can be listed as `&dyn AnyImporter` and adds `import` (parse, then normalize). Each
  method has one home, so calls never need disambiguating.
- **Choosing is the caller's.** `coffer_import::detect` ranks the importers that
  recognise a file, best first, ties in the given order. No candidate or a tie at the top
  means asking the user (or `--preset`), never guessing.
- `SourceFile` holds the undecoded bytes and the file name only, never the directory; its
  `Debug` leaves the contents out. Decoding is the importer's, from the format or preset.

### The CSV importer

M1.7, decided 2026-11-10. `coffer_import::csv`: `CsvImporter` reads files through a
`CsvMapping`, the Rust form a preset (M1.8) or the mapping screen (M3) produces.
`CsvImporter::new` refuses a mapping that can't work for any file (no description column,
the same debit and credit column, a named column without a header row).

- **Encoding is never guessed.** A byte order mark wins (UTF-8, UTF-16LE/BE) and is
  stripped. Otherwise the mapping's encoding is used; `Auto` means UTF-8 only.
  Single-byte encodings accept any bytes, so picking Windows-1252 for a file that isn't
  would silently change accented letters, and with them the dedupe fingerprint. Decoding
  is strict (no replacement characters), and NULs in the text (UTF-16 without a BOM)
  fail the file. A preset or the user states non-UTF-8 encodings.
- **Delimiter**: stated, or detected from the first 50 records among `,` `;` tab `|`.
  Records whose split can't be trusted (below: an unclosed quote, or several lines when
  the mapping doesn't allow it) are left out, since how a stray quote merges lines
  depends on the delimiter. The first remaining record sets the width (at least 2). The
  candidate under which most other records have the same shape (the same count, or
  extra trailing empty fields on either side) wins, then the wider one; a tie is
  refused, so the mapping has to say.
- **Layout**: `skip_rows` raw lines before the header (account summaries), then an
  optional header. Named columns match header cells exactly after trimming; a missing or
  duplicated name fails the file (`ImportError::MissingColumn` / `DuplicateColumn`; the
  name comes from the mapping, so it can be shown). Files without a header use 0-based
  positions. Line numbers are physical lines, counted from byte offsets with `\r\n`,
  `\n` and a lone `\r` (old Mac exports) each ending one line, both there and in
  `skip_rows` (the `csv` crate's own count only knows `\n`), so flagged rows can be found
  in the file.
- **A skip can't end inside a quoted field** (decided 2026-11-11). `skip_rows` stays
  physical lines, not records: a preamble is often not CSV (free text, stray quotes, other
  column counts), and a record count would let one stray quote swallow the header. But a
  summary cell with a line break in its quotes is two lines, and a count that ends
  between them would read the rest of the cell as the header or first row (a confusing
  missing-column error, or rows merged and flagged at a summary line). So the skipped
  lines are read as CSV and, if they end inside a quoted field, the file fails
  (`ImportError::Malformed`, at the line the field opens on). Only a `"` at the start of a
  field opens one (the `csv` crate's rule), so `Account: 6" pizza fund` is text. Field
  starts depend on the delimiter, and a closing quote at the start of a line opens a field
  under any other, so the check uses the delimiter the file is read with: stated or
  sniffed from the text after the skip. Only when sniffing fails does an open field under
  any candidate count, since the broken skip is then the likelier cause and says more.
- **A row with the wrong column count is flagged** (`RowProblem::ColumnCount`), not
  read: an unquoted delimiter in a description shifts every later field, and reading it
  would import a wrong amount. The file's width is the header's cells up to its last
  non-empty one, or without a header the first trustworthy row's count. Extra trailing
  empty fields on a row are fine, so a delimiter at the end of the rows, of the header,
  or of both reads; a value under an unnamed trailing header cell is flagged as an extra
  column rather than ignored.
- **One record per line unless the mapping says otherwise** (decided 2026-11-11). An
  unclosed quote makes the `csv` crate read the rest of the file as one field, and two
  stray quotes merge the lines between them, sometimes into a record with the right
  column count, so transactions would vanish into a description. So each record knows
  how many physical lines it takes and whether a quoted field is still open at the end
  of the file (`CsvRecord::lines`, `unclosed_quote`, following the `csv` crate's rules:
  a field is quoted only when it starts with `"`, so `6" pizza` stays literal).
  `CsvMapping::multiline_fields` (preset `multiline_fields`, default `false`) says
  whether the export really has line breaks inside quoted fields. Off, a data row over
  several lines is flagged `RowProblem::MultiLineRow { lines }` at the line it starts on;
  on, it's read. Either way a quote still open at the end of the file is flagged
  `RowProblem::UnclosedQuote { lines }`, never read as text. A header row like that
  fails the file (its columns can't be trusted).
- **Dates round-trip.** Formats are `strftime` patterns, checked when built
  (`DateFormat::new`: needs a year, month and day; may include a time; no time zones).
  chrono parses leniently (`%Y` reads `26` as year 26), so each value must format back
  to the same text, ignoring ASCII case, or the row is flagged. `DD/MM` against `MM/DD`
  is the mapping's: `DateFormat::candidates` lists every common format a column's values
  fit, and more than one means asking the user. A separate time column overrides the
  date's time when filled in.
- **Amounts** are read with `Money::parse_decimal` and the mapping's decimal mark, from
  a signed column or a debit/credit pair. A pair is `Unsigned` (positive amounts, money
  out negated; a negative value is flagged as unclear) or `Signed` (each value as
  written). A row filling both columns is flagged unless one of them is zero. An
  optional per-row currency column sets the currency amounts are read in; a row not in
  the statement's currency is then flagged by `Statement::push`.
- **Text**: a single description column is kept as written; several are trimmed and
  joined with a space, empty ones left out. An empty description is allowed. Optional
  text fields are `None` when blank, otherwise kept as written; the external ID is
  trimmed. The original amount is read in its own currency and dropped when it just
  repeats the amount (Monzo fills it on every row).
- **Detection**: `Source` when the header has every column the mapping names,
  `Format` for other readable CSV with at least two columns, `Extension` for an
  unreadable `.csv`/`.tsv`/`.txt`. Two presets that both find their columns tie, and the
  caller asks. A stray quote in a data row doesn't change what the header says, so such
  a file still detects (and its import flags the row); a header-less file needs at least
  one trustworthy row for `Format`.
- `CsvFile` (the `Parsed` type: encoding used, delimiter, header, records with line
  numbers and line counts) is public for the mapping screen. Its `Debug` and
  `CsvRecord`'s show counts
  only.
- **The mapping screen (M3.4).** `CsvFile::read(file, &FileLayout)` splits a file before
  any column is chosen (`FileLayout`: encoding, delimiter, skip, header, multi-line;
  `CsvMapping::layout`). `MappingDraft` is a `CsvMapping` whose columns may still be
  unchosen: `from_mapping` (a preset), `suggest` (header names, then values; the
  decimal mark from the values; signs of a money-out column), `build(currency)` (the
  first missing choice is the error, in screen order). `date_formats` lists the formats
  that read a column, equivalent ones once; `DateFormat::common` and
  `TimeFormat::candidates` fill the selects. `CsvImporter::problem_text` gives the
  file's text where a flagged row's problem is ("8,99"), for the preview to show and
  never to log.

### Presets

M1.8, decided 2026-11-10. `coffer_import::preset`: a preset is a `CsvMapping` in TOML plus
an ID, a name, a region, ignored columns and a category map. The field reference for
contributors is `presets/README.md`.

```toml
# presets/uk/monzo.toml (M1.9; comments and most of the category map left out)
id = "uk-monzo"
name = "Monzo"
region = "uk"
currency = "GBP"
encoding = "utf-8"                                 # default "auto"
delimiter = ","                                    # default: detected
skip_rows = 0
header = true                                      # false: columns are 1-based positions
multiline_fields = true                            # notes can span lines; default false
external_id = { column = "Transaction ID" }
date = { column = "Date", format = "%d/%m/%Y" }
time = { column = "Time", format = "%H:%M:%S" }    # optional
amount = { column = "Amount" }                     # or { debit = "Paid out", credit = "Paid in", signs = "unsigned" }
decimal = "."
row_currency = { column = "Currency" }             # per-row currency code
original_amount = { column = "Local amount", currency_column = "Local currency" }
merchant = { column = "Name" }
description = { column = "Description" }           # or { columns = ["A", "B"] }
source_category = { column = "Category" }
source_type = { column = "Type" }
note = { column = "Notes and #tags" }
# balance = { column = "Balance" }                 # Monzo exports have none
ignore = ["Emoji", "Address", "Receipt", "Category split", "Money Out", "Money In"]

[category_map]          # source category -> our category key
"Groceries" = "food.groceries"
"Eating out" = "food.eating_out"
"Savings" = "transfer.savings"
```

- **Strict.** Unknown keys are refused (`deny_unknown_fields`), so a misspelt optional
  field fails instead of being silently unmapped. Values that stand alone (columns, date
  and time formats, encoding, delimiter, currency, category keys) are checked while
  deserializing, so the error points at the line; the whole mapping is then checked by
  `CsvImporter::new`. `amount` and `description` take exactly one of their forms.
- **`multiline_fields`** (default `false`, decided 2026-11-11) maps to
  `CsvMapping::multiline_fields`: set it only for an export that really writes line
  breaks inside quoted fields (multi-line notes), since otherwise a row over several
  lines is the sign of a stray quote and is flagged (see "The CSV importer").
- **Positions count from 1** in TOML (`column = 3`), as the error messages do;
  `CsvMapping` stays 0-based.
- **Bundled.** `crates/import/build.rs` compiles every `presets/**/*.toml` into the binary
  (`Presets::bundled`), so an installed Coffer needs no source tree and a preset is
  never half-installed. Placement is checked at load, not in the build script, so a
  misplaced file fails the tests with a message rather than being skipped.
- **IDs are stable and findable.** The ID is recorded on `import_batch.preset_id`, so it
  never changes once shipped. It is `<region>-<file stem>` optionally followed by
  `-more` (`uk/monzo.toml` → `uk-monzo` or `uk-monzo-current`), lowercase letters,
  digits and single hyphens, and unique; the region must match the directory.
  `Presets::from_files` checks all of this for any `(path, text)` set. User presets
  saved from the mapping screen (M3.5) live in `<data dir>/presets/` and load through
  the same checks, one file at a time (`Presets::with_user_presets`): a broken one is
  reported and left out, and none can take a bundled preset's ID ("Decision record: the
  import flow", point 6).
- **Written back out.** `Preset::to_toml` writes a preset in this format, keys in the
  README's order and defaults left out; `Preset::from_toml` reads it back to the same
  preset (tested on every bundled preset). `Preset::new(PresetParts)` builds one by
  writing and reading it.
- **Columns not read are visible.** `ignore` lists header columns the preset reads on
  purpose not at all; `Preset::unread_columns` returns the file's other unread header
  columns, so the preview can say what a bank added instead of dropping it silently.
- **Category map.** `CategoryKey` (`coffer_core::category`) is one or two segments of
  `[a-z0-9_]` joined by `.`, the shape of `category.key`. Source categories match
  exactly, case included, after trimming the row's value; map keys must already be
  trimmed. Whether a key exists is checked against the database's categories when the
  import maps them; a key it doesn't have, like an unmapped category, is left for rules
  or the user. Bundled presets map only to default keys (a test checks it; see
  "Categories").
  A `category_map` without a `source_category` column is refused.
- **Every bundled preset has a fixture** at `fixtures/presets/<region>/<bank>.*` that it
  detects as `Source` and imports; a test enforces it.
- **Monzo is the reference preset** (M1.9, decided 2026-11-11; `presets/uk/monzo.toml`,
  fixture `fixtures/presets/uk/monzo.csv`, tests in
  `crates/import/tests/preset_uk_monzo.rs`). Settled with it:
  - The ID is `uk-monzo`, with no account-type suffix: personal and joint current
    accounts export the same columns, and two presets with the same header would only
    tie at detection. A Monzo export with other columns gets its own file
    (`monzo-<kind>.toml`).
  - `multiline_fields = true`: "Notes and #tags" is free text from the app and can hold
    line breaks inside its quotes, and Monzo's writer quotes correctly, so a row over
    several lines there is a note, not a stray quote. The column count and the
    end-of-file check still catch broken quoting.
  - The signed `Amount` is read and `Money Out` / `Money In` are ignored (refunds can
    sit in "Money Out" as positive values). `Currency` is read as the row currency, so
    a row not in pounds is flagged rather than taken as GBP.
  - `Name` is the merchant (the pot's name for a pot transfer), `Description` the raw
    text (empty for a pot transfer or a Monzo-to-Monzo payment). `[sub_account_transfer]`
    recognises pot transfers by `Type` = "Pot transfer" (M1.17) and keys them by `Name`
    ("Decision record: sub-accounts", point 8).
  - `Address` is ignored, so merchant addresses never reach the database from Monzo.
  - Monzo's "Transfers" category isn't mapped: Monzo files pot transfers and payments to
    and from people under it, and the latter are spending or income. Pot transfers get
    `transfer.savings` from `[sub_account_transfer] category` instead. "Savings" maps to
    `transfer.savings`; Family and Finances map to the `family` and
    `finance` groups; "General" and "Expenses" are left for rules or the user.
- **`[sub_account_transfer]`** (M1.17, decided 2026-11-11): `kind` (`"pot"` only),
  `when = { column, is }`, `key = { column }`, optional `name = { column }`. A row whose
  `when` column, trimmed, is exactly `is` (case included; `is` must be trimmed and
  non-empty) is a transfer between the account and one of its pots. Its line carries
  `SubAccountRef { kind, key, name }` (trimmed), and the commit mirrors it onto the pot
  ("Decision record: sub-accounts"). A match with a blank key is flagged
  (`RowProblem::SubAccountKeyMissing`) rather than imported without its pot side. Its
  columns count as read (detection, `unread_columns`, `ignore`). Starling Spaces and
  Revolut Vaults become new `kind`s with their own columns. Monzo: `when = Type is "Pot
  transfer"`, `key = Name`, `name = Name`, `category = "transfer.savings"` (points 8 and 9
  of that record). Optional `category`: a default category key every matching row gets,
  ahead of `category_map`.
  - **Checked against a real Monzo export (2026-11-13, M1.9),** by structure only (header,
    counts, value shapes, distinct types and categories): the header matches; the file
    is UTF-8 without a BOM, LF line endings, quoting only where needed; £0.00 card checks
    have `Money Out` "0.00"; pot transfers and Monzo-to-Monzo payments have an empty
    `Description`; pot transfers are in "Transfers"; IDs are all `tx_…` and unique.
    The fixture follows it. Notes over several lines and refunds in `Money Out` didn't
    occur in that export and stay as fixture rows from Monzo's known behaviour.

### The OFX importer

M1.10, decided 2026-11-11. `coffer_import::ofx`: `OfxImporter` reads OFX 1.x (SGML) and
2.x (XML) bank (`STMTRS`) and credit card (`CCSTMTRS`) statements. QFX is OFX with
Intuit's extra tags and reads the same way.

- **Own reader, not `ofx-kit`.** `ofx-kit` (planned in M1.1) fails the whole document on
  one bad `STMTTRN` (an unreadable amount, a missing `FITID`), so it can't flag rows; it
  rewrites `,` to `.` in amounts, falls back to Windows-1252 when UTF-8 fails (a guess),
  and puts file text in its errors. It was removed with `rust_decimal`, its only user.
  The reader is a few hundred lines in `ofx/markup.rs`, handles both syntaxes, and
  keeps every value as text so amounts go through `Money::parse_decimal`.
- **Markup.** A start tag followed by text is a value, closed or not (SGML leaves values
  unclosed; XML and some SGML files close them). A start tag followed by another tag is
  an aggregate if it's a known OFX aggregate, an empty value if it's a known value, and
  otherwise an aggregate only if the file closes that name somewhere. Entities
  (`&amp;`, numeric) are decoded; a bare `&` is kept ("M&S"). Comments, CDATA,
  processing instructions and attributes are handled. Tag names are uppercased.
- **Structure is strict.** An aggregate left open, a closing tag for an aggregate that
  isn't open, text where only elements belong, more than one top-level element, a file
  that ends early (a truncated download), a `STMTTRN` outside a statement or inside
  another, and a statement-level value given twice (`CURDEF`, `LEDGERBAL`) fail the
  file with a line number. Guessing would move transactions between statements or lose
  them.
- **Encoding is never guessed.** A BOM wins. Otherwise OFX 1.x `ENCODING:UTF-8` (or
  `UNICODE`) is UTF-8; `ENCODING:USASCII` takes `CHARSET` (`1252` and Latin-1 are
  Windows-1252, `8859-15` is Latin-9, `NONE` is ASCII read as UTF-8, so other bytes
  fail). OFX 2.x uses the XML declaration's `encoding`, else UTF-8, as does a file
  without a header. Decoding is strict, as for CSV; another charset is refused
  (`ImportError::Unsupported`). A file whose header states the wrong charset reads
  wrongly or fails; that is the bank's statement, not a guess.
- **One statement per import.** `Statement` is one account in one currency, so a file
  with several statements (some banks put every account in one download) isn't split
  silently: `OfxImporter::new()` refuses it with `ImportError::MultipleStatements`, and
  `OfxFile::statements()` lists them (kind, currency, transaction count, last four
  characters of the account number) for the user to pick, then
  `OfxImporter::for_statement(index)` imports that one. Investment statements
  (`INVSTMTRS`) are listed but refused (`Unsupported`); holdings are L.14. Other
  message-set responses (closing statements, sign-on) are not transactions and are
  skipped.
- **Statement fields.** `CURDEF` is the statement's currency; missing or unknown fails
  the file. `LEDGERBAL` (`BALAMT` as of `DTASOF`'s date) is the statement's
  `DatedBalance`; one that can't be read fails the file, like a balance in another
  currency. `AVAILBAL` isn't used: it is the ledger balance less holds, or a card's
  available credit, so it would fail reconciliation. `BANKTRANLIST`'s `DTSTART`/`DTEND`
  and the account number are not stored.
- **Dates are taken as written.** `DTPOSTED` is `YYYYMMDD`, optionally `HHMM`, `HHMMSS`
  or `HHMMSS.XXX`, then optionally `[offset:zone]`. The date and time are the bank's
  local ones and the offset is not applied: converting to UTC would move evening
  transactions to another day than the bank's statement and CSV exports show, and break
  dedupe against them. A time is kept whenever the value has one (many banks write a
  placeholder such as `120000`; hiding it is the app's choice, not the importer's).
  Fractions of a second are dropped. `DTUSER` and `DTAVAIL` are not read.
- **Amounts** are `TRNAMT` through `Money::parse_decimal`, never rounded. OFX allows `.`
  or `,` as the decimal mark and no grouping, so a value with a comma and no point uses
  the comma (`1,234` is then too precise for GBP and flagged, not read as 1234). Signs
  are as the file states them: on a credit card a purchase is negative and a balance
  owed is negative.
- **Currencies.** A `CURRENCY` aggregate means `TRNAMT` is in its `CURSYM`, so the amount
  is read in that currency and `Statement::push` flags it when it isn't the statement's
  (converting with `CURRATE` would round). `ORIGCURRENCY` means `TRNAMT` is already in the
  statement's currency; the original amount isn't derived from the rate, for the same
  reason, so `original_amount` stays empty. Both on one transaction is flagged.
- **Text.** `NAME` (or `PAYEE`'s `NAME`) is the merchant. The description is `NAME`,
  `EXTDNAME` and `MEMO` joined with spaces, except that a piece that starts with the text
  so far replaces it (a 32-character `NAME` cut from the `MEMO`) and a piece the text
  already starts with is left out (the same text twice). No text at all is an empty
  description, as in CSV. `PAYEE`'s address, `CHECKNUM`, `REFNUM`, `SIC`, `PAYEEID` and
  `BANKACCTTO`/`CCACCTTO` are not read. `TRNTYPE` is the source type, as written.
- **`FITID`** is the external ID, trimmed; an empty or missing one is `None`, which
  leaves the row to fingerprint dedupe (M1.11). A `FITID` repeated within the file can't
  identify its rows and `(account_id, external_id)` is unique in the database, so every
  occurrence after the first is flagged (`RowProblem::DuplicateExternalId`, naming the
  first); the first is read as usual. Nothing is deduplicated here.
- **Flagged, not skipped:** a missing or unreadable `DTPOSTED` or `TRNAMT`, a value given
  twice, an unknown `CURSYM`, an amount in another currency, a repeated `FITID`, and a
  correction (`CORRECTACTION`, `RowProblem::Unsupported`): applying a `REPLACE` or
  `DELETE` to an earlier transaction would edit imported data, so the user sees it
  instead.
- **Locations.** Transactions are `SourceLocation::Entry(n)`, the n-th `STMTTRN` in the
  whole file in document order (so entries in a second statement don't restart at 1).
  File-level errors give the line.
- **Detection** is `Format` when the file starts like OFX (an `OFXHEADER` block, an XML
  declaration or `<?OFX …?>` followed by OFX, or a bare `<OFX>`), whatever its name;
  `Extension` for another `.ofx` or `.qfx`. OFX has no per-bank presets, so never
  `Source`. A generic CSV mapping doesn't tie with it on OFX files (tested).
- `OfxFile` (the `Parsed` type: version, encoding used, statements) and `OfxStatement`
  show no account number or transaction contents in `Debug`; the parse tree's `Debug`
  shows value lengths only.

### Dedupe, reconciliation and the commit

M1.11–M1.13, decided 2026-11-11 (decision record "import commit, dedupe and undo").
`coffer_core::import`: `preview(db, account, file name, Statement)` →
`ImportPreview` (every row's `RowOutcome`, the file's balances, a `Reconciliation`,
`counts()`, `needs_confirmation()`), then `commit(db, &preview, Confirmations)` →
`Committed::Batch(summary)` or `NothingNew`; `rehearse(db, &preview)` runs the commit's
steps and rolls them back, for the Review step's figures (M3.4). `undo_preview(db, batch)` and
`undo(db, batch)` remove a batch; `batches`, `batch` and `batch_flags` read the history.

- **Row outcomes**: `New` (with its fingerprint), `AlreadyImported` (the stored row and
  whether its source ID or fingerprint matched), `RepeatedInFile`, `Corrects` (the bank's
  new version of a stored row with that source ID: decision record "a bank's correction
  supersedes the stored row"), or `Flagged`: `Unreadable(RowProblem)`, `SourceIdConflict`
  (a correction that can't replace the stored row, with why), `SourceIdRepeated`, `Locked`. Every row of
  the statement has one, in file order.
- **Source ID first.** A row with an external ID matches only the account's row with that
  ID, and never by fingerprint; source IDs are per account, so the same ID on two
  accounts is two transactions. Excluded rows still count as imported.
- **Fingerprint** (`import::fingerprint`, frozen by a test):
  `v1|<YYYY-MM-DD>|<amount minor>|<currency>|<occurrence>|<normalized description>`.
  The time, merchant and other fields are left out; the account is the unique index's
  other column, not part of the text. The occurrence index counts earlier rows in the
  same file with the same date, amount and normalized description, from 0, separately
  for rows with and without a source ID; flagged rows take none. A stored row without a
  source ID matches when its fingerprint is equal. So identical same-day rows are 0, 1,
  2… and both import; re-importing finds each again; an overlapping export with all of
  a day's rows numbers them the same way. One cut off part-way through a day can number
  that day's identical rows differently. The text is readable rather than hashed: no
  collisions, and nothing the row doesn't already hold. Description normalization is
  `statement::normalize_description`.
- **Running balances** (`Reconciliation::running`): each stated balance must equal the
  previous one plus the amounts since; a flagged row restarts the chain. The file's
  order is the one under which fewer balances fail, ties going by the dates. A mismatch
  is flagged at its row.
- **Balance checks** (`coffer_core::balance::check`, reused by M1.27): the account's
  balances (entered, earlier files, the opening balance, and this file's) are sorted by
  date; the first is the starting point and each later one must equal the last passing
  one plus the transactions after it, up to and including its day. Rows excluded as
  `duplicate` or `superseded` leave the sum. The preview runs it with and without the import and reports
  the checks this file adds, the stored ones whose result it changes, and stored ones
  the file states again. `ReconciliationStatus`: `NoBalances`, `StartingPoint`,
  `Reconciled` or `Mismatch` (the import screen's reconciliation block).
- **Commit** inserts the `import_batch`, its rows (`origin = imported`, `created_at` =
  the batch's `imported_at`), its new balances (`csv` or `ofx`) and its `import_flag`
  rows, in one DB transaction. In the same transaction it stores each new row's preset
  category (`category_source = preset_map`), mirrors sub-account transfers onto their
  pots (`sub_account::mirror`), names every row of the batch
  (`merchant::apply_to_batch`), runs the rules on the batch (`rule::apply_to_batch`),
  which override the preset's category and apply every other action (M1.24), suggests
  transfer pairs (`transfer::detect_for_batch`), then records possible duplicates
  (`duplicate::detect_for_batch`) and suggests refund and reimbursement links
  (`link::suggest_for_batch`); a failure in any step rolls the whole import back.
  `CommitSummary` counts the file's rows categorised by the preset and by rules, the
  mirrored rows and created pots, the pairs suggested, rule splits skipped, possible
  duplicates and link suggestions. Undo clears the batch's split lines, tag links, change
  items, links (either side) and duplicate candidates before deleting its rows;
  `UndoPreview::merged_into` counts rows elsewhere merged into the batch's rows, which
  stay excluded. The M1.28 backup is taken
  just before the transaction.
- **File balances** are `New`, `AlreadyRecorded`, `Repeated`, or `Locked` (dated on or
  before the lock; left out).

## Decision records

Oldest first.

### Decision record: import commit, dedupe and undo (2026-11-11)

Decided while building M1.11–M1.13 (`coffer_core::import`, `coffer_core::balance`,
`coffer_core::account`); the details are under "Import pipeline" → "Dedupe,
reconciliation and the commit".

1. **Preview, then commit what was previewed.** `import::preview` reads the database and
   writes nothing; `import::commit` plans again inside its own `BEGIN IMMEDIATE`
   transaction and refuses (`Stale`) if the plan or the account differs from the
   preview. What the user confirmed is exactly what is stored, even if another process
   imported in between.
2. **Flagged rows are recorded, not stored as transactions.** A row that can't be read
   has no date or amount to store, so "imported as flagged" (`DESIGN.md` → Import) means
   the batch keeps an `import_flag` row (its place in the file and core's reason, never
   its text; a new migration). The same goes for a source ID already taken by a
   different transaction, a row in a locked period, and a running balance that doesn't
   follow from the rows before it (that row itself is imported). Committing with flags
   or a failed reconciliation needs an explicit confirmation from the caller. The Review
   screen's "Import N transactions" counts new rows only (since 2026-11-18, then the
   bank's corrections: "and M corrections"); flagged rows are listed beside it.
3. **A source ID with a different date or amount is a conflict, not a duplicate.** It is
   flagged and not imported. The same ID, date and amount with different text is the
   same transaction (banks re-word descriptions). A repeated ID within one file is
   "repeated in file" when date and amount agree, otherwise flagged. Superseded on
   2026-11-18: such a row is the bank's correction of the stored row, which the import
   replaces; it is flagged only when that can't be done (decision record "a bank's
   correction supersedes the stored row").
4. **Re-importing a file stores nothing.** With no new transaction and no new balance,
   `commit` returns `NothingNew` and records no batch. A balance from a file is new
   unless an earlier import recorded the same balance on the same day; undoing that
   earlier batch takes the balance with it, and importing the file again restores it.
5. **From running balances, only the opening is recorded.** A file with a balance column
   is checked row by row (in whichever order fits: oldest or newest first), and records
   one `balance_snapshot`: the balance at the end of the day before its first row. The
   last day of an export taken during the day is incomplete, so an end-of-day balance
   for it would fail once that day's later rows arrive. The next file's opening checks
   this one's rows. Balances the file states (OFX `LEDGERBAL`) are recorded as given;
   the importer is responsible for their date being an end of day.
6. **The manual anchor is an entered balance.** `account::add_balance` records a
   `manual` (or `statement`) `balance_snapshot`: an end-of-day balance, one per account
   and day. It is checked like every other balance (Review queues, point 1). The
   account's `opening_balance_minor` / `opening_date`, when set, count as one more
   end-of-day balance on that date, so nothing entered there is ignored; core itself
   records entered balances as snapshots.
7. **Undo removes the batch's rows with everything on them.** `import::undo` deletes the
   batch's transactions (mirrored pot rows included), balances and flags and marks it
   `undone`, in one DB transaction. The user's edits on those rows (category, notes,
   share, exclusion, splits, tags, a user-set name; later links) go with them:
   `undo_preview` counts the edited rows so the confirmation can say so, and importing
   the file again brings the rows back without the edits. Undo deletes split lines and
   tag links first (M1.20, M1.21); tables that will reference a transaction must be
   cleared by undo when they arrive, and until then a reference makes the undo fail as
   a whole.
8. **Undo dissolves transfer pairs.** A `transfer_group` with one of the batch's rows is
   deleted and its other rows become ordinary transactions again, to be paired again
   when the file is re-imported (M1.16). A pair with one side gone isn't a transfer.
9. **Undo names later imports that relied on the batch.** A later overlapping import
   skipped the rows this batch already had, so undoing it removes them from the
   account. `undo_preview` lists later committed batches into the same account;
   importing those files again brings the rows back.
10. **Locks hold.** A new row dated on or before `reconciled_through` is flagged and stays
    out; undo refuses a batch with a transaction or balance on or before its account's
    lock date. Both need the user to unlock first (M1.27).

### Decision record: importer paths and locked balances (2026-11-11)

From M1.19, which added end-to-end tests for every importer path (CSV, OFX/QFX,
presets, dedupe, reconciliation, commit, undo, CLI; `crates/import/tests/*_paths.rs`,
`crates/app/tests/cli_paths.rs`) and found one bug.

1. **A balance from the file in a locked period is left out.** A running-balance opening
   or an OFX `LEDGERBAL` dated on or before the account's `reconciled_through` is
   `FileBalanceStatus::Locked`: shown in the preview, not recorded and not checked, like
   a row in the locked period. Recording it added a check inside the lock and made the
   batch impossible to undo without unlocking; a file wholly inside the lock now imports
   nothing.
2. **Test presets aren't bundled.** Mappings that read the generic CSV fixtures live in
   `fixtures/csv/mappings/<region>/*.toml` and are loaded by the tests only.

### Decision record: the import flow (2026-11-14)

M3.4 and M3.5. Marked **(user)**: decided by the user on 2026-11-14 when asked before
the build. Everything else follows `DESIGN.md` → "Import", the CLI's `coffer import`, or
the records above ("Import commit, dedupe and undo", "The CSV importer", "Presets").

1. **Undrawn states are composed from existing specs (user)**, as in M3.2 and M3.3: the
   Map step for a file no preset recognises (FILE section, Format candidates, the
   money-out / money-in pair), OFX (Format locked, Statement select), a file that isn't
   a statement, Save as preset, and the notification after an import. Recorded in
   `DESIGN.md` → "Import" → "Built in M3.4".
2. **Choosing follows `coffer import`, with the user in the loop.** OFX by its format; a
   CSV preset by its own header row, one match only (several: the one an account uses,
   else the first, and the caption says to check). Otherwise the account's preset
   (`account.preset_id`, V15), but only if it reads the file: a preset without a header
   row can only be chosen this way. Otherwise the columns are suggested
   (`csv::MappingDraft::suggest`: header names, then values; deterministic, no model).
   A date format that reads the values more than one way (`01/02/2026`) is left for the
   user, never guessed; formats that read every value as the same dates count once. The
   account is the only one using the preset, else the accounts panel's selection when it
   can take the file, else the only account that can, else the user chooses. Choosing an
   account with a preset switches to it unless the user picked a preset.
3. **A changed preset is your own mapping.** While the mapping equals the preset's, the
   statement is the preset's (its ID on the batch, its category map); once a column
   changes it's read through `CsvImporter` over the user's `CsvMapping` with no preset
   ID and no category map, in the account's currency. Saving it (point 6) makes it a
   preset again.
4. **Mapping in memory on the main thread; the database in the background.** Reading,
   splitting and normalizing an export takes milliseconds, so the preview follows every
   change at once (`import::session::Session`, pure and unit-tested). Everything that
   touches the database or the disk runs on a background thread through
   `import::store::ImportStore`, which opens the database per call through
   `backup::gate`, like the other stores: the setup (presets, accounts, recent imports),
   reading the file, the review, the commit (core takes the backup), undo, saving a
   preset and its copy.
5. **The Review step's AFTER IMPORT figures are a rehearsal.** `import::rehearse` runs
   the commit's own steps (rows, pots, names, rules, transfer pairs, duplicates, links,
   schedules) in a DB transaction that is rolled back (`change::dry_run`), with no
   backup and no confirmation needed, refusing a stale preview like `commit`. So the
   counts are exactly what the commit then reports (a test compares them).
   `CommitSummary::uncategorized` counts the batch's imported rows still needing a
   category (no category and not a confirmed transfer, or a split line without one, as
   the Transactions view's Uncategorised). `commit` and `rehearse` share `write_batch`.
6. **Saved presets are files in the data dir (user).** `<data dir>/presets/<region>/<bank>.toml`
   (`paths::presets_dir`, next to the database as `backups/` is), directories `0700`,
   files `0600`, laid out like the repository's `presets/` so a file can be contributed
   as it is. They load next to the bundled presets (`preset::load_user_presets`,
   `Presets::with_user_presets`) in the app, the account inspector's Preset select and
   the CLI (`coffer import`, `coffer accounts add --preset`). A saved preset can't take
   an ID the bundled or saved presets have, and a file isn't overwritten; one that no
   longer loads is left out with a warning (path and reason, never transaction data),
   so one broken file hides nothing else. Not in the database, so a backup restore
   doesn't roll presets back, and no migration was needed. Rejected: a `user_preset`
   table (V16).
7. **Save as preset and Save a copy (user).** On the Map step, "Save as preset…" asks a
   name and a region (UK, US, euro area); the ID is `<region>-<name as a slug>`
   (`preset::slug`, accents folded). It writes the mapping as preset TOML
   (`Preset::to_toml`, which `Preset::from_toml` reads back to the same preset: tested
   on every bundled preset and every field), saves it and sets it as the account's
   preset for the next import. "Save a copy…" writes the same TOML where the system's
   save dialog says (`0600`). A new preset's category map is empty: no screen maps a
   bank's categories yet. `Preset::new` builds a preset by writing and reading it, so a
   saved preset passes exactly the checks a file does.
8. **Confirming is the Review step's button.** It shows every count, the flagged rows,
   the reconciliation block and a locked period, and with a mismatch its button reads
   "Import and flag the mismatch"; pressing it passes core the confirmations the preview
   asks for. Its count is new rows only, as "Import commit, dedupe and undo", point 2,
   has it (DESIGN's "flagged rows are in the count" predates that record).
9. **Unlock… is inert here (user).** Locks belong to the balance checks review (M3.12),
   so the locked-period block shows the button disabled and says to unlock in Reviews;
   locked rows stay out, as core already does.
10. **After the commit**: the dialog closes, the accounts panel and Transactions read the
    database again, and a notification says what was imported with Undo
    (`import::undo`, the same as `coffer undo-import`; importing the file again brings
    the rows back). The notification stays until dismissed or replaced.
11. **No migration.** M3.4 and M3.5 add none.

### Decision record: a bank's correction supersedes the stored row (2026-11-18)

R.4, decided together with L.21. Before this, a source ID the account already held with
another date or amount was flagged `SourceIdConflict` and left out. Source fields are
never edited, so the stored row stayed wrong and the only remedy was undoing its whole
batch.

1. **The new version is a correction, not a conflict.** In the preview it is
   `RowOutcome::Corrects(Correction)`, counted in `PreviewCounts::corrections`, and it
   needs `Confirmations::corrections` like flagged rows and a mismatch. The CLI's `--yes`
   and the Review step's button ("Import N transactions and M corrections") accept it.
2. **The commit supersedes, in the import's one DB transaction.** The stored row is
   excluded with the new reason `superseded`, with `superseded_by` naming the new row. The
   new row is inserted with the same source ID. The source ID index and
   `transaction_insert_new` count only rows that aren't superseded, so an account holds
   one live row and any number of superseded ones per ID. The stored row is pointed at the
   new row's ID before the insert, so the foreign key is deferred, and triggers check that
   both rows share an account and a source ID.
3. **`superseded` is like `duplicate` for figures.** The row leaves the computed balance,
   spending, income, budgets and reports, and stays visible under the Excluded filter as
   "Replaced by correction". Only an import sets it: it isn't in the Exclude dialog
   (`ExclusionReason::CHOOSABLE`), exclusion edits, change batches and rules refuse or
   skip it, and only undoing the import clears it.
4. **When it can't apply, the row stays flagged.** `SourceIdConflict { existing,
   blocked }` says why: it isn't the same payment (point 11); the stored row is excluded;
   is in a suggested or confirmed transfer; has a suggested or confirmed link; it or the
   new version is dated on or before the lock; or it isn't an imported row (a pot's
   mirrored side). Each tie depends on the old date or amount, so the user resolves it
   first. A scheduled payment the stored row fulfils isn't a tie: the match moves
   (point 6).
5. **Every version is known.** A row equal to a version the account already replaced is
   already imported, so an older export never reverses a correction, and a bank that goes
   back to an earlier figure isn't followed (the live row keeps the latest one). Once the
   older import is undone its version is gone, and that file is a correction again. A
   third version supersedes the current live row.
6. **What the user set moves across, after the rules.** A category the user set or
   approved, a merchant name the user set, the note, the tags (added), `reimbursable`; the
   share if it fits the new amount and no rule set one; the split if the amount is
   unchanged. A rule's or preset's category or name is recomputed for the new row. The
   stored row keeps its own values; what didn't fit stays on it and is listed per row
   (`NotCarried`) in the preview and `CommitSummary::corrections` (the CLI prints both; the
   app's Review step shows the corrections and the rows keeping something back as counts,
   and the per-row list there waits on a design). These writes aren't a change batch:
   undoing the import deletes the new row, and undoing an earlier change batch reverts the
   stored row only, not the copy on its correction. The stored row's `schedule_match`
   moves to the correction unchanged (schedule, occurrence, `matched_by`, `matched_at`),
   even outside the schedule's tolerance: the user or the matcher already decided this
   payment fulfils that occurrence. `schedule_match` is read-only, so the move is a
   delete and re-insert in the commit's transaction (`schedule::move_match`), before the
   import's own matching, which skips matched rows, so the commit summary doesn't count it
   as new. A correction whose stored row had no match is matched like a new row: a
   pending row for 0 that posts as the scheduled amount fulfils its occurrence.
7. **Reconciliation counts the corrected rows.** The preview's movements swap each stored
   row for its correction. A balance stated before the bank corrected a row can then stop
   reconciling; that shows as a mismatch to confirm.
8. **Undo.** Undoing the correcting import deletes its rows and clears `superseded` on the
   rows they replaced, restoring them exactly (`UndoPreview::restored`); a replaced row in
   a locked period refuses the undo. Before the rows go, a correction's schedule match
   moves to the row it replaced: the moved one, one the import made for the correction,
   or one the user has made on it since. Each names the same payment, so the occurrence
   stays fulfilled; this is the one thing the restored row may hold that it didn't. If it
   has a match of its own (the user matched the excluded row by hand), it keeps that one
   and the correction's goes. Undoing the older
   import deletes its superseded rows with the rest of the batch and keeps the corrections
   as ordinary rows, with their schedule matches (`UndoPreview::superseded`). An import whose correction was itself corrected by a later
   import waits for that one (`Error::CorrectedAgain`): restoring its replaced row would
   put two live rows under one source ID.
9. **L.21, pending → posted.** When the bank keeps the ID, posting is a correction and
   this handles it. When the ID changes, the near-duplicate rules catch it if the amount
   holds: R.3's `unmatched_source_id` when date and text agree too, the 3-day window when
   the text changed. A posting with a new ID that also changes the amount (a tip added),
   or moves the date with the text unchanged, is caught by neither; L.21 stays open for
   those and for fixtures showing which exports include pending rows.
10. **Schema: V21** rebuilds `"transaction"` (12-step) with foreign keys off. The
    migration's first line, `-- coffer: foreign_keys=off`, asks `db::migrations::run` to
    turn them off around the run and to check every foreign key inside its transaction
    before it commits.
11. **A correction must be the same payment.** The source ID alone isn't enough: a bank,
    or a saved preset, whose IDs repeat across unrelated rows would turn them into
    corrections. The file row must also be dated within 31 days of the stored row, either
    way (`CORRECTION_WINDOW_DAYS`: a hotel, fuel or car-hire hold can settle weeks later),
    and move money the same way; a zero amount, such as a pending authorisation, fits
    either. The description isn't compared: a pending row's descriptor often differs from
    the posted one's. Otherwise the row stays flagged, `CorrectionBlocked::NotSamePayment`.
    A reused ID within the window and the same direction still reads as a correction; the
    user sees it listed (CLI) or counted (app) and must accept it.

### Decision record: a preset's dedupe identity is frozen (2026-11-18)

R.5. A row's dedupe identity is its source ID, else its fingerprint ("Import commit, dedupe
and undo"), and the fingerprint depends on what a preset reads: the columns it joins into
the description, the date format, the amount's sign handling and whether it maps an ID
column. Presets carry no version, and only the fingerprint's format was frozen by a test,
so editing a bundled preset could move identities silently and the next import of an
overlapping file would duplicate rows already in users' databases.

1. **Each bundled preset's fixture rows have a golden file.**
   `crates/import/tests/golden/<region>/<bank>.txt` holds each committed row's external ID
   (or `-`) and fingerprint in file order. `preset_golden.rs` imports the fixture through
   preview and commit into a scratch database, reads the identities back from the
   `transaction` table, and compares. `dedupe::fingerprint` stays private to core.
2. **A change that moves either needs a new preset ID**, a new preset with its own file,
   fixture and golden file, never an edited golden file. The preset ID is recorded on
   every batch, and the old preset stays loadable. Changes that leave every identity
   alone (category map, name, pot rules) keep the ID.
3. **A preset without a golden file fails the test.** `COFFER_BLESS_GOLDEN=1` writes the
   missing files only and never overwrites one. The rule is in `presets/README.md` →
   "Changing a preset".
4. **Saved presets aren't covered.** They live in the data folder, have no fixture in the
   repo, and the user owns them. Rejected: a version field on presets (a second identity
   to keep in step with the ID, and nothing would stop editing without bumping it).

### Decision record: a file's account number is checked, never stored (2026-11-19)

R.13. An OFX file states its own account (`ACCTID`), and the importer parsed it and kept
only its last four characters for choosing a statement, so a file imported into the wrong
account went unnoticed until its balances disagreed. Choices marked **(user)** were the
user's; the rest are **(composed)**.

1. **Warn, and learn on commit. (user)** The preview warns when the account has at least
   one known hash and the file's isn't among them, and importing anyway is allowed and
   adds the hash, so a replacement card warns once. No confirmation step: the warning
   isn't in `Confirmations`, and the CLI's `--yes` doesn't concern it. An account with
   no hash yet is warned too when another account has had the file's number (composed,
   lead: a new account is where another account's file most likely goes). A file with
   no account ID, a CSV, a number no account has had going into an account with no hash,
   and a database with no key give no warning.
2. **The number is stored only as a keyed hash. (user)** HMAC-SHA256 under a random
   per-database key in `setting` (`import.account_key`, 32 bytes as hex, like the
   pseudonym key; never shown or logged). Created by the first commit that has a number:
   the preview only reads, and without a key no hash exists to compare. The raw number
   lives only in memory, on `Statement` as `SourceAccountNumber`, whose `Debug` is
   redacted, with no `Display`, serialisation or public accessor. It is never in the
   database, logs, errors, exports or MCP output; a test scans the database file for it.
   `account_suffix` (last four, for choosing a statement) is unchanged.
3. **Normalising. (composed)** Whitespace and dashes are dropped and letters uppercased,
   so `12-3456 78` and `12345678` are one number and an IBAN's case or spacing doesn't
   matter. Nothing else is touched, and a number that is empty after that has none.
   Changing it later changes which hashes match, like the fingerprint's normalisation.
4. **A masked number is an identity. (composed)** Some banks mask it
   (`XXXX-XXXX-XXXX-9876`). The masked text is hashed as written: it tells accounts
   apart as far as the bank lets it, and a bank that changes its mask between exports
   warns once, then is learned.
5. **Schema: V23 `account_source_identity`. (composed)** One row per committed batch
   whose file had a number: `id`, `batch_id` (unique), `account_id`, `identity_hash`. An
   account knows a hash while any live batch has it, so undoing a batch deletes its row
   (core does it with the batch's flags and balances) and the account forgets a hash only
   if no other batch brought it. A batch with nothing new isn't recorded, so re-importing
   an old file into an account that predates V23 doesn't learn its number; the next
   import with something new does. `account_id` is `ON DELETE CASCADE` (composed), the
   one exception to "no `ON DELETE`" (V1): in practice an account with a batch can't be
   deleted, so the cascade is a backstop. `removal`'s foreign-key test is about
   references to `"transaction"` and the table has none, so it needs no change.
6. **The preview names the other account. (user)** When the hash is known to another
   account, the warning reads "This file looks like it's from <account name>"; if it is
   known to several, the one imported into most recently. Otherwise it says "a different
   account". The Review step shows it as a `warning-soft` block, and `coffer import`
   prints a `Warning:` line (also on `--dry-run`). When the account has had no other
   number, the block's body says only that the other account has had this one ("Files
   with this account number have gone into Savings."). **(composed)** from the locked-period
   block's box and the wide rule's `warning` triangle ("Import" in `docs/DESIGN.md`).
7. **MCP never reads the table. (composed)** No tool queries it and `coffer mcp` has no
   use for the key; like the pseudonym key it stays in the database and out of every
   output. Backups contain it as they contain the rest of the database.
8. **Deterministic. (composed)** No network or model call: the check is a hash and
   three queries.
9. **Rejected.** A confirmation step before importing (the user chose a warning, not a
   gate); storing the number encrypted (a key in the same database protects no better than
   a hash, and the number is never needed back); a plain unkeyed hash (a short numeric
   account number could be recovered by trying every number); no warning at all for an
   account with no hash (it would miss account A's file going into a new account B).
