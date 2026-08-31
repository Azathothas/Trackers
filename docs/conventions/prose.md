# prose.md

How documents are written here. The mechanical half is checked by
`python3 scripts/check-markers.py` and `python3 scripts/check-docs.py`; the
rest is a reading.

What a document *owes* is [`../../TODO/RULES.md`](../../TODO/RULES.md) section
17, which is normative. This page is how the writing is done.

---

## The rule

Short sentences. No em dashes. No marketing adjectives. No character outside
the five defined below. Present tense. Every claim backed by a command a
reader can run or a path a reader can open.

Write for an agent with no memory of the session that wrote the file, and for
a person looking for one fact.

---

## The five characters, and nothing else

⛔ ⭐ ⚠ ✅ ❌ and no others. Two tiers, and there is no third:

| tier | the set | where it belongs | what it means |
| --- | --- | --- | --- |
| prose markers | ⛔ ⭐ ⚠ | documents | the table below. Sparing, and they do not stack. |
| status glyphs | ✅ ❌ | machine output, result tables, checklists | passed, or failed. Nothing else. |

| marker | meaning |
| --- | --- |
| ⛔ | a rule that has already been broken here, or one whose violation is unrecoverable. A hard stop. |
| ⭐ | reach for this first. The highest-value item on the page. |
| ⚠ | a trap. It works until it does not, and the failure is quiet. |

⛔ **They do not stack.** There is no doubled or tripled marker. Escalating one
is how a vocabulary stops meaning anything: once a page has three levels of
stop, a reader has to weigh them, and weighing is what a marker exists to
prevent.

⛔ **A status glyph never carries a rule, and a marker never reports a
result.** With no glyph available an author reaches for the stop marker to mean
"this one failed", which is exactly the dilution the three-marker rule exists
to prevent.

⚠ **The list is five characters, not a principle, and that is deliberate.** The
tempting version is "allow symbols, forbid anthropomorphic ones", which is
right and unenforceable: no check can decide what is anthropomorphic, so the
boundary moves every time somebody argues for one more glyph. An explicit
allowlist is something a check can hold.

### The density ceiling, and why there is one

⭐ **30 markers per 100 non-blank lines**, enforced by
[`../../scripts/check-markers.py`](../../scripts/check-markers.py). The rule
above was unenforceable for as long as it was a sentence: keeping to the
allowlist was treated as compliance and nothing said how many.

⚠ **The ceiling is a long way above good practice on purpose.** It refuses the
unreadable rather than setting a target. A page at 25 is already dense.

### What the allowlist covers

⛔ **Every tracked text file this project owns, not markdown alone.** A rule
that only looks at documents leaves every script it ships unchecked, which is
where a share of this repository's own violations were: on 2026-08-31 the tree
carried **1655 characters outside the five across 55 files**, 840 of them em
dashes, and **14 of those files were under `src/` or `tests/`**. A
markdown-only rule would have seen none of the fourteen.

⭐ **A specimen inside a code span or a fenced block is permitted**, in
markdown only. A page that bans a character cannot otherwise show a reader
which one it means, and this page could not describe the rule.

⚠ Two exemptions beyond that, both narrow.
[`../../references/`](../../references/) is out of scope entirely: it is a
captured upstream corpus, byte-exact at the commits
[`../../references/PROVENANCE.md`](../../references/PROVENANCE.md) records, and
a check asking anybody to edit it would be asking for a corruption of the
evidence. A **leading** byte-order mark is exempt, and only a leading one; one
a merge left in the middle of a file is still a finding.

---

## Amend in place. Do not stack banners.

⛔ **When a rule changes, rewrite the rule.** Do not append a dated box under
the old text saying the text above is retired.

A document written by accretion, where the paragraph says one thing and a box
below it says the opposite, has a documented failure mode: a reader reads the
first paragraph, stops, and acts on the retired rule.

What to do instead:

