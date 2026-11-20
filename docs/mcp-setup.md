# Using Orre from an AI client (MCP)

Orre runs a local [MCP](https://modelcontextprotocol.io) server, `orre mcp`, that AI
clients such as Claude Code, Claude Desktop and Codex start themselves. It talks over
stdin/stdout only: no port is opened and nothing leaves your machine except what the
AI client itself sends to its model provider. Design and rules: `ARCHITECTURE.md` →
"MCP surface".

What a client can do:

- **Read** your data through tools such as `monthly_summary`, `spending_by_category`,
  `search_transactions` and `recurring_payments`, limited by the access level you give it.
- **Suggest** changes (categories, rules, transfer pairs, merchant names, refund links)
  and **save reports**. Suggestions wait in Orre's inbox until you approve or reject
  them; nothing the AI does changes a transaction, rule, category or account.
- Use the **`monthly_review` prompt**, which walks a month and saves the review as a
  report.

Every call is recorded in Orre's access log.

## Before you start

- You need the `orre` binary. Orre isn't packaged yet, so build it from the repository
  in the dev shell (`cargo build --release -p orre-app`) and use the absolute path of
  `target/release/orre` (`realpath target/release/orre`). Clients start the server
  without your shell's `PATH`, so every example below gives `command` as an absolute path,
  shown as `/path/to/orre`. A binary built in the dev shell also runs outside it: it
  finds its libraries in the Nix store. If a garbage collection removes them, rebuild.
- A database must exist: import something first (`orre import <file>`). The server
  never creates or migrates a database.
- If you keep your data somewhere other than the default data directory, set
  `ORRE_DATA_DIR` in the client's configuration (each example below shows where).

## 1. Choose a name and level for the client

Each client is named in its own configuration with `orre mcp --client <name>`. The name
is how Orre recognises it: letters, digits, `.`, `_` and `-`, for example
`claude-code`, `claude-desktop` or `codex`. It isn't a password. Any program on your
computer can start `orre mcp` with any name, and any program running as you can read
the database file. The levels guard against over-sharing with a client you trust; they
are not a security boundary.

There are three levels:

| Level | The client sees |
|---|---|
| `aggregate` | Totals only: monthly summaries, spending by category, budgets, net worth (accounts by type and number, not name), categories and its own saved reports. It can save reports. No transaction rows. |
| `pseudonymous` | Also transaction rows and recurring payments, with every merchant replaced by a stable token (`M-3f9a0c…`), accounts shown as labels (`current 1`, `pot 2`) rather than names, and filters by date, account, category and amount. No bank descriptions, no text search, no tags. |
| `standard` | Everything above with real merchant names, account names, bank descriptions (as labelled, sanitised untrusted text), text search, public tags, rules and merchant history, and the `propose_*` tools. Full access by design: a merchant name or bank description can be a person's name. Merchant names are untrusted text, like descriptions: capped, with domains shown as `example[.]com`, and withheld if they read like instructions. |

`pseudonymous` limits casual exposure; it isn't anonymity. Tokens hide merchant names,
but amounts, dates and categories are exact, and they can identify a merchant: a £10.99
payment on the 3rd of every month under subscriptions is easy to guess. Choose
`aggregate` if that matters.

Notes, share labels, private tags, merchant addresses, account numbers, sort codes and
IBANs are never sent at any level.

Enable the client, then set its level. In the app, Settings (⌘, or the title bar's
Settings button) → AI clients lists a client that asked to connect under "Asked to
connect", with "Enable…", where you choose its level; each enabled client has a level
select and an on/off switch, the toolbar has the kill switch, and Settings → Access log
shows the log. From a terminal, enabling starts at `aggregate`:

```sh
orre ai-access enable claude-code
orre ai-access level claude-code pseudonymous    # or aggregate, standard
orre ai-access list                              # every client, its level and status
```

A client you haven't enabled can still start `orre mcp`, but it sees no tools and no
prompts. Orre records its name as `requested`, so `orre ai-access list` shows who
asked. `orre ai-access ignore <name>` hides a request.

A change applies to a running client's next call: you don't need to restart it. When a
client's level changes (or it's revoked, or the kill switch is off), the server also sends
`notifications/tools/list_changed`, within about 5 seconds even when the client is idle.
Claude Code then fetches the new tool list. Codex and Claude Desktop don't document what
they do with it, so restart them to see the new list. Either way, a tool above the level is
refused when called.

## 2. Configure the client

