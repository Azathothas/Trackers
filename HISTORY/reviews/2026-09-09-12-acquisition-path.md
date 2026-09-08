# 2026-09-09-12 the acquisition path, byte by byte

*T-086. Follow every path from an upstream byte to a filesystem path, a
subprocess, a parser, or an output file, and confirm what RULES 5.1 requires
rather than assuming it.*

The path is short, which is the first finding: **five hops, no shell, no
temporary file, and no decompression.**

```
urlopen -> read(MAX+1) -> decode(utf-8, replace) -> parse_many -> Aggregate
```

---

## What is enforced by construction

⭐ **The strongest guarantees here cannot be tested away, because there is
nothing to break.**

* **No shell layer at all** (D1). "Never interpolate upstream content into a
  shell command" is not a rule somebody follows; there is no `subprocess` call
  anywhere in `src/`, so the class is unreachable.
* **No decompression.** `fetch` sends no `Accept-Encoding` and
  `urllib.request` does not decompress on its own. A decompression bomb needs
  an expansion step and there is not one, so the entry's "if compression is
  ever accepted" resolves to **it is not**. A gzip body arrives as bytes,
  bounded like any other, and parses to nothing.
* **No temporary file.** The body is a string in memory from `read` to
  `parse_many`. Nothing upstream-derived is ever written to disk before it has
  been parsed and accepted.
* **The cache path is computed before the body exists.** `read_cached` builds
  `<cache>/<source.id>.txt` from the **registry**, then opens it. A body cannot
  influence which file was opened because the file was already open.

## What is asserted now, and was not

`tests/test_acquisition_security.py`, one class per threat the entry names.

| threat | what holds it | how it could have failed |
| --- | --- | --- |
| path traversal | every registry id is a bare path component | an id of `../x` would escape the cache directory, and nothing checked |
| oversized response | `read(MAX+1)` then reject | `read()` then a length check reads it all into memory first, which is the exhaustion the bound exists to prevent |
| control characters | `parse` refuses, and says which class | stripping them would publish a URL nobody wrote |
| decompression bomb | no expansion step exists | accepting `Accept-Encoding: gzip` later would create one, and this test is what would fail |

⚠ **Two of those tests read the source of the function they test**, which is
unusual here and deliberate: `read(MAX + 1)` versus `read()` is invisible in
behaviour on any input small enough to run in a suite, and the ordering of the
path computation against the read is a structural property rather than an
observable one. A behavioural test would have to allocate 8 MiB to see the
difference, and would still not see the ordering.

## What the review found that the tests do not cover

⚠ **`errors="replace"` on the decode is a silent transformation**, and it is
the right one, but it is worth naming. A body that is not UTF-8 becomes text
with `U+FFFD` in it rather than an error, and those lines then fail to parse
and are recorded as rejections with a reason. The alternative -- failing the
whole fetch on one bad byte -- would let a single mojibake line take out a
source. **The rejection record is what makes it safe**: nothing disappears
silently, it disappears with a reason a maintainer can read.

⚠ **`MAX_RESPONSE_BYTES` is 8 MiB against a largest observed source of about
40 KB**, which is a 200x margin. That is generous, and generous is the correct
direction for a bound whose job is to stop exhaustion rather than to police
size: a source that legitimately doubles must not be refused. The volume-swing
check (T-102) is the one that should notice a source growing 200x, and it is
still provisional.

⭐ **The `except Exception` in `fetch` is deliberately broad and is the single
most important line in the module.** It is what makes a transport failure
`FAILED` rather than an empty list, which is the defect **both** pieces of
prior art ship. A narrower clause would let an unanticipated exception escape
and, in a caller with its own broad handler, become an empty source.

## What was not reviewed

* `scripts/fetch-reference-comments.py`, which writes files from GitHub API
  responses. It is a research tool that never runs in the pipeline and never
  touches the published dataset, so it is out of this review's path -- but it
  **does** write paths derived from remote data, and that deserves its own
  pass rather than a sentence here.
* The probe's handling of tracker responses, which is a different hostile
  input on a different path. `bencode.py` and `bep15.py` have their own bounds
  and their own tests; this review followed the **acquisition** path only.
* Whether a malicious upstream could make the output *useful to itself* -- for
  instance by flooding the corpus with its own trackers to raise their share.
  That is a data-integrity question rather than a memory-safety one, and
  nothing here addresses it.
