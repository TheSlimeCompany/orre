# Security

What Orre does to protect your financial data, what it doesn't, and how to report a
problem.

## Supported versions

Orre is pre-release. There are no supported releases yet, so there are no backports:
security fixes go on `main`.

## Reporting a vulnerability

Report it privately through GitHub's private vulnerability reporting on
[lorrehuggan/orre](https://github.com/lorrehuggan/orre/security/advisories/new)
(the repository's **Security** tab → **Report a vulnerability**). Please don't open a
public issue or pull request for a security bug.

<!-- TODO: private vulnerability reporting is not yet enabled on lorrehuggan/orre
     (Settings → Code security → Private vulnerability reporting). Enable it before
     relying on this section. -->

<!-- TODO: add a contact email for reporters who can't use GitHub, if one is wanted. -->

Include:

- what the problem is and what an attacker gains;
- steps to reproduce it, with synthetic data only (never a real bank statement);
- the commit you tested and your OS;
- whether it needs a particular MCP access level or client setup.

Orre is a one-person project, so responses are best effort.

## What Orre protects

- **Your data stays local.** It lives in one SQLite file, `orre.db`, in the platform
  data directory: `~/.local/share/orre` on Linux (or `$XDG_DATA_HOME/orre`),
  `~/Library/Application Support/Orre` on macOS, or `ORRE_DATA_DIR` if you set it. On
  Linux and macOS the file is created with mode `0600`, looser permissions are
  tightened when Orre opens it, and a data directory Orre creates is `0700`.
- **No account, no bank credentials, no telemetry.** You import files downloaded from
  your bank. Orre makes no network calls; the only one planned is an opt-in fetch of
  ECB exchange rates, which isn't built yet.
- **Imports never go through AI.** Importing is deterministic code with no model or
  network call.
- **AI access is off until you allow it.** The MCP server talks over stdin/stdout and
  opens no port. A client gets no tools until you enable it by name in the app or CLI,
  and each client has its own level: `aggregate` (totals only), `pseudonymous` (rows,
  with merchants as tokens and accounts as labels) or `standard` (full access).
- **AI clients can't change your data.** MCP tools are read-only apart from two writes:
  filing suggestions, which wait in an inbox until you approve them in Orre, and saving
  reports. Approving, enabling a client and raising its level are done in the app or in
  a terminal.
- **Some fields never leave through MCP, at any level:** notes (the bank's and yours),
  share labels, private tags, merchant addresses and attachments. Orre doesn't store a
  file's account number (only a keyed hash, to warn about imports into the wrong
  account), and bank text holding a sort code, IBAN or 8+ digit number is withheld.
- **Bank text is treated as untrusted.** Descriptions and merchant names are labelled
  untrusted, stripped of invisible characters, have domains made inert
  (`example[.]com`), and are withheld when they match patterns that read like
  instructions to an AI.
- **You can see and stop AI access.** Every tool call is recorded in an access log
  (arguments reduced to their shape, results not stored), kept for 12 months. The kill
  switch (`orre ai-access off`, or the switch in Settings) refuses every client from its
  next call.

## What Orre does not protect against

- **The database is not encrypted at rest.** Anything that can read your files can read
  your finances: malware, another process running as you, an administrator on the
  machine, a stolen disk without full-disk encryption, or an unencrypted copy of the
  file. Database encryption is planned (roadmap L.6).
- **Access levels are not a security boundary.** They limit what the MCP server hands an
  AI client. They do nothing against a process that reads the SQLite file directly. The
  client name isn't a password either: any program on your computer can start
  `orre mcp` under any name. An AI client with a shell can read the file or run `orre`
  as you; the terminal check on approvals is a speed bump, not a lock.
- **Your AI client sends what it reads to its model provider**, under that provider's
  terms. At `standard` that can include account and merchant names, and bank
  descriptions, which may contain personal details such as addresses or people's names.
  The description check catches common account identifiers, not every personal detail.
  At `pseudonymous`, exact amounts and dates can still identify a merchant behind its
  token.
- **The instruction filter is pattern matching.** Text phrased to slip past it can reach
  the model. The containment is that an AI client can only file suggestions you approve.
- **Backups and exports are plain files.** Automatic backups sit next to the database in
  `backups/` (`0600`, directory `0700`) and exports go where you save them. Neither is
  encrypted, and backups keep data you've since deleted until they're rotated out or you
  delete them. Protect them as you protect the database.
- **Windows** isn't an official platform, and the file-permission steps above apply on
  Linux and macOS only.

## Recommendations

- Turn on full-disk encryption: FileVault on macOS, LUKS on Linux.
- Keep copies of backups and exports only somewhere encrypted.
- Start each AI client at the lowest level that does the job (`aggregate` first), and run
  it in a profile with Orre as its only data source, no web tools and no shell
  ([`docs/mcp-setup.md`](docs/mcp-setup.md), step 3).
- Review the access log now and then: Settings → Access log, or `orre ai-access log`.
- To cut access, `orre ai-access off` stops every client, and `orre ai-access revoke
  <name>` stops one. Both work from any shell.

## Scope

In scope, for example:

- an MCP tool returning a field its access level should withhold, or a field withheld at
  every level (notes, account identifiers, private tags);
- an MCP call writing anything other than a suggestion or a report, or a suggestion
  taking effect without your approval;
- text in an imported file or a bank description causing an action, rather than being
  read as data;
- Orre making a network request, or loosening the database's or a backup's permissions.

Out of scope, for example:

- an attacker who already runs code as your user or has admin rights (see "What Orre does
  not protect against");
- what an AI client or its model provider does with data Orre gave it at the level you
  chose;
- a crafted description that gets past the instruction filter but can only produce a
  suggestion you then approve: report it if you think the filter should catch it, but
  it's treated as a hardening request;
- bugs in dependencies with no effect on Orre (report those upstream).