Each example names the client after itself; use the name you enabled. Replace
`/path/to/orre` with the absolute path of your `orre` binary (see "Before you start"),
and leave out `ORRE_DATA_DIR` when you use the default data directory. Notes such as
*(checked)* say how each step was checked in November 2026: Claude Code 2.1.316 and Codex
CLI 0.166.0 were run on Linux against a synthetic database; the rest comes from each
client's documentation only.

### Claude Code

Add Orre for your user, in every project *(checked)*:

```sh
claude mcp add --scope user orre -e ORRE_DATA_DIR=/path/to/your/orre/data \
  -- /path/to/orre mcp --client claude-code
```

Put the server name (`orre`) before `-e`: `-e` takes several values, and a name right
after it is read as another `KEY=value` and rejected. Everything after `--` is the
command Claude Code runs. `--scope user` writes the server to `~/.claude.json` (in your
home directory on every OS, or under `CLAUDE_CONFIG_DIR` if you set it). Without
`--scope`, the server is added for the current directory only (`local` scope, also in
`~/.claude.json`).

Or add it to one project, in a `.mcp.json` at the project's root *(checked)*:

```json
{
  "mcpServers": {
    "orre": {
      "command": "/path/to/orre",
      "args": ["mcp", "--client", "claude-code"],
      "env": { "ORRE_DATA_DIR": "/path/to/your/orre/data" }
    }
  }
}
```

Claude Code asks you to approve a project's `.mcp.json` servers when you first start
`claude` there. Until you do, they show as `⏸ Pending approval`.

**Check it works** *(checked)*. `claude mcp list` (or `claude mcp get orre`) starts the
server, checks it, and doesn't call a model:

```text
$ claude mcp list
Checking MCP server health…

orre: /path/to/orre mcp --client claude-code - ✔ Connected
```

- `✔ Connected` means the server started and found the database. It doesn't mean
  the client is enabled: a name Orre doesn't know connects too, and it gets no tools.
  `orre ai-access list` shows whether it's enabled. A name the server saw but you haven't
  enabled is listed as `requested`.
- `✘ Failed to connect — ENOENT: Executable not found in $PATH`: `command` isn't an
  absolute path to the binary.
- `✘ Failed to connect — CONNECTION_CLOSED`: the server started and then stopped. To see
  why, run the same command in a terminal with stdin closed, for example
  `ORRE_DATA_DIR=/path/to/your/orre/data /path/to/orre mcp --client claude-code < /dev/null`.
  `there is no Orre database at …` means the data directory is wrong or empty.
  `connection closed: initialize request` means the server found the database and only
  stopped because nothing talked to it, so the server itself is fine.

In a session, `/mcp` lists Orre's tools.

### Claude Desktop

