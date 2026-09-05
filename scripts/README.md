# scripts

The checks, the generator, and the two helpers.

⭐ **One command runs the whole local gate**, and it is the one to reach for
rather than typing a list from memory:

```bash
python3 scripts/check-gate.py
```

⛔ **Everything here runs with NO NETWORK and needs nothing installed beyond
Python 3.11.** That is the selection rule. A gate that has to fetch something
is red whenever somebody else's host is down, and a check fetched at gate time
is code nobody reviewed judging the tree it is judging. The two scripts that do
touch the network say so in their own headers and never run in CI.

⛔ **Python, not shell.** RULES 15.5: a `.sh` that a gate depends on is a
platform requirement in disguise. Every check here is one implementation that
runs identically on Linux, macOS and Windows, which is also why there is no
second implementation to drift from.

---

## The check contract

⛔ **Every check here satisfies all five.** A script that does not is not a
check; it is a script somebody has to remember to interpret.

1. **A header saying what defect it exists to catch.** Not what it does: what
   goes wrong without it. ⭐ This is the field that decides whether a future
   session keeps it, deletes it, or writes a second one that overlaps.
2. **Exit 0 pass, 1 fail, 2 could not run.** ⚠ Those are three different facts.
   "The check failed" and "the check could not run" mean opposite things about
   whether you can ship.
3. **A `--json` switch**, so the gate runner can consume it.
4. **No dependence on the directory it is run from.** Paths resolve from the
   script's own location.
5. **Read only.** A check that repairs things by default is a check nobody can
   use to find out whether something is wrong.

⚠ **A check that measures an open defect must not fail the build for that
defect alone.** Record the count and judge it only past a stated ceiling.
⭐ **The other half of that rule is that the ceiling comes off when the entry
closes**, and every ceiling here names its entry. An exemption nobody removes
is a check that stopped checking.

⛔ **An exit code is read from the process that produced it, unpiped.** Not
`check | grep`, not `check | tee`. A pipeline reports the last command's
status, so a check that failed reads green.

---

## What is here

### The record and the evidence

| script | what defect it catches |
| --- | --- |
| [`check-todo.py`](check-todo.py) | the work record disagreeing with itself. Every count is re-derived from the rows, so closing an entry cannot leave a stale number behind |
| ⭐ [`check-citations.py`](check-citations.py) | a promise the reader cannot follow. Paths, relative links, `RULES n.n`, `C-nn`, `T-nnn`, `D-n`, **line numbers into the corpus**, retired figures, stated test counts, and cited directories that git cannot track. Its first run found 374 broken citations in a tree several reviews had already read |
| [`check-corpus-integrity.py`](check-corpus-integrity.py) | evidence quietly missing from every clone. It counts the disk against the index, because two ignore rules once dropped 111 corpus files with no word in `git status` |
| [`check-decision-record.py`](check-decision-record.py) | a decision recorded without the alternatives it rejected, which is a preference wearing a decision's name |

### The code

| script | what defect it catches |
| --- | --- |
| [`check-no-third-party-imports.py`](check-no-third-party-imports.py) | decision D1 quietly becoming false. It parses with `ast` rather than grepping, because a grep for `import` matches prose about importing things |
| [`check-vantage-metadata.py`](check-vantage-metadata.py) | a health record that does not say where it was measured from. Takes `--path` so it can read a sweep written into scratch. ⚠ **Exits 2 while no record exists, and that is correct**: returning 0 would report "every record carries its vantage" while checking nothing. The gate carries it as an expected skip until a sanctioned vantage has probed the corpus ([T-024](../TODO/measurement.md)) |
| [`check-vendor-pin.py`](check-vendor-pin.py) | a vendored file that quietly stopped matching its pin, in either direction |

### The documents

| script | what defect it catches |
| --- | --- |
| [`check-docs.py`](check-docs.py) | a fenced shell block nobody can paste, an angle-bracket placeholder a shell reads as a redirect, vocabulary that asserts quality instead of demonstrating it, and a page nothing links to |
| [`check-markers.py`](check-markers.py) | prose that reads as machine output. The five-character allowlist and the density ceiling, over **every** tracked text file rather than markdown alone |
| [`check-control-bytes.py`](check-control-bytes.py) | a byte that makes a file invisible to both review tools at once |
| [`check-one-home.py`](check-one-home.py) | the same sentence in two documents, which is where drift starts |
| [`check-no-secrets.py`](check-no-secrets.py) | anything in the tree that must not be published, including a private tracker's passkey |

⛔ **One rule, one enforcer.** Link resolution is `check-citations.py`'s and
nothing else looks at links. The character allowlist is `check-markers.py`'s.
Control bytes are `check-control-bytes.py`'s. Two checks holding one rule is
two places for it to be wrong, and they will be wrong differently.

### The runner

[`check-gate.py`](check-gate.py) delegates and holds no rules of its own.

- ⛔ **A skipped check is reported as a skip, never as a pass.**
- ⛔ **Zero passes is red whatever the skips say.**
- `--strict` makes a skip a failure, which is what CI should pass.
- `--fast` drops the test suite and the offline generation. ⚠ It is for an
  edit-and-recheck loop, never for a verdict.

⚠ **It writes nothing into the tree.** The census and the generator both take
an output path, and the runner points them at scratch, because a gate that
dirties the working tree makes RULES 10.3 step 6 unsatisfiable.

### Not checks

| script | |
| --- | --- |
| [`generate.py`](generate.py) | builds the dataset. `--offline` runs the whole pipeline against the pinned fixtures, which is what makes the gate reproducible on any host |
| ⛔ [`probe-corpus.py`](probe-corpus.py) | **the one script that opens sockets to other people's servers.** BEP 34 is consulted per host before any probe and there is no flag that skips it; the run is bounded by a concurrency limit, one connection per host, a per-attempt timeout and a whole-run deadline. `ci` probes a sample, not the corpus. It has **no offline mode**: a run that opened no socket could still emit a record per tracker saying `unknown`, and that file would satisfy `check-vantage-metadata.py` while nothing had been measured |
| [`fetch-reference-comments.py`](fetch-reference-comments.py) | ⚠ **touches the network.** Corpus building, never a pipeline step, never in CI |
| [`vendor/toolkit/`](vendor/toolkit/) | the probe and the commit-and-push helper, pinned. Not this project's code |
| [`_scope.py`](_scope.py) | shared file scoping. Where the `references/` exemption is defined, and why |

---

## Adding one

1. **Name the defect first.** If you cannot say what goes wrong without this
   script, it is not a check.
2. **Follow the contract**, all five points.
3. ⭐ **Mutation-prove it.** Plant the defect it exists to catch, run it, and
   read the exit code unpiped. **A guard that has never been seen to refuse is
   a guard nobody knows works.**

   ⛔ **And prove the negative case.** A guard that refuses everything is as
   useless as one that refuses nothing, and it looks identical from a passing
   mutation test.

4. **Wire it into [`check-gate.py`](check-gate.py)**, and into
   [`../.github/workflows/gate.yml`](../.github/workflows/gate.yml).
5. **Document it**: here, and in
   [`../docs/agent-tooling.md`](../docs/agent-tooling.md).

⚠ **A script that lives only in a transcript is re-derived every session.**
Where a scratch helper does something a future session will also need, promote
it.
