# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`GLOSSARY.md`** at the repo root.
- **`docs/ARCHITECTURE.md`** → "Contents", then the decision records in the
  `docs/architecture/<topic>.md` files that cover the area you're about to work in.

If `GLOSSARY.md` doesn't exist, **proceed silently**. Don't flag its absence; don't suggest
creating it upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and
`/improve-codebase-architecture`) creates it lazily when terms actually get resolved.

## File structure

Single-context repo:

```
/
├── GLOSSARY.md
├── docs/
│   ├── ARCHITECTURE.md          ← overview + "Contents": every section and decision record
│   └── architecture/
│       ├── import.md            ← reference sections, then "## Decision records", oldest first
│       ├── mcp.md
│       └── …
└── crates/
```

## Where decisions go (overrides `docs/adr/` in ADR-FORMAT.md)

This repo has no `docs/adr/`, and you must not create one. Its ADRs are the decision records:

- A new one goes at the end of its topic file in `docs/architecture/`, under
  `## Decision records`, as `### Decision record: <title> (<YYYY-MM-DD>)`.
- Add it to the list in `docs/ARCHITECTURE.md` → "Contents", under its topic.
- Keep the content ADR-FORMAT.md asks for (context, decision, consequences); only the location
  and heading differ.
- Changing an invariant in `AGENTS.md` always needs a decision record.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Choosing between candidate names

When two or more names compete for a concept, scenario-test them before choosing: a table
of the candidates against real sentences (a UI label, a CLI message, an MCP tool
description, an instruction to an agent, and the name said aloud while working). Reject
any candidate that clashes with an existing glossary term or a code term (Rust, GPUI,
SQLite, MCP); qualify it instead, as in "transfer pair" rather than "pair". Terms likely
to collide here: batch (import batch, `change_batch`), rule, check, review.

## Flag decision conflicts

If your output contradicts an existing decision record, surface it explicitly rather than silently overriding:

> _Contradicts `ARCHITECTURE.md` → "Decision record: M2: MCP server", but worth reopening because…_
