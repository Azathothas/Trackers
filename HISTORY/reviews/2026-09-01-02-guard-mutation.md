# Review 2, the guard mutation

**Date:** 2026-09-01. **Standpoint:** somebody who assumes every check added
this session is theatre until it has been seen to refuse something.

**What I looked for:** a guard that cannot fail, a guard that refuses
everything, and a guard whose scope silently excludes the thing it claims to
cover. ⛔ **A guard that has never been seen to refuse is a guard nobody knows
works**, and six new checks landed in one session.

**What I did not look at:** the six checks this project already had. They were
mutation-proved when they were written and nothing this session changed their
rules, with two exceptions noted below.

**Method:** plant the defect, run the guard from a process of its own, read the
exit code unpiped, restore the tree, and confirm the tree came back.

---

## The positive cases: can it refuse

| planted | guard | exit |
| --- | --- | --- |
| an em dash in a `.py` file, so the markdown specimen exemption cannot be the reason it passes | `check-markers.py` | ✅ 1 |
| 18 markers across 6 non-blank lines, over the ceiling of 30 per 100 | `check-markers.py` | ✅ 1 |
| a literal escape byte in a document | `check-control-bytes.py` | ✅ 1 |
| a NUL byte, which cannot be tested by the same code path as the others | `check-control-bytes.py` | ✅ 1 |
| an unterminated quote in a fenced shell block | `check-docs.py` | ✅ 1 |
| an angle-bracket placeholder in a fenced shell block | `check-docs.py` | ✅ 1 |
| a banned adjective in prose | `check-docs.py` | ✅ 1 |
| a page nothing links to | `check-docs.py` | ✅ 1 |
| a 19-word sentence in two documents | `check-one-home.py` | ✅ 1 |
| a byte appended to a vendored file | `check-vendor-pin.py` | ✅ 1 |
| a token shaped like a code-host credential | `check-no-secrets.py` | ✅ 1 |

## ⛔ The negative cases, which are the half that gets skipped

A guard that refuses everything is as useless as one that refuses nothing, and
it looks identical from a passing mutation test.

| planted | guard | exit | why this case exists |
| --- | --- | --- | --- |
| the same em dash inside a markdown code span | `check-markers.py` | ✅ 0 | without this the specimen exemption is untested, and a page that bans a character could not name it |
| the same long sentence twice **in one file** | `check-one-home.py` | ✅ 0 | one home means one document, not one occurrence. A guard counting occurrences would refuse every page that repeats itself for emphasis |
| the tree with the planted files removed | all | ✅ 0 | proves the failures above were the plant and not the tree |

## The runner's own two guards

`check-gate.py` makes two claims that are not about any individual check, and
both were planted for:

| planted | expected | exit |
| --- | --- | --- |
| every check pointed at a script that does not exist, so every row is a skip | ⛔ zero passes is red whatever the skips say | ✅ 1, printing `0 passed, 0 failed, 14 skipped` and `zero checks passed. A green verdict over nothing is not a verdict.` |
| one check made unrunnable, with `--strict` | a skip is a failure under strict | ✅ 1, with the row reported as a skip rather than as a pass |

⚠ **The first attempt at the `--strict` case was contaminated and had to be
re-run**, which is worth recording because it is the shape of a mutation test
that proves nothing. Hiding an existing check to force a skip also broke
`check-citations.py`, because several documents cite that file by path. The run
exited 1 with `--strict` and would have exited 1 without it.

⭐ **Re-run in isolation:** one extra row was added to the runner pointing at a
script that does not exist and that nothing in the tree cites. Same tree, same
rows, everything else passing:

| invocation | result |
| --- | --- |
| `check-gate.py` | exit **0**, `14 passed, 0 failed, 1 skipped` |
| `check-gate.py --strict` | exit **1**, the same counts |

That is the guard and nothing else.

---

## Findings

### 1. `check-vantage-metadata.py` is exempt from `--strict`, and the exemption needs a removal condition

It exits 2 by design until health records exist. `check-gate.py` marks it
`expect_skip` so `--strict` does not fail on it.

⛔ **That is an exemption, and an exemption nobody removes is a check that
stopped checking.** The flag is commented with the condition that retires it,
in the runner and in `docs/methodology/gate.md`: the moment P2 emits a record
and the check starts exiting 0, the flag comes off. ⚠ Nothing enforces that
mechanically, which is a real gap. The cheapest fix a future session could make
is to have the runner refuse an `expect_skip` row that exits 0, so the flag
fails the build once it is wrong.

### 2. `check-no-secrets.py` holds a ceiling, and the ceiling is the same shape

Six private-tracker credentials are in the corpus and the check refuses a
seventh. **Proved in both directions**: the check passes at six and the count
is printed on the pass line so it cannot silently drift, and an eighth would
fail. ⚠ It would **not** fire if one of the six were replaced by a different
one, because the ceiling counts rather than pins. That is deliberate, the
corpus is re-fetched from upstreams that change, and pinning six specific
strings would make a routine re-capture a red build.

### 3. A scope rule cannot be proved by a comparison, and one here is proved by a fixture instead

`check-markers.py` claims to read every tracked text file rather than markdown
alone. ⚠ **A tree with no violation outside markdown produces an identical
number either way**, so the claim is invisible to any comparison. The `.py`
plant above is the fixture that proves it: the file is not markdown, it is not
in `references/`, and the guard refused it.

The same reasoning applies to the `references/` exemption, and there the
evidence is the reverse: the corpus holds 315 characters outside the five and
the check passes, which is only possible because the exemption is real.

### 4. Two of the project's existing checks were changed, so they were re-proved

`check-citations.py` and `check-todo.py` both had their link scan narrowed to
ignore code spans. ⚠ **A narrowing is exactly the change that can turn a guard
off**, so both were re-run against the tree afterwards: `check-citations.py`
still refuses a link that does not resolve, an empty directory, an unknown rule
id and a line citation past the end of a file, and `check-todo.py` still
refuses a count that disagrees with its rows, which it demonstrated during this
session when `T-107` was added.

---

## What would have made this pass fire harder

Every guard here refused what it was written to refuse and accepted the case it
must not refuse. ⚠ **What it does not establish is whether the rules are the
right rules.** A check that correctly enforces a wrong rule passes every test
in this document. Two candidates for that, both recorded rather than fixed: the
banned-vocabulary list is somebody's taste with a check behind it, and the
density ceiling of 30 was measured on three trees rather than derived.
