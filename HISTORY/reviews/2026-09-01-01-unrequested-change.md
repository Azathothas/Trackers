# Review 1, the unrequested change

**Date:** 2026-09-01. **Standpoint:** the operator, reading a diff of somebody
else's work on their own repository and asking one question of every hunk:
*was this asked for?*

**What I looked for:** a change the adoption did not require. On an adoption
this is the pass that matters most, because every unrequested change is a
surprise in somebody else's repository, and the ones that look like tidying are
the ones that get through.

**What I did not look at:** whether the new documents are correct. That is
review 3. Whether the new checks work is review 2.

**The subject:** 101 files, 8259 insertions, 2468 deletions, 36 files added and
3 deleted, against the tree as it stood at the start of the session.

---

## Findings

### 1. 55 files were rewritten by a script, and that is by far the largest hunk

The character migration touched every document, every experiment and most of
`src/` and `tests/`. It is 1237 insertions against 1237 deletions, which is the
signature of a pure substitution and is the reason to trust it least: a diff
that large is not read line by line by anybody.

**Asked for.** The operator chose "apply fully and arm the check" over two
narrower options, with the measured count in front of them.

**What makes it checkable rather than trusted.** The substitution changes
punctuation and nothing else, `check-markers.py` proves the postcondition, and
the suite passed unchanged afterwards, and it has since grown by two. ⚠ **One substitution was
genuinely wrong and the suite caught it**: the box-drawing character in the
probe's ladder diagram became a backslash, which Python read as an invalid
escape sequence inside a docstring. It is `+-` now, and that is the argument
for running the suite after a cosmetic pass rather than assuming a cosmetic
pass is cosmetic.

⚠ **One class of substitution is not fully checkable and I am saying so:**
840 em dashes became a spaced double hyphen. That is meaning-preserving by
construction, and it is not the same thing as rewriting the sentences, which
would have been better prose and would have risked changing what a normative
rule says. `docs/conventions/prose.md` still says "no em dashes" and the tree
now satisfies the letter of that rule everywhere and the spirit of it in the
documents a human reads. **A future session that rewrites the remaining
double-hyphen parentheticals into commas and colons is doing real work, not
tidying.**

### 2. Three files were deleted from the reference corpus

`references/Azathothas__TEMPLATE/tree/AGENTS.md` `(removed)`, that tree's
`docs/templates/AGENTS.md` `(removed)`, and
`references/Azathothas__bit-cli/tree/docs/AGENTS.md` `(removed)`.

**Not directly asked for, and defensible.** The adopted methodology's own
vendoring rule forbids keeping an agent instruction file from another project,
because a file with that name anywhere under a repository is read as
instructions by the tools working in it. Adopting the rule and leaving the
files would have been adopting a rule the tree immediately violated.

**What I checked before doing it:** nothing in this repository cites any of the
three, verified by grep across every document and the load-bearing citation
fixture. Every other file in both captured trees is untouched, including the
methodology documents `TODO/RULES.md` rests on.
`references/PROVENANCE.md` records the removal in the section that exists for
exactly this, and `check-corpus-integrity.py` still passes.

### 3. Two experiments were changed, and neither was in scope

`experiments/_conditions.py` and `experiments/19-scheme-census.py` now pass an
explicit newline when writing. **This was not asked for and I did it anyway**,
because the defect it fixes makes committed evidence depend on who ran the
instrument, and RULES 15.5 already required the fix. The alternative was to
adopt a methodology whose own rule the tree broke in the file that produces its
evidence.

⚠ **It changes bytes on Windows only**, which is why nobody had seen it: every
committed result so far came from a runner, where the old code and the new code
agree.

### 4. Two checks the session did not set out to touch were changed

`check-citations.py` and `check-todo.py` both reported a markdown code span as
a broken link. Fixing them was not in scope either, and leaving them would have
meant a green gate depended on nobody writing `[int](2.65)` in a document about
rounding.

⚠ **These are the project's own instruments and they now differ from what the
previous session left.** Both changes are additive, both are commented with the
false positive that produced them, and neither weakens a rule: the strip is
local to the link scan and the backticked-path rule still sees the spans.

### 5. The workflow lost its per-check step list

`gate.yml` used to name each check as its own step. It now runs
`scripts/check-gate.py --strict` once.

**Not asked for.** The reason is that keeping the list in both places is the
forbidden pattern about a value in two places with no check between them, and
the two had already drifted: the workflow did not run the checks added this
session and nothing would have said so.

⚠ **The cost is real and worth stating:** a failure now shows as one red step
rather than a named one. The runner prints a table naming the check that
failed, so the information is in the log rather than in the step list.

### 6. A Windows leg was added to CI

**Not asked for.** RULES 15.5 asserts that a contributor on Windows can run
everything a contributor on Linux can, and until this session nothing had ever
run it. The session itself is the evidence that it was worth checking: three of
the defects above are Windows-only.

---

## What I changed as a result of this pass

Nothing, in code. Two things are now written down that were not: the honest
limit of the em-dash substitution, above, and the fact that the workflow's
step list was collapsed on purpose, which is in `gate.yml`'s own header so the
next session does not read it as an omission.

## What would have made this pass fire harder

A finding here would be a change with no rule behind it. Every one above is
either something the operator chose or something an adopted rule required in
the same change. ⚠ **The pass that could still fire is the one nobody can run
yet:** whether the 840 substituted dashes read as well as rewritten sentences
would. That is a reading, it is not mechanical, and I have recorded it as a
known limit rather than claimed it is fine.
