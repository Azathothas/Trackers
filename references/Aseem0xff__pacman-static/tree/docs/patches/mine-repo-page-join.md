# mine-repo.sh — page join

**Vendored file:** [`../../scripts/mine-repo.sh`](../../scripts/mine-repo.sh)
**Upstream:** `Azathothas/TEMPLATE`, `scripts/common/mine-repo.sh`, at
`6eaf4b5fbe8e3207de231f86641e95179e3bc79f`

## The change

Join paginated API responses by parsing each page as its own JSON document,
instead of concatenating the pages and recovering the array bounds by counting
`[` and `]` characters over the raw text.

## What it unblocks

The comment fetch. Every issue comment in this sweep's corpus — 202 of them on
`firasuke/mussel` alone, including the maintainer rulings cited in
`RESEARCH.md` §5 — arrived as an empty array before this change, while the run
printed `comments: ok`.

`docs/methodology/references.md` names comments as the source that only it
has: "the maintainer's ruling is nearly always in a comment". The sweep read
`comments.json`, found `[]`, and was one step from recording the trackers as
silent.

## Why it cannot be done outside the vendored tree

The joiner is an inline `node -e` program inside `fetch_list()`. It has no
seam: no flag, no environment variable, and no hook selects a different join.
The only way to change the join is to change that function.

## Mechanism

The scanner counts every `[` and `]` in the concatenated buffer, including
those inside string values. Comment bodies are markdown, so they carry
brackets in links and in pasted logs. Measured on the captured fixture: 38
bracket characters inside comment bodies, net imbalance `+2`. The depth
counter therefore never returns to zero at the array's real end, `out` stays
empty, `[]` is written, and the enclosing function returns 0.

## Second change, same edit

A join that produces `[]` from a page whose text contains `"url"` is now
reported as a failure and recorded as a gap in `PROVENANCE.md`, rather than
being written out as an empty tracker. The original defect was invisible
precisely because nothing checked this.

## Reproducing it

```sh
experiments/40-mine-repo-joiner-defect.sh    # exit 1 while the defect is present
```

Runs the upstream joiner verbatim against a committed fixture and compares it
to `json.load` over the same bytes.

## Could a future upstream release retire this patch?

**Yes.** The defect is in upstream's own file, and any change that joins pages
with a real parser fixes it independently. At the next sync, run `40-` against
the new upstream copy: if it exits 0, delete this patch and take upstream's
version.

No upstream issue number is known for it.
