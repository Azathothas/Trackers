# TODO

<!-- TEMPLATE for the todo model. Copy to TODO/INDEX.md and delete this
     comment. Every entry, one line each, sorted by id. -->

Every entry, one line each, sorted by id. The entry itself lives in the
`{{category}}.md` its row links to, and it closes there with its own acceptance
command, actually run, with the output recorded.

⛔ **What to work on next is not here.** [`PROGRESS.md`](PROGRESS.md)'s
"Start here next session" is the work order and is the only place that carries
one. This file carries the list, the definitions, the counts, and the argument
behind the current ordering.

[`RULES.md`](RULES.md) is how this repository is worked on.

⭐ **The counts below are derived from the rows, never typed.** Closing one
entry moves several numbers at once, and a session that gets one wrong fails a
gate after the work is done and the message is written.

```bash
{{the command that re-derives the counts}}
```

```bash
{{the command that asserts them independently}}
```

---

## Priority

- **P0** breaks correctness, loses data, or takes the process down.
- **P1** a documented capability does not work, or a flag does nothing.
- **P2** worth doing; nothing is wrong without it.
- **P3** worth recording so it is not rediscovered.

## Effort

S is under a day. M is a few days. L is a week. XL is longer, and ⚠ is almost
always two entries pretending to be one.

## Status

`open`, `partial`, `blocked`, `done`. ⛔ There is no `wontfix` and no
`deferred`: a blocked entry stays open with the blocker named and what would
unblock it.

---

## The ordering, and the argument behind it

{{Why the current priorities are what they are. This is written down so a
future session can re-derive the order rather than re-argue it. When the
argument changes, rewrite it here and say in PROGRESS.md that you did.}}

---

## Counts

**{{N}} items. {{N}} open, {{N}} partial, {{N}} blocked, {{N}} done.**

| priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | {{n}} | {{n}} | {{n}} | {{n}} | {{n}} |
| P1 | {{n}} | {{n}} | {{n}} | {{n}} | {{n}} |
| P2 | {{n}} | {{n}} | {{n}} | {{n}} | {{n}} |
| P3 | {{n}} | {{n}} | {{n}} | {{n}} | {{n}} |
| **All** | {{n}} | {{n}} | {{n}} | {{n}} | {{n}} |

---

## Entries

| ID | Priority | Category | Status | Item |
| --- | --- | --- | --- | --- |
| [{{T-001}}]({{category}}.md) | {{P1}} | {{category}} | {{open}} | {{one line}} |
