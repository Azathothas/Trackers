# Progress

<!-- TEMPLATE. Fill every {{PLACEHOLDER}} and delete this comment.
     This file is REWRITTEN every session. It carries no history: for history,
     read the git log, the entries and the handoffs.
     It is the only file the next session is told to read first, so everything
     that changes from session to session lives here and nowhere else. -->

⭐ **Read this first.** It is the only thing the kickoff prompt tells a session
to read, so everything that changes from session to session is here: the
baseline, what the last session did, and the work order.

It carries no history. Every session rewrites it.

How this repository is worked on: [`RULES.md`](RULES.md).
{{Every entry, one line each: `INDEX.md`, in the todo model only.}}
Routing for an agent: [`AGENTS.md`]({{PATH TO AGENTS.md}}).

> **The shape this file must keep.** The state line with the start instant in
> ISO 8601 UTC. The measured baseline with any CI run named by id. The counts.
> What this session did. What is in progress. **Start here next session** as an
> ordered list. Open questions for the operator.
>
> ⛔ Do not count anything by hand that a script can derive.

---

## State

- **Last session:** {{ISO 8601 UTC start instant}}, {{attended | unattended}}.
- **Tree:** {{clean | dirty, and what is uncommitted}} at {{short commit}}.
- **Deployed:** {{version, or "not deployed". Never "the latest".}}
- **CI:** {{run id and the commit it covers, or "no CI in this project"}}.

## Baseline, as measured this session

⛔ Re-measure rather than trusting the number below. It was true once.

| check | result | at the start |
| --- | --- | --- |
| {{typecheck / lint / format}} | {{pass or fail}} | {{was}} |
| {{tests}} | {{N passed, M files, against M on disk}} | {{was}} |
| {{the project's own guards}} | {{pass or fail}} | {{was}} |
| {{size, via a line counter}} | {{N}} | {{was}} |

{{Todo model only:}}
**{{N}} entries: {{N}} done, {{N}} partial, {{N}} blocked, {{N}} open.**

---

## What this session did

{{A few lines per item. What changed, and what it proved.}}

⛔ **Every premise a measurement disproved, named here**, with what was believed
and what was measured. That is the half a future session cannot re-derive.

## What is in progress

{{The item, the file, and the exact resume point. Or "nothing".}}

⛔ A half-finished change is recorded here as partial, never left silent.

---

## Start here next session

⭐ **This is the work order and it lives nowhere else.** Not in the index, which
carries the list rather than the order. Not in the kickoff prompt, which would
be a second copy going stale the moment an item closes.

1. {{item id, the file, and what to do}}
2. {{item id, the file, and what to do}}
3. {{item id, the file, and what to do}}

---

## Open questions for the operator

{{Each one: what you need decided, the options, and your recommendation.
Or "none". ⛔ Never silence: a session that had a question and did not ask it
has made the next session re-derive it.}}

---

## Settled, and not to be raised again

{{The decisions that keep getting re-opened, with the ruling and the date.
This section exists so a session does not spend its budget re-litigating
something already decided. Keep it short: a long one means the decisions are
not actually settled.}}
