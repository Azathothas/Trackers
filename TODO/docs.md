# Docs

The documentation set, and the checks that keep it from drifting.

---

### T-120 The documentation set is a fraction of what is required

Source:      the brief's section 28 (the documentation set)
Category:    docs
Priority:    P2
Effort:      L
Status:      open

Problem:     Twenty-six topics are named as the minimum and most have no page.
             What exists: `README.md`, `docs/AGENTS.md`, and -- in `HISTORY/`,
             which is the record rather than the manual -- the reference sweep,
             the decisions, the claims, the corrections, the gates, the corpus
             baseline and the reviews. **`docs/` itself holds one file.**
Premise:     Three of the hardest are already carried, and they are carried in
             the **README** rather than only in a methodology page, which is the
             requirement most easily satisfied in the wrong place: measurement
             limitations and vantage bias, announce ethics, and the
             capability classification.
Approach:    Document, at minimum: project purpose, architecture, source
             methodology, source registry, parsing, normalization,
             deduplication, validation, health states, protocol testing,
             **measurement limitations and vantage bias**, **announce ethics**
, scoring, historical reliability, categories, publication,
             GitHub Releases, raw GitHub data, the `data` branch, caching,
             source failures, issue automation, housekeeping, reproducibility
, security, **known limitations**, license.
Decision:    **Documentation must answer practical questions quickly** and be
             organised as usable manuals -- no project lore, no history dumps in
             reference pages. History belongs in `HISTORY/`, which is what that
             directory is for.
             **The README must carry the vantage limitations of RULES 3.4**, not only a deep
             methodology page, because the people most likely to misread the
             data will never open one.
Prove:       A checker asserting every named topic has a page. **The
             cross-reference half is done** -- `scripts/check-citations.py`
             (T-121) already fails on a link, path, line number, register id or
             rule section that does not resolve, so what remains here is
             coverage of the topic list, not link integrity.

---

### T-121 Nothing checks that documentation citations still resolve

Source:      RULES 2; RULES 7
Category:    docs
Priority:    P2
Effort:      S
Status:      **done**

Problem:     The documents cite files, line numbers and commits heavily --
             `torrent_miscellaneous.pas:393`, `views.py:131`, `tracker.py:163`
             and dozens more. **Nothing verifies any of them.** A citation that
             has silently stopped resolving is worse than no citation, because
             it still looks like evidence.
Premise:     The corpus is tracked in-tree at captured commits precisely so
             these are checkable rather than aspirational. That makes the
             checker cheap to write.
Approach:    A sibling of `check-todo.py` that resolves every cited path,
             asserts every cited line number exists, and fails on a dead
             internal link.
Decision:    Named `scripts/check-citations.py`, not `check-docs.py` as the
             `Prove:` clause originally said, because it checks citations
             wherever they appear -- including in `src/`, `experiments/` and the
             workflows, which carried 214 of the 374 broken ones. A checker
             scoped to `docs/` would have found almost none of the damage.
             Rejected: extending `check-todo.py`, which would have made one
             script answer two unrelated questions.

**Done.** `scripts/check-citations.py`, which found **374 broken citations on
its first run**: a reference to a retired design document (223), a markdown
link that does not resolve, a `RULES n.n` naming no section, a
`C-nn`/`T-nnn`/`Dn` with no register row, a backticked path that does not
exist, a `path:NN` past the end of the file, and a corpus figure that no
instrument produced (23). A path this project intends to create is legitimate
and must say `(planned)` on the same line, so "does not exist" and "not built
yet" cannot be confused.

**It grew to twelve checks over the session, and every addition was paid for by
a defect it then found**: bare section references; a line citation that exists
but no longer says what it is cited for; a stated test count that has drifted
twice; and a link to an **empty directory**, which resolves on the author's
disk and 404s in every clone -- the cause of six red CI runs here.

The line-number half is the one that matters, and it resolves a bare
`views.py:131` by basename against the tracked corpus -- which is only possible
because the corpus is in-tree at captured commits.

Prove:       `python3 scripts/check-citations.py` exits 0, and deliberately
             breaking one citation makes it exit 1.
Accepted:    2026-08-31. Exits 0 on the tree. Mutation-tested: changing
             `torrent_miscellaneous.pas:393` to `:99393` produces
             `line citation past end of file: torrent_miscellaneous.pas:99393
             but references/GerryFerdinandus__bittorrent-tracker-editor/tree/
             source/code/torrent_miscellaneous.pas has 777 lines` and exit 1.

---

### T-122 The consumer contract is documented but nothing enforces it

Source:      the brief's section 3 (audience and consumer contract)
Category:    docs
Priority:    P2
Effort:      S
Status:      open

Problem:     The contract is stated in the README: stable file paths, stable
             schemas versioned when a breaking change is unavoidable, plaintext
             a dumb consumer can `curl | client`, and a documented pin target.
             **Breaking any of these silently is a defect of the same class as
             publishing wrong data**, and today nothing would catch it.
Premise:     Written; unenforced. Nothing is published yet, so the cost of
             fixing this is currently zero and rises the moment it is.
Approach:    Treat the contract as an API: a test that the published path set
             and the schema field set match the documented ones exactly, and
             that a schema change without a version bump fails.
Decision:    Consumers **SHOULD** pin a branch name or a release tag and **MUST**
             be told not to pin a commit SHA on the data branch, because history
             housekeeping invalidates those by design (T-081).
Prove:       A test that fails when a documented path or field disappears.

---

### T-123 Most acceptances cannot be run as written

Source:      the brief's section 6 (an entry's `Prove` is a command); found by review 4
Category:    docs
Priority:    P2
Effort:      M
Status:      open

Problem:     The work model requires an entry's `Prove:` to be **the
             acceptance, which is a command**. Measured 2026-08-31: **46 of 63
             entries have a `Prove:` that is a paragraph**, not something a
             session can execute. `bit-cli`'s own mining guide states the cost
             in one line -- *"a 'prove' with no command is a paragraph"* -- and
             the cost lands on the next session, which has to invent the check
             before it can close anything.
Premise:     Measured, by parsing every entry for a backticked command in its
             `Prove:` field. Most are checkable *in principle*: "a test that X"
             becomes `python3 -m unittest tests.test_x`. They are not
             written so a session can run them.
Approach:    Rewrite each `Prove:` as either a **command** or a **named
             artefact** whose existence a script asserts -- the brief allows
             both, and a committed artefact is a legitimate acceptance. Where
             the command names something unbuilt, mark it `(planned)`, which
             `scripts/check-citations.py` already requires for a path that does
             not exist yet.

             Then gate it: a checker that fails an entry whose `Prove:` names
             neither a command nor an artefact path.
Decision:    **Do not bulk-rewrite all 46 by pattern.** This session fixed the
             six in the current work order by hand and stopped there, because
             inventing a specific command for unbuilt work manufactures
             precision the entry does not have -- which is the defect that
             produced corrections 15 and 17. Each remaining entry gets its
             command when a session next touches it, and the gate is what makes
             "next touches it" mean something.
Prove:       `python3 scripts/check-todo.py` fails when an entry's `Prove:`
             contains neither a backticked command nor a path, and passes on
             the tree.