*(Documentation only: Claude Desktop wasn't installed where this was checked.)* Claude
Desktop runs on macOS and Windows. On Linux it's in beta, for Ubuntu 22.04+ and Debian 12+
only; on other distributions, use Claude Code. Edit `claude_desktop_config.json`:

| OS | File |
|---|---|
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

The app opens it for you from **Settings → Developer → Edit Config**, and creates it if it
doesn't exist. In newer versions the local MCP servers are under **Settings → Desktop app →
Extensions**.

```json
{
  "mcpServers": {
    "orre": {
      "command": "/path/to/orre",
      "args": ["mcp", "--client", "claude-desktop"],
      "env": { "ORRE_DATA_DIR": "/path/to/your/orre/data" }
    }
  }
}
```

`command` must be an absolute path: Claude Desktop doesn't see your shell's `PATH`. On
Windows, write each `\` in a path as `\\`. Quit Claude Desktop completely and start it
again after each edit.

**Check it works.** Click **+** in the chat box, then **Connectors**: Orre should be
listed with its tools. The **Local MCP servers** section (or Settings → Developer in older
versions) shows its status. If it doesn't start, look in Claude Desktop's logs:
`mcp-server-orre.log` holds the server's error output. The logs are in
`~/Library/Logs/Claude` (macOS), `%APPDATA%\Claude\logs` (Windows) or
`~/.config/Claude/logs` (Linux). Then run the command in a terminal, as described under
Claude Code above.

### Codex

Add Orre with the CLI *(checked)*:

```sh
codex mcp add orre --env ORRE_DATA_DIR=/path/to/your/orre/data \
  -- /path/to/orre mcp --client codex
```

or write it yourself in `~/.codex/config.toml` (`%USERPROFILE%\.codex\config.toml` on
Windows, or `$CODEX_HOME/config.toml` if you set `CODEX_HOME`). This is what the command
above writes *(checked)*:

```toml
[mcp_servers.orre]
command = "/path/to/orre"
args = ["mcp", "--client", "codex"]

[mcp_servers.orre.env]
ORRE_DATA_DIR = "/path/to/your/orre/data"
```

A project's `.codex/config.toml` works too, but only in a project you've marked as
trusted. The Codex CLI, the IDE extension and the ChatGPT desktop app share this file.

**Check it works.** `codex mcp list` and `codex mcp get orre` show the configuration
*(checked)*, but they don't start the server: their `enabled` is the setting, not a
connection. To check the command itself, run it in a terminal as described under Claude
Code above *(checked: the command Codex wrote, started the way an MCP client starts it,
listed the seven `aggregate` tools and the `monthly_review` prompt)*. In a Codex session,
`/mcp` lists the servers that are running *(documentation only)*. Codex gives a server 10
seconds to start (`startup_timeout_sec`).

## 3. Use a profile with Orre as the only data source

Transaction text comes from your bank statements, and anyone who can put text on a
statement (a payee's reference, a merchant's name) can try to give the AI instructions.
Orre withholds text that reads like instructions and marks bank text as untrusted, and
the AI can only *suggest* changes. The remaining risk is an AI that reads your data and
then sends it somewhere. So run Orre in a client profile that has:

- Orre as its only MCP server: no other servers that read or send data;
- no web search, web fetch or browser tools;
- no shell or network access it doesn't need. A client with a shell can run `orre`
  itself, or read the database file, as you.

How to do that, per client (check your client's current documentation; options change):

- **Claude Code**: put the `.mcp.json` above in a file of its own, say
  `orre-only.json`, in an empty directory, and start sessions from there with:

  ```sh
  ENABLE_CLAUDEAI_MCP_SERVERS=false \
    claude --strict-mcp-config --mcp-config orre-only.json --tools ""
  ```

  - `--strict-mcp-config` uses only the servers in `--mcp-config` and ignores every
    other MCP configuration, such as your user, project and local servers.
  - `--tools ""` turns off every built-in tool: the shell, file tools, `WebFetch` and
    `WebSearch`. MCP tools aren't affected, so Orre's tools stay.
  - `ENABLE_CLAUDEAI_MCP_SERVERS=false` keeps out the connectors you've added on
    claude.ai *(documentation only)*.

  If you want some built-in tools, name them instead (`--tools "Read"`), or keep the
  defaults and remove the web tools with `--disallowedTools WebFetch WebSearch`. That
  still leaves the shell, which can reach the network. These options each take a list,
  so put nothing after them: a prompt there is read as another file or tool name. *(Checked:
  `claude --help`, and Claude Code accepted these flags with this file. A session can't
  be started without signing in, so the session itself wasn't checked.)*
  `claude mcp list` ignores `--mcp-config`. To check the file, use it as a project's
  `.mcp.json` once (see "Check it works" above).
- **Claude Desktop** *(documentation only)*: keep Orre the only entry under
  `mcpServers`, turn off other extensions under Settings → Desktop app → Extensions, and
  turn off web search and other connectors (**+** → **Connectors**) in the conversations
  where you use Orre.
- **Codex**: profiles (`--profile`) only add settings on top of `~/.codex/config.toml`,
  so they can't remove servers already set up there. Give Orre a Codex home of its
  own instead, say `~/.codex-orre`, and start Codex with `CODEX_HOME=~/.codex-orre codex`.
  Its `config.toml` *(checked: Codex accepts these settings and rejects invalid values;
  what they do comes from its documentation)*:

  ```toml
  web_search = "disabled"     # the default, "cached", is on
  sandbox_mode = "read-only"  # commands run without network access

  [features]
  apps = false                # no ChatGPT connectors

  [mcp_servers.orre]
  command = "/path/to/orre"
  args = ["mcp", "--client", "codex"]

  [mcp_servers.orre.env]
  ORRE_DATA_DIR = "/path/to/your/orre/data"
  ```

  Codex keeps its login in its home too, so sign in once with
  `CODEX_HOME=~/.codex-orre codex login`. Web search and connectors aren't covered by
  the sandbox's network rules, which is why they're turned off here on their own
  *(documentation only)*.

## 4. Ask, review, approve

Ask in the client, for example *"Where did my money go last month?"*, or use the
`monthly_review` prompt with a month (`2026-10`). The client calls Orre's tools; each
call carries a short `purpose`, which you'll see in the log.

Suggestions and reports wait in the inbox:

```sh
orre inbox                       # pending suggestions
orre inbox show 12               # one suggestion, with what approving it would change
orre inbox approve 12 13         # approve several at once: all or nothing
orre inbox approve 14 --apply-to-history   # a rule, also applied to past rows
orre inbox reject 15 --reason "wrong category"
```

Approving is yours to do. `orre inbox approve`, `orre ai-access enable` and the other
commands that change your data or widen AI access refuse to run unless they're attached to
an interactive terminal, so run them yourself, or use the app. (`orre inbox reject`,
`orre ai-access off`, `revoke` and lowering a level work from anywhere, so a script can
always shut a client out. Previews work from anywhere too: `orre import --dry-run`, and
`undo-import`, `schedules delete`, `inbox delete-report` and `backup --delete` without
`--yes`, show what would happen and change nothing.) The check is a speed bump, not a lock:
a client with a shell tool can fake a terminal or open the database file directly. That's
why the profile in step 3 gives the client no shell; if it has one, sandbox it away from
Orre's data directory and the `orre` binary.

Saved reports:

```sh
orre inbox reports               # saved reports, newest first (no bodies)
orre inbox report 7              # one report, as plain text
orre inbox delete-report 7 8     # lists what would go; add --yes to delete
orre inbox delete-report --client claude-code --before 2026-01-01 --yes
```

A client reads back only its own reports, and only those saved at its current level or
below: a client lowered to `aggregate` doesn't see the review it wrote at `standard`. You
see every report. Each client can keep at most 500 saved reports. Once it reaches that, it can't save more
until you delete some; `orre inbox reports` flags a client that's at the cap. Only you
can delete reports: no MCP tool can.

Text an AI wrote is shown labelled as unverified, without hidden characters; a report's
markdown and links are printed as they are, never rendered or opened.

## 5. Check what the AI read: the access log

```sh
orre ai-access log                       # the latest 50 calls, newest first
orre ai-access log --client codex --limit 200
```

Each row shows the time, client, tool, the level and outcome (`ok`, `refused_off`,
`refused_not_enabled`, `refused_level`, `refused_restoring`, `refused_unavailable`,
`error`), how many rows the call returned, how many
descriptions were withheld as instructions, the client's `purpose`, and the call's
arguments with any free text replaced by its length (`{"length":12,"type":"string"}`).
The log never stores what was returned. Rows older than 12 months are deleted when a
server starts.

Calls refused because the database couldn't be opened are logged too, a little later:
`refused_restoring` while a backup was being restored, `refused_unavailable` when the
database needed updating (migrations pending) or the restore lock couldn't be checked.
The log is inside the database, so the server keeps them in `orre.db-access-pending`
next to it (same fields, same redaction) and moves them in, at the time they happened,
on the next call that can open the database, or when you run `orre ai-access`. If a
client keeps calling for a long time while the database is unavailable, the file stops at
1,000 calls; the rest are counted, and the log shows one row per client and tool with
how many (`refused_restoring (40 calls)`).

## The kill switch

```sh
orre ai-access off    # every client is refused from its next call
orre ai-access on     # the enabled clients work again, at their levels
orre ai-access revoke codex   # one client; it stays listed and can be enabled again
```

`off` takes effect on a running client's next call and is logged as `refused_off`.

## Restoring a backup while a client is connected

`orre restore` works while a client is connected: the server keeps no database
connection open between calls. While the files are swapped it refuses calls with
"Orre is restoring a backup", and its next call reads the restored database. Those
refused calls are in the restored database's log as `refused_restoring`, at the time
they were made. Restoring an older backup doesn't roll the log back either: the calls
logged since that backup was taken are copied into the restored database, and
`orre restore` says how many.

If a restore is killed or crashes, nothing stays blocked: the restore's lock on
`orre.db-restoring` goes with its process, and the database is complete either way (the
one before the restore, or the backup). The file itself stays next to the database, empty
between restores; that's expected. The next `orre restore` warns that an earlier one
stopped, and goes ahead.

## Troubleshooting

- **No tools listed**: the client isn't enabled (`orre ai-access list`; enable it under
  the exact name in its configuration), or the kill switch is off.
- **"Orre couldn't open its database"**: no database at the data dir the server sees
  (check `ORRE_DATA_DIR`), or it needs migrating: run any `orre` command, such as
  `orre accounts`, which updates it.
- **A tool is missing**: it's above the client's level (`orre ai-access level`).
- **"Orre can't tell whether a backup is being restored"**: the server can't open or lock
  `orre.db-restoring` next to the database. Check that you own it and that the data dir
  is on a local disk (file locks may not work on a network share). When no `orre
  restore` is running, deleting the file is safe.
- **Debug output**: set `ORRE_LOG=debug` in the server's `env`. Logs go to stderr and
  never hold transaction text at `info` or above.
