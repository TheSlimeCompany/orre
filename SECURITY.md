# Security

What Orre does to protect your financial data, what it doesn't, and how to report a
problem. For where your data lives, how to back it up and restore it, and how to turn AI
access off, see [`docs/data-and-privacy.md`](docs/data-and-privacy.md).

## Supported versions

Orre is pre-release, with no supported releases yet. Security fixes go on `main`.

## Reporting a vulnerability

Report it privately through
[GitHub private vulnerability reporting](https://github.com/TheSlimeCompany/orre/security/advisories/new)
(**Security** tab → **Report a vulnerability**). Don't open a public issue for a
security bug. If you can't use GitHub, email lorreslime@gmail.com instead.

Include what goes wrong and what an attacker gains, steps to reproduce with synthetic
data only (never a real statement), the commit and OS you tested, and the MCP access
level or client setup it needs. Orre is a one-person project, so responses are best
effort.

## What Orre protects

- **Your data stays local,** in one SQLite file, `orre.db`, in the platform data
  directory (`~/.local/share/orre` on Linux, `~/Library/Application Support/Orre` on
  macOS) or in `ORRE_DATA_DIR`. On Linux and macOS the file is created `0600`, looser
  permissions are tightened when Orre opens it, and a data directory Orre creates is
  `0700`.
- **No account, no bank credentials, no telemetry.** You import files downloaded from
  your bank. Orre makes no network calls; the only one planned is an opt-in ECB
  exchange-rate fetch, which isn't built yet.
- **Imports never go through AI.** Importing is deterministic, with no model or network
  call.
- **AI access is off until you allow it.** The MCP server uses stdin/stdout and opens no
  port. A client gets no tools until you enable it in the app or CLI, at its own level:
  `aggregate` (totals), `pseudonymous` (rows, with merchants as tokens and accounts as
  labels) or `standard` (full access).
- **AI clients can't change your data.** MCP tools only read, apart from filing
  suggestions, which wait in an inbox for your approval, and saving reports.
- **Some data never leaves through MCP:** notes, share labels, private tags, merchant
  addresses and attachments. Bank text holding a sort code, IBAN or 8+ digit number is
  withheld too. That limits what MCP returns, not what Orre stores: descriptions are
  kept as your bank wrote them, account numbers included, in the database and its
  backups. Only the account number a statement gives for its own account is stored
  as a keyed hash instead.
- **Bank text is treated as untrusted.** Descriptions and merchant names are labelled
  untrusted, stripped of invisible characters, have domains made inert
  (`example[.]com`), and are withheld when they read like instructions to an AI.
- **You can see and stop AI access.** Every tool call goes into an access log (arguments
  reduced to their shape, results not stored). The kill switch, `orre ai-access off` or
  the switch in Settings, refuses every client from its next call.

## What Orre does not protect against

- **The database isn't encrypted at rest.** Anything that can read your files can read
  your finances: malware, another process running as you, another admin, a disk or copy
  without encryption. Database encryption is planned (roadmap L.6).
- **Access levels aren't a security boundary.** They limit what the MCP server hands a
  client, not what a local process can read from the file. A client name isn't a
  password, and an AI client with a shell can read the file or run `orre` as you.
- **Your AI client sends what it reads to its model provider,** under that provider's
  terms. At `standard` that includes account and merchant names and bank descriptions,
  which can contain addresses or people's names: the check catches common account
  identifiers, not every personal detail. At `pseudonymous`, exact amounts and dates can
  still identify a merchant.
- **The instruction filter is pattern matching.** Cleverly phrased text can get past it;
  the containment is that a client can only file suggestions you approve.
- **Backups and exports are plain files.** They're unencrypted, and backups keep data
  you've since deleted until they're rotated out or you delete them. Protect them like
  the database.
- **Windows** isn't an official platform, and the permission steps above don't apply
  there.

## Recommendations

- Turn on full-disk encryption: FileVault on macOS, LUKS on Linux.
- Keep backups and exports only somewhere encrypted.
- Start each AI client at the lowest level that works, in a profile with Orre as its
  only data source and no web tools or shell ([`docs/mcp-setup.md`](docs/mcp-setup.md),
  step 3).
- Review the access log: Settings → Access log, or `orre ai-access log`.
- Cut access with `orre ai-access off` (every client) or `orre ai-access revoke <name>`.

## Scope

In scope, for example: an MCP tool returning data its level, or every level, should
withhold; an MCP call writing anything but a suggestion or report, or a suggestion
taking effect without approval; bank text or an imported file causing an action instead
of being read as data; Orre making a network request or loosening file permissions.

Out of scope, for example: an attacker who already runs code as you or has admin
rights; what an AI client or its model provider does with data Orre gave it at the level
you chose; text that slips past the instruction filter but can only produce a
suggestion you'd have to approve (welcome as a hardening report, not a vulnerability);
dependency bugs with no effect on Orre (report those upstream).