1. **Rewrite the rule to what it is now.** The current text is the only text.
2. **Move the superseded wording to [`../../HISTORY/`](../../HISTORY/)**, with
   the date and why it changed. A separate file, not a box on the live page.
   [`../../HISTORY/README.md`](../../HISTORY/README.md) is that directory's own
   contract.
3. **Link to it once**, from the rule, in a sentence.

⚠ This is not licence to delete. A superseded rule is moved, never dropped, so
a future session that wonders why a rule has its shape can find out instead of
re-deriving it wrongly.

---

## One fact, one home

Every fact lives in exactly one document. If it must appear in a second place,
derive it there or have a check assert the two agree.

⛔ **This is checked.**
[`../../scripts/check-one-home.py`](../../scripts/check-one-home.py) refuses a
sentence of 12 or more words that appears in two documents. It compares
sentences, so a fact restated in different words passes here and fails a
review; that is the same split every prose rule has.

⚠ Corpus figures have one home and it is
[`../../HISTORY/corpus-baseline.md`](../../HISTORY/corpus-baseline.md). RULES
2.1 and 3.11 exist because three mutually contradictory sets were once in
circulation and none came from an instrument.

⚠ **Two exemptions.** [`../../HISTORY/`](../../HISTORY/) is exempt entirely: a
superseded page states things the live pages now state differently, which is
the point of it. [`../AGENTS.md`](../AGENTS.md) and
[`../../README.md`](../../README.md) are exempt from each other, because a
reader may be handed exactly one of them; a sentence either shares with any
other file is still refused.

---

## Never a fabricated number

⛔ **Where the real value is unknown, write a dash.** A wrong number on a
report is worse than no number, because a blank gets checked and a number gets
used.

⚠ **A measurement carries its conditions or it is not a measurement.** A rate
with no date, no machine, no sample count and no input size cannot be compared
to anything, which makes it worse than an absence: it invites a comparison that
means nothing. RULES 1.5 lists the phrases that indicate a number nobody took.

---

## What a document is not

**A document says what the thing does. It does not say what the project did.**

A fixed defect belongs on a reference page only where a reader needs it to use
the thing correctly. "The probe splits its failure vocabulary by whose fault
each failure is" is a constraint and stays. "The scrape-URL derivation used to
corrupt a path" is history and goes to
[`../../HISTORY/corrections.md`](../../HISTORY/corrections.md).

⚠ An unlinked page is not read, so it is not corrected, and that is the state
every stale document passes through on its way to being wrong.
`check-docs.py` refuses one.

---

## Banned vocabulary

Words that assert quality instead of demonstrating it. They survive review
because they feel like description, and `check-docs.py` refuses them.

⭐ **The list is inside a fenced block on purpose**, which is how the check
skips it. A page that bans a word cannot otherwise name the word, and this is
the same specimen exemption the character rule needs.

```text
seamless, blazing, effortless, robust, powerful, cutting-edge,
state-of-the-art, world-class, elegant, simply, obviously, revolutionary,
game-changing, rock-solid, bulletproof, lightning-fast, of course
```

⚠ **`simply` and `just` do the real damage.** They tell a reader who is stuck
that the thing they cannot do is easy. The second is refused only where it is
doing that job: `just as`, `just over` and `just under` are comparisons and
pass.

Replace the adjective with the measurement, or delete it. "Fast" becomes the
number and its conditions. `robust` becomes what it survives.

---

## Defensive framing is not neutral

⛔ **Describe what the code does in plain technical terms.** Do not write
up-front disclaimers arguing that something is legitimate, and do not tell a
future reader not to re-open a question.

Both backfire, in opposite directions. A defensive paragraph primes a skeptical
reader to look for the thing it denies, and a grepping agent trips on the
reassurance words themselves and spends its budget reading the matched line.

State the mechanism and its constraints. Name prior art briefly where it helps.
Stop there. The same applies to identifiers: prefer a neutral accurate name to
an evocative one.
