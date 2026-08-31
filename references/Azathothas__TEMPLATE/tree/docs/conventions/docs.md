# docs.md

The document set, what each one owns, and the rules that keep it trustworthy.

[`prose.md`](prose.md) is how they are written. This is which ones exist and
what makes them true.

---

## The set

Adapt the names. Keep the roles. Create what the project has a use for and
nothing else: a file nobody selected is a file a future session reads,
believes, and follows into a rule that was never meant to apply.

| file | owns |
| --- | --- |
| `AGENTS.md` | ⭐ the router. What to read for which task. Restates nothing, links everything. |
| `README.md` | what this is, for a competent stranger. What, why, how to start, where the docs are. |
| the record | ⭐ the one file every session reads first. The baseline, what the last session did, and the work order. Nothing else carries a work order. |
| `RULES.md` | how this repository is worked on, rule by rule, with what each cost to learn |
| `HUMAN.md` | the operator's side: machine setup, validation, the runbooks, the division of labour, the prompts they paste |
| `SECURITY.md` | the threat model, who holds what, the blast radius of each leak. Writing it is the audit. |
| `CHANGELOG.md` | what shipped, when, and where the evidence is |
| `docs/architecture.md` | ⭐ the technical reference. Schema, state machines, algorithms, limits. When any document conflicts with this one, this one wins and the other is the defect. |
| `docs/code-map.md` | where things live and why. The layer rule and what enforces it. |
| `docs/limits.md` | what is true and not going to change, and why |
| `docs/lessons.md` | what was learned, tagged, with the source |
| the work plans | one per unit, plus the template they are authored from |
| the handoffs | in stage mode. The durable memory between sessions. |
| ⭐ `docs/history/` | **the story.** Superseded wording, reversed decisions, dead ends, reference sweeps, review passes. ⛔ Everything above says what is true now; this says what was believed and why that changed. [`../methodology/history.md`](../methodology/history.md). |
| `RESUME.md` | ⚠ written at the START of a session and refreshed as work moves, so a session that dies mid-task still hands something over. Overwritten every session, never appended to. [`../methodology/sessions.md`](../methodology/sessions.md). |

⛔ **`docs/history/` exists so that none of the rows above fill up with
narrative.** An agent working from this template wrote its project's history
into every document it touched; nothing was untrue and the result was
unreadable. ⚠ The instinct was right, which is why forbidding it did not work:
a superseded explanation is worth keeping for the reason
[`prose.md`](prose.md) gives. It needed a destination, and the rule that told it to move the wording
never named one.

---

## The invariants

### One fact, one home

Every fact lives in exactly one document. A version string, a constant, a rate
limit, a schema: one place.

⛔ **A value in two documents with no check between them drifts**, and the copy
a reader trusts is the wrong one. If a number must appear twice, derive it from
the source, or have a check assert the two agree.

⚠ The trap is that a value which never changes cannot expose a missing check.
It sits correct for a year and drifts the first time it moves.

### The technical reference wins

When any document conflicts with `architecture.md`, the reference is right and
the other document is the defect. Fix it in the same change and note the
conflict in the handoff.

### Documentation ships with the code it describes

⛔ Doc and code drifting apart is a forbidden pattern. The moment code changes a
documented behaviour, the document changes with it. In the same commit, not
later.

### Every claim is verified before it is written

Writing the documentation is the audit. Being forced to say precisely what
something does, and then checking whether that is true, is where a surprising
share of real defects are found.

⚠ The most confident sentence in a file is regularly the only false one. A test
file header asserting it ran "exactly as production uses it" hid the gap that
shipped a server error for six units of work.

### Prefer a shape a check can assert

Where a document names a file, a constant, a route or an identifier, prefer a
form a check can verify against the tree, so a rename fails a gate instead of
rotting quietly.

⭐ The strongest version of this is a catalogue where each entry declares which
files read it, and a check opens those files and looks. That is a document that
reviews itself.

⚠ **A document that cannot be checked is a document that drifts.** That is not
an argument against writing prose. It is an argument for making the mechanical
parts mechanical, so the reading is spent on the parts that need it.

### Say what is not true

Reserve a place for the truths that are tempting to hide. This is slower than
it looks. This has a known gap. This estimate excludes something unmeasurable.

⛔ A limit hidden is a defect filed against the user later.

---

## Lessons, and how they are tagged

`docs/lessons.md` is the running log of what worked and what bit. Every entry
carries its source and one tag:

| tag | meaning |
| --- | --- |
| `adopt` | do this. It was measured or it was paid for. |
| `avoid` | rejected, with the reason, so nobody re-derives it |
| `future` | a good idea, not now, with what would make it now |
| `honest-limit` | a truth to keep documented where a user will see it |

⭐ It is the institutional memory that stops the project re-learning the same
lesson. Seed it from any prior art studied; append after every review that
surfaces something.

⚠ A lesson that is grep-able belongs in
[`forbidden-patterns.md`](forbidden-patterns.md) as well, and a lesson that is
mechanical belongs in a check instead.

---

## The changelog

**What shipped, when, and where the evidence is.** One entry per shipped unit
of work, pointing at the record that carries the detail.

⭐ It is also the destination for what a documentation pass removes. When a
document loses the *story* of a fix, what broke and what the sentence used to
say, the story comes here. So this file is expected to grow, and its length is
not a defect.

| the text is | where it goes |
| --- | --- |
| a fact, limit or constraint a future session needs | ⛔ the document. Not here. |
| a measurement with its conditions | ⛔ the document, as a table. Not here. |
| the story of a fix, or a superseded claim kept for provenance | ⭐ here |
| the full detail of one session's work | ⛔ the handoff. Here goes a pointer to it. |

Four rules, and [`scripts/common/check-changelog.sh`](../../scripts/common/check-changelog.sh)
holds all four. ⭐ They were stated here and enforced by nothing for as long as
this document existed, which is the shape a rule takes on its way to becoming
a preference:

1. ⛔ **Newest first, always.** A new entry goes at the top of its section,
   never appended to the bottom.
2. ⛔ **Every heading carries a date.** Consider a full ISO 8601 UTC stamp:
   several entries sharing one date cannot be ordered from what was written
   down.
3. ⛔ **Every entry names its record**, the handoff or plan or commit carrying
   the evidence. An entry with no record is a claim.
4. ⛔ **Every entry says whether it deployed.** "No version bump and no deploy"
   is a complete and common answer. Silence is not.

And two things an entry must not do:

- ⛔ **Do not tidy the file while shipping something else.** Reordering old
  entries in the commit that adds a new one makes both unreviewable. Tidying is
  its own commit.
- ⛔ **Do not delete an entry.** A superseded one is amended in place with a
  dated note. Amend, never silently delete.

⚠ A check can hold the order, the dates and the pointers. **It cannot check
that an entry is true.** That stays with the claim audit,
[`../methodology/reviews.md`](../methodology/reviews.md) lens 3.
