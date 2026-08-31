# T-244 — browser-fingerprinted HTTP client research

Which Rust HTTP client can carry a real browser's TLS and HTTP/2 fingerprint **and** still build into
a standalone static executable?

**Short answer: `impit`** — the only one of nine candidates that static-links, because it is the only
one that is not BoringSSL. Its TLS fingerprint is excellent. Its HTTP/2 fingerprint is currently
broken, for a reason that is a small fix in a fork.

---

> ## ⚠️ This is a DRAFT. Verify before you trust it.
>
> A **first-pass investigation done under time pressure**, on **one Linux x86_64 box**, against
> **localhost only**. It is meant to *steer* an implementation, not to settle it.
>
> Revision 2 of `RESEARCH.md` **corrects four claims from revision 1**, one of which reversed a stated
> weakness of the recommended library. That is the expected error rate here — assume more remain.
>
> **Do not act on any load-bearing claim without re-running the check yourself.** Every finding ships
> with the tool that produced it, precisely so you can.
> [§14 (five reviews)](RESEARCH.md#14-appendix-five-deep-reviews) and
> [§15 (what to double-check)](RESEARCH.md#15-what-to-double-check) list the known-weak claims — read
> those **before** the recommendations.
>
> **Not tested at all:** macOS, Windows, HTTP/3, any live origin, and the recommended static binary's
> full HTTPS happy path.

---

## Findings at a glance

**Build results** — Linux x86_64, rustc 1.98.0, identical release profiles
(`strip`, `opt-level="z"`, `lto`, `codegen-units=1`, `panic="abort"`):

| Client | glibc | Size | Links C++? | **Static musl** | **Static size** |
|---|---|---|---|---|---|
| **`impit`** | ✅ | 6.67 MB | no | ✅ **`static-pie`** | **6.80 MB** |
| `wreq` 0.16.0 | ✅ | 4.65 MB | **yes — `libstdc++.so.6`** | ❌ fails | — |
| `koon` 0.8.1 | ✅ | — | yes | ❌ fails | — |

Both BoringSSL clients fail with the same cmake error: they need `x86_64-linux-musl-g++`, a musl
**C++** cross-compiler that is **not in Debian/Ubuntu apt at all**.

**Fingerprint results** — captured off the wire with [`tlsprobe`](research/tlsprobe):

| | TLS (JA4) | HTTP/2 (Akamai) |
|---|---|---|
| Real Chrome | `t13d1516h2_`**`8daaf6152771`**`_…` | `…\|15663105\|0\|`**`m,a,s,p`** |
| `impit` | ✅ `8daaf6152771` — exact match | ❌ **`m,s,a,p`** — wrong, and the same for every profile |
| `wreq` | ✅ `8daaf6152771` | ✅ **`m,a,s,p`** Chrome / **`m,p,a,s`** Firefox — correct per profile |
| `curl` | ❌ `e8f1e7e78f70` | ❌ `m,s,a,p` |

impit's HTTP/2 profile **data is correct** — `chrome_151` declares `m,a,s,p`. The *delivery* is
broken: apify's `h2` fork is `0.4.7`, reqwest resolves `0.4.19`, so cargo declines the patch with a
**warning rather than an error** and stock `h2` encodes the frame.
[Full root cause →](RESEARCH.md#7-root-cause-impits-h2-patch-silently-does-not-apply)

## The nine candidates

| Repo | ⭐ | Licence | What it actually is | Verdict |
|---|---|---|---|---|
| [`impit`](https://github.com/apify/impit) | 578 | Apache-2.0 | HTTP client, **rustls** | ✅ **Adopt** — the only static-linkable one |
| [`pingly`](https://github.com/0x676e67/pingly) | 63 | Apache-2.0 | Fingerprint analysis **server** | ✅ Adopt as a **test oracle**, not a dependency |
| `chromiumoxide` *(not in the original list)* | — | MIT/Apache-2.0 | CDP client | ✅ Adopt for `--render` |
| [`wreq`](https://github.com/0x676e67/wreq) | — | Apache-2.0 | HTTP client, BoringSSL | ⚠️ Keep behind an optional feature |
| [`koon`](https://github.com/scrape-hub/koon) | 17 | MIT | HTTP client, BoringSSL | 👀 Watch — best fingerprint research, unpublished |
| [`nokk`](https://github.com/koloss777/nokk) | 7 | MIT/Apache-2.0 | Headless engine (V8), `wreq` | 👀 Watch — honest alpha |
| [`obscura`](https://github.com/h4ckf0r0day/obscura) | 22.2k | Apache-2.0 | Headless engine (V8), `wreq` | ⚠️ External binary only — stealth is off by default |
| [`aginxbrowser`](https://github.com/yinnho/aginxbrowser) | 4 | Apache-2.0 | obscura + service wrapper | ❌ Skip — redundant, pre-release `wreq` |
| [`crw`](https://github.com/us/crw) | 739 | **AGPL-3.0** | Scraping service | ❌ Skip — licence, **and no fingerprinting at all** |
| [`phrona`](https://github.com/alvaro-co/phrona) | 1 | **AGPL-3.0** | Metasearch engine | ❌ Skip — licence, wrong domain |
| [`ParallaX`](https://github.com/yuzeguitarist/ParallaX) | 5 | **PolyForm NC** | Censorship-resistance proxy | ❌ Skip — not an HTTP client, licence forbids commercial use |

**Five of the nine are BoringSSL or depend on `wreq`**, so they inherit the problem that rules out
`wreq` itself. `crw` has no fingerprinting. `ParallaX` is a different kind of software entirely.

## How to read this

| You have | Read |
|---|---|
| **2 minutes** | This page, above. |
| **10 minutes** | [`RESEARCH.md` §0–§1](RESEARCH.md#0-what-changed-in-revision-2) (what changed, the bottom line, the honest trade) then [§15](RESEARCH.md#15-what-to-double-check). |
| **You are implementing T-244** | [§8 the cost of impit](RESEARCH.md#8-the-cost-of-impit) → [§9 forking](RESEARCH.md#9-forking-impit--the-serious-option) → [§10 CDP](RESEARCH.md#10-driving-an-already-installed-browser-over-cdp) → [§12 the plan](RESEARCH.md#12-recommended-plan-for-t-244). |
| **You are checking my work** | [§14 five reviews](RESEARCH.md#14-appendix-five-deep-reviews), then re-run the tools below. |
| **You want the tools, not the prose** | [`research/`](research/) and the quickstart below. |

## Tree

```
.
├── README.md                    ← you are here
├── RESEARCH.md                  ← THE DELIVERABLE. Findings, rankings, recommendations, 5 reviews
├── LICENSE                      ← The Unlicense (public domain)
├── LICENSE-MIT-0                ← MIT No Attribution (alternative, at your option)
└── research/
    ├── README.md                ← how to run the probes
    ├── fpsync.py                ← browser-version drift tracker + ground-truth capture (stdlib only)
    ├── ja3.py                   ← r1's throwaway script. SUPERSEDED by tlsprobe; kept for history
    ├── fixture-page.html        ← test page: 1 .torrent link, 1 magnet, 1 unrelated, 1 off-host
    ├── tlsprobe/                ← THE ORACLE: TLS + HTTP/2 fingerprint capture server (Rust)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── main.rs          ← listener, TLS termination, CLI, --expect assertions
    │       ├── tlsfp.rs         ← ClientHello parser, JA3, JA4, JA4_r, browser markers
    │       ├── h2fp.rs          ← HTTP/2 frames, Akamai fingerprint, HPACK header order
    │       └── huffman.rs       ← RFC 7541 Huffman table + decoder
    └── probes/                  ← minimal build probes behind the size/static-link measurements
        ├── t-impit/             ← impit probe. NOTE the mandatory [patch.crates-io] block
        │   ├── Cargo.toml
        │   └── src/main.rs      ← fetch a page, extract .torrent/magnet links + anchor text
        └── t-wreq/              ← wreq probe, same job. Builds on glibc; FAILS on musl
            ├── Cargo.toml
            └── src/main.rs
```

## Quickstart

```bash
# 1. Build the oracle
cd research/tlsprobe && cargo build --release

# 2. See what your client puts on the wire (TLS + HTTP/2)
./target/release/tlsprobe --port 8443 &
NO_PROXY=127.0.0.1 my-client https://127.0.0.1:8443/    # client must skip cert verification

# 3. ClientHello only — no handshake, so no cert bypass needed
./target/release/tlsprobe --raw --port 8443

# 4. Are our profiles stale? (exit 1 = drift)
python3 ../fpsync.py drift --impit-src /path/to/impit

# 5. What does a REAL installed Chrome emit? (the authoritative reference)
python3 ../fpsync.py capture --tlsprobe target/release/tlsprobe
```

## The two tools

| Tool | Language | Deps | Answers |
|---|---|---|---|
| **`research/tlsprobe/`** | Rust | 4 crates | *What does my client actually put on the wire?* |
| **`research/fpsync.py`** | Python 3.8+ | **stdlib only** | *Are my profiles current, and what does a real browser emit?* |

### `tlsprobe`

Stands up a local listener, captures the ClientHello by `peek`ing the socket (so rustls can still read
the same bytes), optionally terminates TLS with a throwaway cert, negotiates `h2` via ALPN, and reads
the client's opening flight.

```
tlsprobe [OPTIONS]
  -p, --port <N>          listen port (default 8443)
      --raw               do not terminate TLS; capture the ClientHello only
      --json              emit one JSON object per connection
      --once              exit after the first connection
      --expect-ja4 <S>    assert the JA4 string, else exit 1
      --expect-ja3 <S>    assert the JA3 hash, else exit 1
      --expect-akamai <S> assert the Akamai HTTP/2 fingerprint, else exit 1
```

Reports JA3 (both GREASE-filtered and raw), JA4, **JA4_r** (un-hashed — what you diff when hashes
disagree), the Akamai HTTP/2 fingerprint, full HPACK-decoded header order, and a checklist of browser
markers (GREASE, ECH, ALPS, cert compression, PQ key share).

Every read goes through a bounds-checked cursor, so a truncated or hostile record returns `None`
rather than panicking.

> **Two gotchas worth repeating.**
> **Assert JA4, never JA3** — JA4 sorts before hashing so it survives extension shuffling; JA3
> preserves order and will flake.
> **Capture JA4 in `--raw` mode** — for `impit`, disabling certificate verification also changes its
> `signature_algorithms`, so a JA4 read through a terminated handshake is not the shipping JA4.

### `fpsync.py`

```
fpsync.py versions     current stable versions, from vendor APIs
fpsync.py drift        compare local profiles against stable; EXIT 1 on drift
fpsync.py upstream     what the newest published wreq-util ships
fpsync.py capture      drive a real installed browser at tlsprobe → ground truth
fpsync.py report       everything at once
```

`--json` works before or after the subcommand. Sources: Google `versionhistory`, Mozilla
`product-details`, Microsoft's enterprise feed. Every fetch is individually error-trapped, so one dead
endpoint degrades that field rather than the run.

`capture` is the important one: it resolves an installed Chrome/Edge, drives it headless at
`tlsprobe`, and records what the genuine browser emits — so you can assert against the browser itself
rather than a value copied from a blog post.

## Glossary

Enough to read `RESEARCH.md` without a second tab.

| Term | Meaning |
|---|---|
| **JA3** | MD5 over TLS version, cipher list, extension list, curves, point formats — **in wire order**. Order-sensitive, so it changes when a client shuffles extensions. |
| **JA4** | Successor to JA3. **Sorts** ciphers and extensions before hashing, so it is stable across shuffling. Three parts: `a` (version, SNI, counts, ALPN), `b` (cipher hash), `c` (extension + signature-algorithm hash). |
| **JA4_r** | JA4 un-hashed. The hashes tell you *that* two clients differ; JA4_r tells you *where*. |
| `t13d1516h2` | A JA4 `a` segment: TCP, TLS 1.3, **d**omain (SNI present — `i` means an IP literal), 15 ciphers, 16 extensions, ALPN `h2`. |
| **Akamai fingerprint** | The HTTP/2 identity: `SETTINGS\|WINDOW_UPDATE\|PRIORITY\|PSEUDO_HEADER_ORDER`. |
| **`m,a,s,p`** | Pseudo-header order — `:method, :authority, :scheme, :path`. **Chrome's.** Firefox sends `m,p,a,s`; curl and stock `hyper` send `m,s,a,p`. |
| **GREASE** | RFC 8701. Deliberately invalid values browsers inject to keep servers tolerant. Their *presence* is a browser marker; their *values* are random, which is why JA3 varies per connection. |
| **ALPS** (`0x44cd`) | Application-Layer Protocol Settings. Chrome-specific; its presence is a strong Chrome signal. |
| **ECH** (`0xfe0d`) | Encrypted Client Hello. Present in current Chrome. |
| **X25519MLKEM768** (`0x11ec`) | Post-quantum hybrid key share that current Chrome sends. |
| **ML-DSA** (`0x0904/5/6`) | Post-quantum signature algorithms advertised by Chromium 150+. Their absence changes the JA4 `c` hash. |
| **HPACK** | HTTP/2 header compression. Header *names* are often Huffman-coded, which is why `tlsprobe` carries the RFC 7541 table. |
| **BoringSSL** | Google's OpenSSL fork. **C++.** Lets you order a ClientHello arbitrarily — which is why `wreq` and `koon` use it, and why they cannot static-link without a musl C++ toolchain. |
| **musl** | A libc that supports true static linking. `musl-tools` provides a **C** compiler only — no C++. |

## Methodology, and what you can trust

**What was measured**, on one Linux x86_64 box:

* Build success, binary size, and static-linking status for `impit`, `wreq`, `koon` — the three
  clients actually compiled. Verdicts for the other six are read off their `Cargo.toml` files. Sound
  inference, but **inference, not measurement**.
* TLS fingerprints, captured off the wire by `tlsprobe`, cross-checked against published JA4 values.
* HTTP/2 fingerprints, captured through a terminated handshake, one client per probe instance.
* Extension-order stability across six consecutive captures.
* Profile counts, parsed from published crate sources rather than grepped loosely.

**What was not:** macOS, Windows, HTTP/3, live origins, and the static binary's full HTTPS path. The
sandbox's MITM proxy blocks browser-shaped TLS, so all functional testing was against localhost.

**Provenance of every number:** each claim in `RESEARCH.md` names the command that produced it. The
"real Chrome" reference values in §6 came from **published documentation, not a browser I ran** —
`fpsync capture` exists to replace them, and it should be the first thing you do.

## TODO / known gaps in these tools

Neither tool is finished. Roughly in priority order.

### `tlsprobe`

- [ ] **No HTTP/3 or QUIC.** The biggest gap. `impit` forces `http3` on, so its QUIC fingerprint is
      entirely unmeasured — and since HTTP/2 turned out to be silently broken, H3 deserves suspicion.
      `pingly` already covers this and may be a better answer than building it here.
- [ ] **No golden-file mode.** `--expect-ja4` takes one string; it should read a JSON manifest of
      expected fingerprints per profile and check them in one run.
- [ ] **Single-threaded, one connection at a time.** Fine as an oracle, useless for capturing a fleet.
- [ ] **Huffman decoder does a linear table scan per bit** (257 comparisons × up to 30 bits/symbol).
      Correct and fast enough for header blocks; replace with a lookup tree before using it hot.
- [ ] **No JA4_ro** (raw original ordering). Given that `impit` shuffles extensions, JA4_ro is exactly
      what you would diff to characterise the shuffle.
- [ ] **TLS 1.2 and HTTP/1.1 paths are thin** — header order is captured for HTTP/1.1, nothing else.
- [ ] **No PSK/session-resumption handling.** A second connection carries `pre_shared_key` and yields
      a different JA4; the tool does not flag it, and this briefly confused my own measurements.
- [ ] **No tests.** The parser is bounds-checked, but fixture-based tests over recorded `.bin` hellos
      would be cheap and are missing.

### `fpsync.py`

- [ ] **`capture` was never actually run** — no browser exists in the environment it was written in.
      The no-browser path is verified; **the happy path is not.** Test this first.
- [ ] **Chromium-family only.** Firefox has no equivalent headless-CDP capture path here.
- [ ] **No Safari version source.** Apple publishes no machine-readable feed; tracked by hand.
- [ ] **Regex-based source parsing.** Version numbers are scraped with regexes over Rust source, which
      breaks the moment upstream renames something. Deliberately dumb so it has zero dependencies, but
      fragile.
- [ ] **Cannot read `koon`'s profiles** — koon is not on crates.io, so the tarball route fails; it
      would need a git checkout.
- [ ] **`drift` compares major versions only**, which is weaker than it sounds: a fingerprint can
      change *within* a Chrome major. Version equality is a loose proxy for fingerprint equality.
- [ ] **Does not generate anything.** It detects drift and captures ground truth but will not *write* a
      profile. The obvious next step is emitting a `TlsFingerprint`/`Http2Fingerprint` stub from a
      `capture` run — turning "your profile is stale" into "here is the replacement".
- [ ] **No caching or rate-limit handling** on the vendor APIs.

### The research itself

See [`RESEARCH.md` §15](RESEARCH.md#15-what-to-double-check). The highest-value follow-up is not on
either tool — it is to **check whether T-244 needs impersonation at all**: fetch the ten sites it
actually targets with plain `reqwest` and a normal User-Agent, and count the failures. If none fail,
the correct implementation is an HTML parser and nothing in this repo. One hour, and it could save the
entire fork effort.

## FAQ

**Why not just use `wreq`? It has 1.8M downloads and better fingerprints.**
It does, and it is genuinely the more mature library — including the only correct HTTP/2 fingerprint
measured here. It cannot produce a static standalone binary without you first building a musl C++
toolchain. If that constraint is negotiable, `wreq` is the better client;
[§9's escape hatch](RESEARCH.md#the-escape-hatch--do-not-pick-one-client) keeps both.

**Is `impit` "pure Rust"?**
No. It pulls `aws-lc-sys` (AWS-LC, itself BoringSSL-derived C) plus `ring`. The win is architectural:
**one** TLS stack (rustls, which the tree already has) with a swapped crypto provider, versus a second
complete TLS stack. The proof is that aws-lc-sys cross-compiles to static musl and BoringSSL does not.

**Why is `obscura` ranked below libraries with 100× fewer stars?**
Different job. It is a headless browser engine, and its TLS fingerprinting is an off-by-default
`wreq` feature — so stock obscura has none, and enabling it reintroduces BoringSSL. Star count is not
evidence about the axis that matters here.

**Can I just copy this into my project?**
Yes — see below. No attribution, no notice, no permission.

## Licence — do whatever you want, no attribution

Everything in this repository — the research prose, the tools, the probes, the fixtures — is released
into the **public domain** under [The Unlicense](LICENSE).

**You may copy, modify, publish, use, compile, sell, or distribute any of it, in whole or in part, for
any purpose, commercial or otherwise, with no attribution, no credit, no notice, and no permission
required.** Paste it into your codebase, rewrite it, ship it, sell it, claim it as your own — all
explicitly fine.

If your organisation's policy is unhappy with public-domain dedications (some are), the same content
is **alternatively available under [MIT No Attribution](LICENSE-MIT-0)**, at your option. MIT-0 is
OSI-approved and also requires no attribution.

`SPDX-License-Identifier: Unlicense OR MIT-0`

**Two scope notes:**

1. These licences cover **this repository's own content**. They say nothing about the nine surveyed
   projects, which carry their own licences (Apache-2.0, MIT, AGPL-3.0, PolyForm Noncommercial) —
   `RESEARCH.md` records each, and the AGPL and PolyForm entries are flagged as disqualifying. The
   probe manifests reference third-party crates that remain under their own terms.
2. Factual observations about third-party software — version numbers, measured build sizes, captured
   fingerprints — are not copyrightable in the first place. Use them freely regardless.

**No warranty.** This is draft research that is known to contain errors. It is provided as-is.
