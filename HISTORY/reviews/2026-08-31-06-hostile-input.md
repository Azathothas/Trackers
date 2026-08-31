# Review 6 -- hostile input, in both directions

**Date:** 2026-08-31, **Standpoint:** somebody who controls one of the
upstream lists this project fetches, and wants to make it do something it
should not. Then, the direction that gets forgotten: somebody who **consumes**
what this project publishes.

**What I looked for:** a path from a byte an upstream chose to a filesystem
path, a subprocess, a parser crash, or a consumer's shell.

---

## Method

Not by reading the threat model. By feeding sixteen adversarial strings through
the **real** parser and looking at what came out, then by running the **real**
published output through RFC 3986's character set.

---

## Inbound: the parser held

| attack | result |
| --- | --- |
| path traversal `udp://../../etc/passwd:6969/announce` | rejected -- *host was only dots* |
| absolute path `udp:///etc/passwd/announce` | rejected -- *no host* |
| shell metacharacter `.../announce;rm -rf /` | rejected -- *control or whitespace* |
| newline injection | rejected |
| null byte | rejected |
| CRLF header injection | rejected |
| 5 KB line | rejected -- *longer than 2048 bytes* |
| port overflow `:99999` | rejected -- *out of range* |
| negative port `:-1` | rejected |
| scheme confusion `file:///etc/passwd` | rejected -- *unknown transport* |
| `javascript:alert(1)` | rejected -- *no scheme separator* |
| unbracketed IPv6 | rejected -- *no host* |
| unicode homoglyph host | rejected -- *not a valid hostname or IP* |
| percent-encoded traversal `%2e%2e%2f` | accepted, and **correctly**: `%2e` decodes (unreserved), `%2f` does not (reserved), so the result is a path string and not a traversal |

**No source-supplied string reaches the filesystem.** The one `os.path.join` in
the acquisition path builds its filename from `source.id` -- a registry
constant -- and never from content.

**There is no subprocess, `os.system`, `shell=True`, `eval` or `exec` anywhere
in `src/`.** RULES 5.1's "never execute upstream content, including as a shell
argument built by string interpolation" is not a rule somebody has to remember;
there is no shell layer to interpolate into.

---

## Outbound: this is where it failed

The direction the threat model does not cover: **this project publishes to
consumers, and it tells them to pipe the output.** The README recommends
plaintext that a *"dumb consumer can `curl | client`"*.

Running the emitted 1337-line file through RFC 3986's permitted character set:

```
http://opentracker.acgnx.com:6869/announce"
https://tracker.kitaujisub.site/announce.php?authkey=213|10003|j46n2q
https://tracker.kitaujisub.site/announce.php?authkey=215|10003|j46n2q
```

Three lines carrying a character **no URI may contain** -- a stray `"` that is
an HTML attribute terminator leaked by somebody's scraper, and two `|`. Both
are shell-significant in exactly the idiom the README recommends.

**They were accepted because nothing checked.** The parser rejected control
characters and whitespace and stopped there; every other character was allowed
through to the primary output, which is the compatibility-critical format for
the primary audience.

**Fixed.** `normalize.parse` now refuses a line containing anything outside RFC
3986's set, naming the offending character in the reason. 1337 -> 1334, with
three rejections returned rather than dropped, so the disappearance is
explainable (RULES 3.10).

**Rejected rather than percent-encoded, and that is the interesting call.** The
module's standing bias is that *not* normalizing is safer than normalizing,
because merging two trackers destroys data. Encoding `|` as `%7C` would have
kept two entries -- and would have changed somebody's endpoint on our guess
about what they meant. A refusal is auditable and recoverable; a wrong
encoding is a tracker we invented. Recorded as a named rule in
`normalize.RULES`, not as a silent behaviour.

**Mutation-tested:** removing the check fails five tests.

---

## What I looked for and did not find

* **A decompression bomb path.** Nothing accepts compressed responses today, so
  there is nothing to bomb. If compression is ever accepted, T-086's acceptance
  requires the test.
* **An unbounded read.** `MAX_URL_LENGTH` bounds the line; `acquire` bounds the
  body.
* **A parser that crashes rather than rejects.** Sixteen adversarial inputs,
  zero uncaught exceptions; every rejection carried a reason.
* **A rejection that vanishes.** `parse_many` returns `(accepted, rejected)`;
  a caller cannot ignore it the way a logged warning can be ignored.

## What this review did NOT establish

* **That T-086 is done.** It is not, and this review does not close it. A
  security review owes a *test per threat* and a committed write-up; this is
  sixteen probes and one fix. The threats it did not exercise are the ones
  T-086 names: decompression, and the full path from an upstream byte to every
  output file rather than to the plaintext alone.
* **That the RFC 3986 set is the right filter.** It is defensible and it is not
  what a torrent client enforces -- the one client parser in the corpus checks
  only the scheme prefix. A tracker whose passkey genuinely contains `|` is now
  dropped, and if that turns out to be real rather than an artefact, the right
  answer is a recorded decision, not a quiet loosening.
* **Anything about the JSON or CSV outputs.** They do not exist yet (T-060).
* **That upstreams are not hostile in ways I did not think of.** Sixteen
  strings is sixteen strings.
