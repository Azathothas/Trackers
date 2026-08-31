# T-244 — Browser-fingerprinted HTTP client survey

> ## ⚠️ DRAFT — verify before trusting
>
> First-pass research, done under time pressure, on **one Linux x86_64 box**, against **localhost
> only**. Revision 2 already corrects **four** claims from revision 1 ([§0](#0-what-changed-in-revision-2)) —
> assume more errors remain. Read [§14 (five reviews)](#14-appendix-five-deep-reviews) and
> [§15 (what to double-check)](#15-what-to-double-check) **before** acting on any recommendation;
> they list the claims that are known to be weak. Every finding ships with the tool that produced it
> so you can re-run it rather than take my word.
>
> **Untested: macOS, Windows, HTTP/3, any live origin, and the recommended binary's full HTTPS path.**

**Prepared:** 2026-08-25 · **Revision 2** · **For:** the agent implementing T-244

> **Revision 2 corrects revision 1 on three points and adds four sections.** If you read r1, jump to
> [§0](#0-what-changed-in-revision-2) first — one of the corrections reverses a stated weakness of the
> recommended crate, and one adds a defect serious enough to change how you plan the work.

> **Scope note.** This is a steer, not a spec. Every number was measured on one Linux x86_64 box
> ([§2](#2-test-environment)); macOS and Windows claims are reasoned, **not** measured. Re-verify
> anything load-bearing.

---

## 0. What changed in revision 2

| # | r1 said | r2 measured | Severity |
|---|---|---|---|
| 1 | impit's JA4 is `t13d1515h2_…` | **`t13i1515h2_…`** — the SNI marker is `i`, not `d`. My r1 script hardcoded `d`. | Medium — the load-bearing `8daaf6152771` claim is unaffected, but do not paste the r1 string into an assertion. |
| 2 | "impit emitted a **fixed** extension order … a potential tell" | **Wrong.** Across 6 captures: **3 distinct extension orders, 3 distinct JA3 hashes, 1 stable JA4.** impit *does* shuffle. | High — r1 wrongly penalised impit and wrongly credited `koon` as "demonstrably ahead" here. |
| 3 | "seven of the nine are BoringSSL or depend on `wreq`" | **Five** of nine (`koon`, `nokk`, `obscura`, `aginxbrowser`, `phrona`). Seven are *unusable*, which is a different claim. | Medium — the table was right, the sentence over-counted. |
| 4 | H2 fingerprint "unverified — the highest-value open question" | **Now measured.** impit's Akamai fingerprint is **wrong and profile-invariant**; `wreq`'s is correct and per-profile. Root cause found: **impit's `h2` patch silently does not apply.** | **Critical** — new [§6](#6-the-http2-fingerprint--the-gap-r1-flagged-now-measured) and [§7](#7-root-cause-impits-h2-patch-silently-does-not-apply). |

New sections: [§7](#7-root-cause-impits-h2-patch-silently-does-not-apply) (the defect),
[§9](#9-forking-impit--the-serious-option) (fork-and-patch strategy),
[§10](#10-driving-an-already-installed-browser-over-cdp) (CDP), and
[§14](#14-appendix-five-deep-reviews) (five review passes).

The r1 recommendation **stands**, but it is now a trade with open eyes rather than a clean win.

---

## 1. Bottom line

**Adopt `impit` (Apache-2.0, apify) for the static tier — but budget for forking it.** It is the only
candidate of the nine that builds a fully static Linux executable, because it is the only one that is
not BoringSSL. Its TLS fingerprint is excellent. **Its HTTP/2 fingerprint is currently broken**, for a
reason that is a two-line fix in a fork ([§7](#7-root-cause-impits-h2-patch-silently-does-not-apply)).

The TODO's blocker — *"This tree already carries `rustls`. Two TLS stacks in one binary is a larger
change than any crate count says"* — is exactly right, and it eliminates most of the field:

| Repo | TLS stack | Verdict |
|---|---|---|
| `impit` | **rustls** (+ aws-lc-rs crypto provider) | ✅ the one exception |
| `koon` | BoringSSL (own `btls` fork) | same problem as `wreq` |
| `nokk` | `wreq` → BoringSSL | inherits it |
| `obscura` (`--features stealth`) | `wreq` → BoringSSL | inherits it |
| `aginxbrowser` | `wreq` → BoringSSL (pinned `=6.0.0-rc.28`) | inherits it |
| `phrona` | `wreq` → BoringSSL | inherits it |
| `crw` | plain `reqwest` + rustls | **no fingerprinting at all** |
| `pingly` | rustls (it is a *server*) | not a client — see [§11.2](#112--pingly--adopt-as-the-test-oracle) |
| `ParallaX` | own Safari-26 TLS state machine | not an HTTP client; licence poison |

**Your `wreq` dealbreaker is confirmed, and precisely located.** `wreq` builds fine on glibc. The
problem is that BoringSSL is **C++**:

* the glibc build links **`libstdc++.so.6`**, which is what breaks "standalone";
* the static musl build **fails outright**, needing `x86_64-linux-musl-g++` — a musl **C++**
  cross-compiler that `musl-tools` does not ship and that is **not in Debian/Ubuntu apt at all**.

`koon` fails identically. `impit` static-links needing only `musl-tools` (a **C** compiler; aws-lc-sys
compiles C, not C++).

### The honest trade

| | `impit` | `wreq` |
|---|---|---|
| Static standalone binary | ✅ **6.80 MB static-pie** | ❌ impossible without a musl C++ toolchain |
| TLS stacks in your binary | **1** (rustls, already present) | **2** (rustls + BoringSSL) |
| TLS/JA4 fidelity | ✅ excellent, verified | ✅ excellent |
| **HTTP/2 Akamai fidelity** | ❌ **broken** (fixable, [§7](#7-root-cause-impits-h2-patch-silently-does-not-apply)) | ✅ **correct, per-profile** |
| Profiles | 21 | 113 |
| On crates.io | ❌ git only | ✅ 1.82M downloads |
| Maintained by | Apify (funded company) | one maintainer |

If T-244's origins only read JA3/JA4 — most indexers — impit as-is is sufficient. If any of them read
the HTTP/2 fingerprint (Akamai, Cloudflare do), impit as-is is **detectably incoherent**: Chrome TLS
with non-Chrome HTTP/2 is a louder signal than a plain client, because no real browser does that.

**Recommended path: adopt impit, fix the h2 patch in a fork, verify with `tlsprobe`.** See
[§9](#9-forking-impit--the-serious-option).

---

## 2. Test environment

| | |
|---|---|
| Host | Linux 6.18 x86_64, 4 cores, Debian-family |
| Rust | `rustc 1.98.0 (88d9e12ae 2026-08-18)` — upgraded mid-test; `wreq` 0.16 demands **≥1.98** |
| Targets | `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` |
| Toolchain | cmake, ninja, clang, go, perl, `musl-gcc` (via `musl-tools`) |
| Release profile | `strip=true, opt-level="z", lto=true, codegen-units=1, panic="abort"` — identical for every candidate |

Each client was given the **actual T-244 job**: fetch a page, extract every `href` ending `.torrent`,
every `magnet:` URI, and the anchor text beside each.

> ⚠️ **Sandbox caveat.** Outbound HTTPS here goes through a MITM agent proxy, so *live* HTTPS fetches
> fail for any client presenting a real browser ClientHello. All functional testing was against
> **localhost**. Artifact of my sandbox, not a library defect — and mildly confirmatory: a vanilla
> client sails through that proxy, a browser-shaped one does not.

---

## 3. Measured build results

| Client | glibc | glibc size | Links C++? | **Static musl** | **Static size** | Build time |
|---|---|---|---|---|---|---|
| **`impit`** | ✅ | 6.67 MB | no | ✅ **`static-pie`, fully static** | **6.80 MB** | 45 s |
| `wreq` 0.16.0 | ✅ | 4.65 MB | **yes — `libstdc++.so.6`** | ❌ **fails** | — | 2 m 10 s |
| `koon` 0.8.1 | ✅ | n/m¹ | yes (BoringSSL C++) | ❌ **fails** | — | 2 m 34 s |

¹ My `koon` probe linked the crate but never called it, so the linker dead-stripped it — the 288 KB
binary is **meaningless** and is not reported as a size. What is meaningful: `koon-core` **compiled**
on glibc and **failed** on musl.

**The failure, identical for `wreq` and `koon`:**

```
The CMAKE_CXX_COMPILER:  x86_64-linux-musl-g++
is not a full path and was not found in the PATH.
thread 'main' panicked at cmake-0.1.58/src/lib.rs:1132:
```

`apt-cache search musl` returns four packages — `musl`, `musl-dev`, `musl-tools`, and an unrelated
Perl module. **No musl C++ toolchain in apt.** To static-link `wreq` or `koon` you must first supply
one: build `musl-cross-make`, adopt `cargo-zigbuild`, or move releases into Alpine (Alpine's native
`g++` *is* musl-based — which is why upstream `pingly` publishes an Alpine image).

**impit verified end-to-end:**

```
$ t-impit http://127.0.0.1:8099/index.html
status=200 OK bytes=466
/files/ubuntu-24.04.torrent                        Ubuntu 24.04 LTS Desktop amd64
magnet:?xt=urn:btih:abc123def456&dn=Ubuntu+24.04   Ubuntu 24.04 (magnet)
https://example.org/other.torrent                  Debian 13 netinst
matches=3
```

Your acceptance criterion's first half, passing, from a 6.8 MB static binary.

---

## 4. TLS fingerprint — verified against captured bytes

I wrote **`tlsprobe`** ([§13](#13-tlsprobe--the-oracle)), a Rust listener that captures the
ClientHello and computes JA3/JA4/JA4_r from the wire bytes.

| | `impit` (`chrome_151`) | `curl` (OpenSSL) |
|---|---|---|
| **JA4** | `t13i1515h2_`**`8daaf6152771`**`_806a8c22fdea` | `t13i3111h2_e8f1e7e78f70_b26ce05bbdd6` |
| Cipher suites | 15 + **1 GREASE** | 31, **no GREASE** |
| Key share | **`X25519MLKEM768`** (`0x11ec`) + GREASE | classical only |
| Signature algs | **ML-DSA** (`0x0904/5/6`) present | absent |
| ECH (`0xfe0d`) | ✅ | ❌ |
| `compress_certificate` (`0x1b`) | ✅ | ❌ |
| ALPS (`0x44cd`) | ✅ | ❌ |

**`8daaf6152771` is the published Chrome/Chromium JA4 cipher hash** — confirmed against public JA4
documentation, not my inference. impit reproduces Chrome's cipher list *exactly* and carries every
marker separating a current Chrome from a generic TLS client, including the ML-DSA signature
algorithms that `koon`'s changelog calls out as necessary for a byte-correct JA4 on Chromium 150+.

### Extension order — r1 was wrong

Six consecutive impit captures:

| | value |
|---|---|
| distinct **JA4** | **1** |
| distinct **JA3** | **3** |
| distinct extension orders | **3** |

impit **shuffles** its leading extensions (`0x44cd`, `0x000b`, `0xff01` permute; positions 4–15 are
fixed). Real Chrome shuffles more broadly, so this is partial — but it is emphatically *not* the
static order r1 claimed, and `koon` is not "demonstrably ahead" here.

> **Operational consequence, and it matters for your CI:** **assert on JA4, never on JA3.** JA4 sorts
> ciphers and extensions before hashing, so it is stable across the shuffle; JA3 preserves order, so a
> JA3 assertion against impit **will flake**. `tlsprobe --expect-ja4` exists for this reason.

### A footgun: `with_ignore_tls_errors(true)` degrades the fingerprint

Measured, same profile, same binary, one flag apart:

| builder | signature_algorithms | JA4 |
|---|---|---|
| default | `0904,0905,0906,0403,0804,…` (**ML-DSA**, Chrome's list) | `t13i1515h2_8daaf6152771_`**`806a8c22fdea`** |
| `.with_ignore_tls_errors(true)` | `0201,0203,0401,0403,…` (generic rustls) | `t13i1515h2_8daaf6152771_`**`f246ce76b0ba`** |

Turning off certificate verification **silently reverts `signature_algorithms` to rustls defaults**
and changes the JA4. Anyone who sets this for testing, or to get through a corporate MITM proxy, loses
part of the impersonation without warning. **Do not ship it enabled**, and if you set it in tests,
know that your test is no longer measuring the shipping fingerprint.

### Profile coverage — where impit is genuinely weakest

| Library | Profiles | Newest Chrome |
|---|---|---|
| `koon` | **268** | Chrome 152 |
| `wreq-util` 0.2.0 | **101** | Chrome 149 |
| **`impit`** | **21** ⚠️ | Chrome 151 |

impit ships 13 Chrome, 4 Firefox, 3 OkHttp, 1 iOS Safari. Thin, but for T-244 arguably sufficient —
you need one current credible Chrome, not a catalogue. No Edge, no desktop Safari.

*(The `wreq-util` figure is 101, not the 113 r1 reported: r1 grepped loosely and double-counted;
`fpsync.py upstream` parses the published crate source and gives chrome 41, firefox 17, edge 19,
opera 16, safari 5, okhttp 3.)*

### Staleness — measured, not assumed

`fpsync.py drift` against live vendor APIs on 2026-08-25:

| | impit ships | current stable | gap |
|---|---|---|---|
| Chrome | **151** | 151 (Linux) / **152** (Win, Mac) | ✅ current on Linux, 1 behind elsewhere |
| Firefox | **144** | **154** | ❌ **10 majors behind** |

impit's Chrome profile is genuinely current — that is the one that matters for T-244. **Its Firefox
profile is badly stale**, and a Firefox 144 fingerprint in late 2026 is a *worse* signal than no
impersonation, because it identifies a browser essentially nobody is still running. If you ever select
the Firefox profile, port a current one first ([§9.3](#9-forking-impit--the-serious-option));
`wreq-util` ships Firefox 151.

---

## 5. Reproduction of the TLS result

```bash
cd research/tlsprobe && cargo build --release
./target/release/tlsprobe --raw --port 8443 &
NO_PROXY=127.0.0.1 your-client https://127.0.0.1:8443/
```

---

## 6. The HTTP/2 fingerprint — the gap r1 flagged, now measured

r1 called this "the highest-value open question." It is now answered, and the answer is bad for impit.

`tlsprobe` terminates TLS with a throwaway cert, negotiates `h2` via ALPN, and reads the client's
opening flight: SETTINGS, WINDOW_UPDATE, PRIORITY, and the HPACK-decoded header order. That yields the
**Akamai fingerprint** — `SETTINGS|WINDOW_UPDATE|PRIORITY|PSEUDO_HEADER_ORDER`.

One client per probe instance, one connection each:

| Client | Akamai HTTP/2 fingerprint |
|---|---|
| **Real Chrome** (published) | `1:65536;2:0;3:1000;4:6291456;5:16384;6:262144\|15663105\|0\|`**`m,a,s,p`** |
| **`wreq` Chrome136** | `1:65536;2:0;4:6291456;6:262144\|15663105\|1:1:0:219\|`**`m,a,s,p`** ✅ |
| **`wreq` Firefox139** | `1:65536;2:0;4:131072;5:16384\|12517377\|3:0:0:21\|`**`m,p,a,s`** ✅ |
| **`impit` chrome_151** | `2:0;4:6291456;5:16384;6:262144\|15663105\|0\|`**`m,s,a,p`** ❌ |
| **`impit` firefox_144** | `2:0;4:131072;5:16384;6:16384\|12451842\|0\|`**`m,s,a,p`** ❌ |
| `curl` | `3:100;4:10485760;2:0\|1048510465\|0\|`**`m,s,a,p`** |

**Read the last column.** `wreq` emits Chrome's `m,a,s,p` for its Chrome profile and Firefox's
`m,p,a,s` for its Firefox profile — the orders really do differ per browser, and `wreq` tracks that.
**impit emits `m,s,a,p` for both** — which is neither, and is exactly what `curl` and stock `hyper`
emit. impit's pseudo-header order is **profile-invariant and wrong**.

impit is not entirely untuned: `4:6291456` and `|15663105|` are Chrome's real values, correctly
carried per profile. So someone did the work. It is the **delivery** that fails —
[§7](#7-root-cause-impits-h2-patch-silently-does-not-apply).

Other divergences in impit's H2:

* **Missing `1:65536`** (`SETTINGS_HEADER_TABLE_SIZE`) — Chrome always sends it.
* **Extra `5:16384`** (`MAX_FRAME_SIZE`) — Chrome sends it too, so this one is fine.
* **No PRIORITY** — Chrome sends `1:1:0:219` on the HEADERS frame; `wreq` reproduces that, impit sends none.
* impit's Firefox window is `12451842`; the real Firefox value (which `wreq` emits) is `12517377`.

**Header order after the pseudo-headers is correct** in impit — `sec-ch-ua`, `sec-ch-ua-mobile`,
`sec-ch-ua-platform`, `upgrade-insecure-requests`, `user-agent`, `accept`, `sec-fetch-*`,
`accept-encoding`, `accept-language`, `priority`. That is Chrome's order, and it satisfies the
"header set" half of your acceptance criterion.

---

## 7. Root cause: impit's `h2` patch silently does not apply

This is the most important technical finding in the survey, and it is a **live bug in impit**.

The mechanism impit intends:

1. Every fingerprint declares an order. `chrome_151` declares, correctly:
   ```rust
   pseudo_header_order: vec![":method", ":authority", ":scheme", ":path", ":protocol", ":status"]
   ```
   That is Chrome's `m,a,s,p`. **The data is right.**
2. `Impit::new()` writes it to a process-global env var:
   ```rust
   std::env::set_var("IMPIT_H2_PSEUDOHEADERS_ORDER", pseudo_headers_order.join(","));
   ```
3. apify's **forked `h2`** reads it when encoding a HEADERS frame:
   ```rust
   // apify/h2 @7f393a7, src/frame/headers.rs:106
   std::env::var("IMPIT_H2_PSEUDOHEADERS_ORDER").unwrap_or(PSEUDOHEADERS.join(","))
   ```

**Step 3 never runs.** The fork is `h2 0.4.7`; `reqwest 0.13.4` resolves `h2 0.4.19` from crates.io.
Cargo's `[patch.crates-io]` only substitutes when the patch version *satisfies the requirement*, and
`0.4.7 < 0.4.19` does not. Cargo says so — as a **warning**, not an error:

```
warning: patch `h2 v0.4.7 (https://github.com/apify/h2?rev=7f393a72…)` was not used in the crate graph
```

and the lockfile shows both, with the registry copy the one actually linked:

```
name = "h2"  version = "0.4.19"  source = "registry+https://github.com/rust-lang/crates.io-index"
name = "h2"  version = "0.4.7"   source = "git+https://github.com/apify/h2?rev=7f393a72…"
```

So stock `h2` encodes the frame with its fixed `:method,:scheme,:authority,:path` — the `m,s,a,p` on
the wire. **Confirmed empirically:** setting `IMPIT_H2_PSEUDOHEADERS_ORDER` externally to Chrome's
order changes nothing, because nothing reads it.

### Why this matters beyond the fingerprint

* **It fails open and silent.** The library keeps working, tests pass, only the impersonation quietly
  degrades. Nobody notices without a wire capture.
* **The env-var design is unsound anyway.** `std::env::set_var` is `unsafe` in Rust 2024 and is UB if
  another thread reads the environment concurrently — which, in a `tokio` multi-thread runtime doing
  HTTP, it will. It is also **process-global**: two `Impit` clients with different fingerprints in one
  process would fight, last-constructed-wins. Even repaired, this design cannot support per-client
  profiles. A fork should replace it, not just rebase it ([§9](#9-forking-impit--the-serious-option)).
* **It is cheap to detect.** `tlsprobe --expect-akamai` catches it in CI in one connection.

**Verify this yourself before building on my word** — check whether the warning appears in your build
and whether the wire shows `m,a,s,p`. It is possible apify fix it between my read and yours.

---

## 8. The cost of `impit`

**8.1 — Not on crates.io in any usable form.** The `impit` crate there is `0.1.0`, published
**2025-01-10**, MIT, 1,332 downloads — a stale unrelated placeholder. The real project is Apache-2.0
and far ahead. **Use a git dependency pinned to a rev.**

**8.2 — Requires four `[patch.crates-io]` forks in *your workspace root*.** Verified: without them,

```
error: failed to select a version for `rustls`.
package `impit` depends on `rustls` with feature `impit` but `rustls` does not have that feature.
```

The apify `rustls` fork adds `impit = ["aws_lc_rs", "ring", "brotli", "zlib"]`, absent upstream. Your
root manifest needs:

```toml
[patch.crates-io]
h2         = { git = "https://github.com/apify/h2",         rev = "7f393a728a8db07cabb1b78d2094772b33943b9a" }
rustls     = { git = "https://github.com/apify/rustls",     rev = "23b2c17427c095b768e22ccf0dadb97266860cf1" }
tower-http = { git = "https://github.com/apify/tower-http", rev = "f9efc0d9193e774d33aedc1022b922efefc22052" }
hyper-util = { git = "https://github.com/apify/hyper-util", rev = "9b7795dfd7158fc55e7c84b65bf1dae1d2dea67d" }
```

`[patch]` applies only from a workspace root, so this is contagious: it replaces `rustls` for **the
entire bit-cli tree**, including whatever already uses it for tracker HTTPS. Mitigating: the fork is
`rustls 0.23.43`, upstream's current release — **rebased, not stale**, and the delta is a feature
declaration, not a rewrite. The `h2` line is the one that **does not currently take effect**
([§7](#7-root-cause-impits-h2-patch-silently-does-not-apply)).

**8.3 — Forces `RUSTFLAGS='--cfg reqwest_unstable'` tree-wide.** impit has **no `[features]`
section**, so reqwest's unstable `http3` is unconditional:

```
error: The `http3` feature is unstable, and requires the
       `RUSTFLAGS='--cfg reqwest_unstable'` environment variable to be set.
```

Global, invasive, cache-busting, and `cargo install bit-cli` needs it too. Put it in
`.cargo/config.toml` `[build] rustflags` so it is not a documentation burden.

**8.4 — "One TLS stack" is true; "no C" is not.** impit pulls **`aws-lc-sys` 0.44.0** — AWS-LC, itself
BoringSSL-derived C — plus `ring`; `cc` and `cmake` are both in the graph. The difference from `wreq`
is *architecture*, not C-freedom:

* **`wreq`/`koon`** add a **second complete TLS stack** — own handshake state machine, own socket layer.
* **`impit`** keeps **one** stack (`rustls`, already yours) and swaps the *crypto provider* underneath.

The proof is the build table: aws-lc-sys cross-compiled to static musl with only `musl-tools`;
BoringSSL could not be made to at all.

**8.5 — A risk that turns out not to apply.** `obscura`'s source warns that mixing `aws-lc-rs` with
reqwest's `ring` makes rustls' `CryptoProvider` auto-selection **panic at runtime**. impit enables
both, so I checked: it constructs its provider **explicitly** (`CryptoProvider::builder()` in
`src/tls/mod.rs`, cached in a `OnceLock`), not via process-global `install_default()`. Safe — but if
bit-cli ever calls `install_default()` itself, re-check.

**8.6 — `with_ignore_tls_errors(true)` degrades the fingerprint.** See
[§4](#a-footgun-with_ignore_tls_errorstrue-degrades-the-fingerprint).

---

## 9. Forking `impit` — the serious option

You asked whether to fork impit and patch it, combining the best of `wreq`. **I think you should, and
the case is stronger than it looks** — because you are *already* taking on fork risk. §8.2 puts four
apify git forks in your workspace root regardless. Forking impit does not add a new class of risk; it
gives you control over risk you have already accepted.

### What a fork buys, in order of value

**9.1 — Fix the `h2` patch (high value, low effort).** The bug in §7 is a version-resolution failure.
Two routes:

* **Cheap:** rebase apify's `h2` changes onto `0.4.19` and bump the fork's version so the patch
  actually applies. The change itself is small — a `pseudo_order` field on the HEADERS encoder and an
  ordered emit loop. **Effort: hours.** This alone takes impit's Akamai fingerprint from wrong to
  nearly-Chrome.
* **Right:** while you are in there, delete the env var. Thread the order through as a normal field
  from `Impit` → `reqwest` → `hyper` → `h2`, so it is per-client rather than process-global, and so
  you are not relying on `set_var` being sound. **Effort: days**, because it touches four crates —
  but it is the difference between a hack that works and a design that survives concurrency.

**9.2 — Port `wreq`'s H2 SETTINGS fidelity (medium value, low effort).** `wreq` emits `1:65536` and
the `1:1:0:219` PRIORITY frame; impit does not. These are data, not architecture — read the values out
of `wreq-util`'s profile tables (Apache-2.0, compatible) and add them to impit's `Http2Fingerprint`.
**Effort: hours.** No BoringSSL comes along: you are copying *numbers*, not code.

**9.3 — Widen the profile set (medium value, ongoing).** impit has 21 profiles, `koon` has 268 and
`wreq-util` 113. Both are permissively licensed (MIT and Apache-2.0). Profiles are declarative data —
cipher lists, extension orders, H2 settings — so porting them is transcription plus verification with
`tlsprobe`, not engineering. Start with the two or three you actually need.

**9.4 — Cut the dependency weight (low value, medium effort).** impit's unconditional `http3` forces
`--cfg reqwest_unstable` on your whole tree (§8.3). A fork can put `http3` behind a feature flag and
delete the rustflag requirement entirely. Nice-to-have.

**9.5 — Fix `with_ignore_tls_errors` (low effort).** Make it preserve `signature_algorithms` instead
of falling back to rustls defaults (§4), or at minimum log loudly.

### What a fork does *not* buy

Be clear-eyed: **you cannot port `wreq`'s TLS layer.** Its fidelity comes from BoringSSL letting you
order the ClientHello arbitrarily. Bringing that across means bringing BoringSSL, which is the thing
you rejected. The fork's ceiling on TLS is whatever the apify `rustls` fork can express — which,
measured, is already very good (§4). **The realistic goal is: keep impit's TLS, reach `wreq`'s HTTP/2.**
That is achievable, and it closes the only measured gap.

### Recommended shape

Fork **`apify/impit`** and **`apify/h2`** into your org. Leave `rustls`, `tower-http`, and
`hyper-util` pointed at apify's — you have no changes to make there and every reason to keep taking
their rebases. Pin all five by rev. Upstream your h2 fix to apify as a PR: if they take it, your fork
shrinks to nothing; if they do not, you have lost nothing.

**Total effort for the version worth doing (9.1 cheap + 9.2 + 9.5): roughly a week**, and it is
verifiable at every step with `tlsprobe --expect-akamai`. That is a real cost, but it is the price of
a standalone binary with a coherent fingerprint, and there is no other route to that combination.

### The escape hatch — do not pick one client

The strongest answer is **not to choose**. Make the client a compile-time feature:

```toml
[features]
default    = ["static-tls"]              # impit — static, portable, one TLS stack
static-tls = ["dep:impit"]
max-fidelity = ["dep:wreq", "dep:wreq-util"]   # BoringSSL, dynamic, Alpine/glibc only
```

Put both behind one internal `trait Fetcher { async fn get(&self, url) -> Result<Page> }`. The
default release is the static impit build; anyone who needs maximum fidelity against a hostile origin
builds with `--features max-fidelity` and accepts a dynamic binary. The abstraction is small — you
need `get`, headers, redirects, a body — and it means the §7 bug never blocks a release.

---

## 10. Driving an already-installed browser over CDP

The TODO's `--render` half specifies: *"drive a Chrome or Edge that is **already installed** over the
DevTools protocol… Off by default, never bundled, and absent gracefully when no browser is found."*

**None of the nine candidates do this.** `obscura`, `nokk`, and `aginxbrowser` are browser *engines
you ship* — the opposite of "already installed, never bundled". They **are** the CDP endpoint; you
need a CDP **client** that attaches to someone else's browser.

### The crates

| Crate | Version | Downloads | Recent | Licence | Notes |
|---|---|---|---|---|---|
| **`chromiumoxide`** | 0.9.1 | 3.58 M | 1.66 M | **MIT OR Apache-2.0** | async/tokio, typed CDP bindings generated from the protocol JSON. **The default choice.** |
| `headless_chrome` | 1.0.22 | 2.97 M | 804 K | MIT | **Blocking** API, simpler, smaller. Good if you want no async in this path. |
| `chromey` | 2.54.0 | 316 K | 54 K | MIT OR Apache-2.0 | Formerly `spider_chrome`; a `chromiumoxide` derivative tuned for scraping. Newer, smaller community. |

All three are pure-Rust CDP clients: **no TLS stack, no BoringSSL, no bundled browser**. They speak
WebSocket to a browser you launch or attach to, so they do not disturb the static-linking story at
all — the binary stays standalone, and the browser is an external runtime dependency that is either
present or not.

**Recommendation: `chromiumoxide`**, feature-gated. Highest usage, actively maintained, dual-licensed,
and its async model matches the `tokio` runtime impit already brings.

### The part the crates do not solve: finding the browser

`chromiumoxide` can launch a browser but will not *find* one for you in the way the TODO requires.
Write a small resolver, in this order:

1. An explicit `--browser-path` / `BIT_BROWSER` override. Always first.
2. Platform defaults:
   * **Linux** — `$PATH` lookup for `google-chrome`, `google-chrome-stable`, `chromium`,
     `chromium-browser`, `microsoft-edge`; then flatpak/snap paths.
   * **macOS** — `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`, the Edge and Chromium
     equivalents, and the same under `~/Applications`.
   * **Windows** — `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe`, then
     `%ProgramFiles%`/`%ProgramFiles(x86)%`/`%LocalAppData%` under `Google\Chrome\Application\`.
3. An already-running instance: if `--remote-debugging-port` is reachable, attach instead of launching.
4. **Found nothing → return a typed "no browser" error, not a panic.** That is the "absent gracefully"
   requirement, and it is the one behaviour to unit-test.

### How the tiers compose

```
bit-cli info <url>
  │
  ├─ static tier (default) ──── impit fetch → lol_html extract
  │     └─ 0 links found, or the page is obviously script-rendered?
  │           └─ if --render was NOT passed: report "page yielded no links; try --render"
  │
  └─ --render (opt-in) ──────── resolve installed browser
        ├─ none found → typed error, exit non-zero, name the paths searched
        └─ found → chromiumoxide: launch headless, navigate, wait for network idle,
                   take the DOM, extract from that
```

Keep the extraction code identical across both tiers — one function over an HTML string — so
`--render` changes only where the HTML came from.

### Two cautions

* **A driven Chrome is not a stealthy Chrome.** It sets `navigator.webdriver`, exposes CDP artifacts,
  and is what anti-bot vendors are *best* at spotting. It is the right tool for "this page builds its
  links in script" — the TODO's actual stated need — and the wrong tool for a hostile origin. That is
  fine: the TODO already says a CAPTCHA is a refusal, not a puzzle.
* **Version skew is real.** CDP changes between Chrome majors. `chromiumoxide` pins a generated
  protocol version; a much newer or older installed browser can fail on specific commands. Keep the
  `--render` path to the boring core — `Page.navigate`, `Runtime.evaluate`, `DOM.getDocument` — which
  has been stable for years.

---

## 11. Ranked findings

Ranked #1 first. Roles differ — #1 and #2 are complementary, not competing.

### 11.1 — `impit` — **ADOPT** (static tier), plan to fork

| | |
|---|---|
| **Stars / forks** | 578 ⭐ / 44 |
| **Licence** | **Apache-2.0** ✅ |
| **Owner** | **Apify** — a funded commercial scraping company; the only corporate-backed entry |
| **Stack** | `reqwest` 0.13 + **`rustls`** 0.23.43 + `tokio`; HTTP/1.1, /2, /3 |
| **Static musl** | ✅ **6.80 MB `static-pie`** |
| **crates.io** | ❌ git dependency only |
| **TLS fidelity** | ✅ verified Chrome JA4, ML-DSA, ECH, ALPS, PQ key share |
| **H2 fidelity** | ❌ **broken** — [§7](#7-root-cause-impits-h2-patch-silently-does-not-apply) |

The only candidate that produces the artefact you require. Everything else is a negotiation about how
much of "standalone binary" to give up. Already carries **`lol_html` 2.9**, so your HTML extraction is
a dependency you are paying for anyway.

### 11.2 — `pingly` — **ADOPT** (as the test oracle)

| | |
|---|---|
| **Stars / forks** | 63 ⭐ / 7 · **Apache-2.0** ✅ |

**Not a client.** A TLS/HTTP/1/2/3 **fingerprint analysis server**: JA3, JA4, Akamai H2, HTTP/3, QUIC,
passive TCP. It is the direct answer to *"asserted against a recorded capture rather than eyeballed."*

By **`0x676e67`, the author of `wreq`** — built to validate exactly this class of client. Its own deps
are rustls-based, so it builds without BoringSSL pain. It wants `libpcap` and `sdjournal` (systemd);
use its Alpine image `ghcr.io/0x676e67/pingly` to skip that.

**`tlsprobe` ([§13](#13-tlsprobe--the-oracle)) covers the JA3/JA4/Akamai subset with no system deps**,
and is what I used. Use `tlsprobe` in CI; reach for `pingly` when you need HTTP/3, QUIC or TCP-level
detail.

### 11.3 — `chromiumoxide` — **ADOPT** (render tier) — *not in your list*

3.58 M downloads, MIT OR Apache-2.0, async CDP client. See [§10](#10-driving-an-already-installed-browser-over-cdp).
This is the crate the TODO's `--render` design actually calls for.

### 11.4 — `wreq` — **KEEP AS AN OPTIONAL FEATURE**

| | |
|---|---|
| **Licence** | Apache-2.0 ✅ · **1,819,172 downloads** (660,538 recent) · **MSRV 1.98** |

Your TODO's research checks out: `wreq-util` 0.2.0 has **101 emulation variants**, `rquest`'s 152
versions are **all yanked**, and the download counts confirm it is the de-facto choice. Most mature,
best-covered, and — now measured — **the only client here with a correct, per-profile HTTP/2
fingerprint**.

It fails on exactly the axis you called a dealbreaker. But per [§9](#the-escape-hatch--do-not-pick-one-client),
"dealbreaker for the default build" and "unavailable" are different things: gate it behind
`--features max-fidelity` and you keep it for the cases that need it.

### 11.5 — `koon` — **WATCH** (best fingerprint research, wrong stack)

17 ⭐ · **MIT** ✅ · **not on crates.io** despite its README saying `cargo install koon-cli`.

Technically the most ambitious fingerprinting here: **268 profiles**, ML-DSA sig algs for Chromium
150+, **pinned per-profile extension order**. The changelog reads like someone diffing against real
browser captures. But: BoringSSL via a **personal git fork** (`scrape-hub/btls`), a **forked `http2`**,
not published, 17 stars, first commit 2026-02-23. Its musl build failed identically to `wreq`'s.

**Its profile tables are MIT — mine them for [§9.3](#9-forking-impit--the-serious-option) even though
you will not depend on the crate.**

### 11.6 — `nokk` — **WATCH** (honest alpha)

7 ⭐ · **MIT OR Apache-2.0** ✅ (best licence terms in the survey) · V8 + own DOM + CDP; **`wreq`**
transport (needs cmake + libclang).

The most interesting *idea* — build the browser coherent from TLS to JS rather than bolting stealth
plugins onto Chromium — and refreshingly honest: README says "**Alpha**", admits it is "not yet a match
for a dedicated fingerprinting suite like CreepJS", and ROADMAP marks CDP coverage, JS stealth and
scaling 🟡/⬜. Phase 9 (packaging, crates.io, prebuilt binaries) is ⬜ — nothing published. Revisit in
six months.

### 11.7 — `obscura` — **CONDITIONAL** (external binary only)

**22,200 ⭐** / 1,600 — by far the most popular · **Apache-2.0** ✅ · **`deno_core` 0.350** (→ V8),
`html5ever`, vendored `taffy` + `cosmic-text`.

Genuinely impressive; Cloudflare credits it as the prototype for Kitesurf. Real V8 via `deno_core`
(hence ~2 min builds, not the hours a V8-from-source build takes), speaks CDP, ~70 MB, ~30 MB RAM.

**Two catches.** (1) Its stealth tier **is `wreq`**, and `obscura-net`'s `stealth` feature is **off by
default** — so stock obscura has *no* TLS fingerprinting, and enabling it drags BoringSSL back in. It
pins `wreq-util =3.0.0-rc.12` exactly, with a source comment explaining that caret requirements broke
the build when rc APIs shifted (their issue #234). (2) It is a browser you **ship**, and the operator
ruled "never bundled".

If you want a `--render` fallback that needs no installed Chrome, obscura as an **optional external
binary** is defensible. As a dependency: no.

### 11.8 — `aginxbrowser` — **SKIP**

4 ⭐ · Apache-2.0 · first commit **2026-08-23** (two days before this survey). Inlines obscura's V8
path and adds an HTTP+MCP service. Stealth is `wreq` pinned `=6.0.0-rc.28`/`rc.10` — **pre-release,
exact-pinned** — with a source comment that the rc APIs shift underneath, and another noting the
`prefix-symbols` workaround "is only correct on Linux/Android (per wreq's README)": a **known
cross-platform defect** in exactly the area you care about. Take obscura directly.

### 11.9 — `crw` (fastCRW) — **SKIP** (licence + wrong shape)

739 ⭐ · **AGPL-3.0** ❌. A competent, popular Firecrawl-alternative scraping **service**. Two
disqualifiers, either sufficient: **(1)** AGPL-3.0 for a distributed CLI binary is a licensing decision
far above a TODO item, and it carries a CLA giving the vendor relicensing rights you do not have.
**(2) It has no browser fingerprinting** — its client is plain `reqwest` + `rustls`, so it does not
solve the problem T-244 exists to solve.

### 11.10 — `phrona` — **SKIP** (licence + wrong domain)

**1 ⭐** · **AGPL-3.0** ❌. A **metasearch engine** — 26 engines, merged and ranked. Not an HTTP client.
Impersonation is `wreq` at `6.0.0-rc.29`. 63 downloads; one of its two crates.io versions already
yanked.

### 11.11 — `ParallaX` — **SKIP** (not applicable, licence-blocked)

5 ⭐ · **PolyForm Noncommercial 1.0.0** ❌❌. **Not an HTTP client and not a scraping tool** — a
censorship-resistance SOCKS5 proxy (GFW circumvention, ML-KEM-1024 rekeying, TLS camouflage). It shares
the keyword "TLS fingerprint" with your search and nothing else. PolyForm Noncommercial forbids
commercial use — categorically incompatible with a general-purpose open-source CLI. It landed in your
list on keyword overlap. Drop it.

---

## 12. Recommended plan for T-244

1. **Static tier — `impit`, git-pinned.** Four `[patch.crates-io]` lines in the workspace root,
   `reqwest_unstable` in `.cargo/config.toml`. Pin revs, never branches. Budget real time for §8.2 —
   it swaps `rustls` tree-wide.
2. **Fix the H2 patch.** Fork `apify/h2`, rebase onto `0.4.19`, bump the version so the patch applies
   (§9.1). Without this your HTTP/2 fingerprint is `curl`'s. Verify with `tlsprobe --expect-akamai`.
3. **Extraction — `lol_html`.** Already in impit's graph; a streaming rewriter suits "every `href`
   ending `.torrent`, every `magnet:`, plus anchor text" and avoids a second HTML parser. *(My probe
   used `scraper` for speed of writing — `lol_html` is the cheaper long-term call but I did not
   benchmark it. Untested advice.)* Multiple matches → report and refuse, per the ruling.
4. **Verification — `tlsprobe` in CI.** `--expect-ja4` and `--expect-akamai` against stored goldens.
   **Assert JA4, never JA3** (§4). This is your acceptance criterion, mechanised.
5. **Render tier — `chromiumoxide`**, feature-gated, with the browser resolver in §10.
6. **Keep `wreq` behind `--features max-fidelity`** (§9 escape hatch) so the H2 gap never blocks a release.
7. **Mine `koon` and `wreq-util` profile tables** (MIT / Apache-2.0) if you need more than impit's 21.

**Do not** adopt `crw` or `phrona` (AGPL), `ParallaX` (noncommercial), or `aginxbrowser` (redundant
with obscura, pinned to pre-release `wreq` with a known cross-platform defect).

---

## 13. `tlsprobe` — the oracle

`research/tlsprobe/` — a dependency-light Rust tool that replaces the throwaway `ja3.py` from r1.

**What it does that the Python did not:**

* **Correct JA4** — r1's script hardcoded the SNI marker as `d`; `tlsprobe` emits `d`/`i` correctly,
  keeps signature algorithms in their original (unsorted) order per spec, caps counts at 99, and
  handles the empty-cipher case.
* **JA4_r** — the un-hashed form. When two fingerprints disagree, the hashes tell you only *that*;
  JA4_r tells you *where*.
* **JA3 both ways** — GREASE-filtered and raw-per-original-spec, so you can see the shuffle.
* **HTTP/2** — terminates TLS with a throwaway cert, negotiates `h2`, parses SETTINGS / WINDOW_UPDATE /
  PRIORITY / HEADERS, and emits the **Akamai fingerprint**. This is what produced §6.
* **Full HPACK decoding**, including the RFC 7541 Huffman table, so header *names* outside the 61-entry
  static table decode properly and the header-order capture has no holes.
* **HTTP/1.1 fallback** — records request header order when ALPN does not pick `h2`.
* **CI mode** — `--json`, `--once`, and `--expect-ja4` / `--expect-ja3` / `--expect-akamai`, which exit
  non-zero on mismatch.
* **Panic-free parsing** — every read goes through a bounds-checked cursor, so a truncated or hostile
  record returns `None` instead of unwinding.

```bash
cd research/tlsprobe && cargo build --release

# Human-readable, TLS + HTTP/2
./target/release/tlsprobe --port 8443

# CI assertion
./target/release/tlsprobe --once --expect-akamai '1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p'

# ClientHello only, no TLS termination
./target/release/tlsprobe --raw --port 8443
```

Clients must be pointed at it with verification disabled — the cert is camouflage. **Note the §4
footgun**: for impit, disabling verification also changes its `signature_algorithms`, so a JA4 captured
that way is not the shipping JA4. Capture JA4 in `--raw` mode (no handshake needed) and the Akamai
fingerprint in terminated mode.

---

## 13a. `fpsync` — staying current as browsers ship

`research/fpsync.py` — Python 3.8+, **standard library only**, no `pip install`.

A profile table pinned to Chrome 151 is a *correct* fingerprint of a browser nobody runs any more,
which is its own tell. Browsers ship every few weeks; your impersonation has to move with them.
`fpsync` answers three questions on a schedule.

**1. What is actually stable right now?** Straight from vendor APIs — Google's `versionhistory`,
Mozilla's `product-details`, Microsoft's enterprise feed:

```
$ fpsync.py versions
current stable
  chrome/linux 151.0.7922.173
  chrome/win   152.0.7977.54
  chrome/mac   152.0.7977.54
  firefox      154.0  (esr 140.14.0esr)
  edge         151.0.4129.107
```

**2. Have our profiles drifted?** Parses impit's `database.rs` and the published `wreq-util` source,
compares against stable, and **exits 1 on drift** so it drops straight into CI:

```
$ fpsync.py drift --impit-src vendor/impit
impit profiles
  chrome   newest=151   (13 profiles)
  firefox  newest=144   (4 profiles)
DRIFT [high] impit firefox profile: ours=144 stable=154
$ echo $?
1
```

That is a real finding, not a demo: it is how [§4](#staleness--measured-not-assumed) was measured.

**3. What does a *real* browser put on the wire?** This is the authoritative one. `capture` resolves
an installed Chrome/Edge using the [§10](#10-driving-an-already-installed-browser-over-cdp) resolver,
drives it headless at `tlsprobe`, and records the JA4 and Akamai fingerprint the genuine browser
emits:

```
$ fpsync.py capture --tlsprobe tlsprobe/target/release/tlsprobe
ground truth from Google Chrome 152.0.7977.54
  JA4     t13d1516h2_8daaf6152771_e5627efa2ab1
  Akamai  1:65536;2:0;3:1000;4:6291456;5:16384;6:262144|15663105|0|m,a,s,p
  headers :method, :authority, :scheme, :path, sec-ch-ua, ...
```

**This closes the loop.** Instead of asserting your impersonation against a value copied from a blog
post — including the one in [§6](#6-the-http2-fingerprint--the-gap-r1-flagged-now-measured) of this
document, which came from documentation rather than a browser I ran — you assert it against the
browser itself, on the machine you care about. It also reuses the browser you already need for
`--render`, so it costs no new dependency.

With no browser installed it fails the way §10 asks for — a typed error naming what it searched, exit
2, no panic:

```
$ fpsync.py capture
capture failed: no installed browser found
  searched: google-chrome, google-chrome-stable, chromium, chromium-browser, microsoft-edge, ...
```

### Suggested CI wiring

```yaml
# weekly, plus on any change to the profile table
- run: python3 research/fpsync.py drift --impit-src vendor/impit   # exit 1 → open an issue
- run: python3 research/fpsync.py report --json > fingerprints/upstream-$(date +%F).json
# on a runner that has Chrome:
- run: python3 research/fpsync.py capture --json > fingerprints/chrome-truth.json
- run: ./tlsprobe --once --expect-ja4 "$(jq -r .ja4 fingerprints/chrome-truth.json)" &
       cargo run -- info https://127.0.0.1:8443/
```

The last two lines are the acceptance criterion in its strongest form: capture the real browser, then
assert that `bit-cli` is indistinguishable from it. Keep the JSON snapshots in git — a diff over time
is the cheapest possible record of when and how a fingerprint moved.

**Caveats.** Vendor APIs change without notice; every fetch is individually error-trapped so one dead
endpoint degrades that field rather than the run. Apple publishes no machine-readable version feed, so
Safari is tracked by hand. `capture` needs a Chromium-family browser — Firefox has no equivalent
headless CDP path here.


---

## 14. Appendix: five deep reviews

Five passes over this document. Findings from all five are already folded into the text above; they
are recorded here so you can see what was challenged and what survived.

### Review 1 — factual accuracy

| Claim | Finding |
|---|---|
| impit JA4 `t13d1515h2_…` | ❌ **Wrong.** Marker is `i` (no SNI on an IP literal). Corrected throughout. |
| "impit emitted a fixed extension order" | ❌ **Wrong, and methodologically bad** — asserted from n=2. n=6 shows 3 distinct orders. Corrected; the derived claim that koon is "demonstrably ahead" was withdrawn. |
| "seven of nine are BoringSSL or depend on wreq" | ❌ **Five.** Seven are *unusable*, which is a different claim. Corrected. |
| `8daaf6152771` is Chrome's JA4_b | ✅ Confirmed against public JA4 documentation. Independently reproduced by `tlsprobe`. |
| wreq glibc build succeeds, musl fails | ✅ Confirmed; exact cmake error captured. |
| No musl C++ toolchain in apt | ✅ Confirmed — `apt-cache search musl` returns four packages, none C++. |
| impit needs the `[patch]` block | ✅ Confirmed by removing it and capturing the resolver error. |
| "323 crates total" | ⚠️ Imprecise — that was the whole probe graph including `scraper`/`tokio`, not impit alone. Softened. |
| "static-links with zero toolchain setup" | ⚠️ Overstated — needs `musl-tools`. Corrected to "needs a C compiler, not a C++ one". |
| koon 288 KB binary | ✅ Correctly withheld as meaningless (dead-stripped). |

### Review 2 — decision usefulness

| Issue | Resolution |
|---|---|
| The ranking was not tested against the H2 finding | §1 now carries an explicit trade table; impit stays #1 because static linking is a **hard operator constraint** and the H2 gap is fixable, but this is now argued, not assumed. |
| The doc identified the CDP gap but did not answer it | New [§10](#10-driving-an-already-installed-browser-over-cdp) with three crates, a recommendation, a browser resolver, and tier composition. |
| No fallback when static-linking and fidelity conflict | New [§9 escape hatch](#the-escape-hatch--do-not-pick-one-client): feature-gate both clients behind one `Fetcher` trait. This is a better answer than picking either. |
| `pingly` recommendation partly obsolete | Repositioned: `tlsprobe` for CI, `pingly` for HTTP/3/QUIC/TCP depth. |
| Corrections were invisible | New [§0](#0-what-changed-in-revision-2) revision table. |
| Acceptance criteria only implicitly mapped | §12 step 4 now names the exact assertions, including "assert JA4, never JA3". |
| `lol_html` recommended but never benchmarked | Flagged inline as untested advice. |

### Review 3 — adversarial: what would make this recommendation wrong?

Deliberately arguing against my own conclusion.

1. **"impit's H2 bug might be mine, not theirs."** Could the unused patch be an artifact of *my* probe
   manifest rather than impit's own workspace? **Partly conceded — verify this first.** In impit's own
   workspace the resolution may differ. But the mechanism is version-based (`0.4.7` cannot satisfy a
   `0.4.19` requirement) and the wire evidence is unambiguous. **Action: reproduce inside impit's own
   tree before filing anything upstream.** This is the single most important thing to re-check.
2. **"The measured JA4 came from an IP-literal connection with no SNI."** True. A real request carries
   SNI, giving `t13d1516h2_…`. The cipher and extension hashes are unaffected, but **the exact JA4 you
   assert in CI must be captured the way you will actually connect**, not copied from §4.
3. **"22.2k stars should beat 578."** obscura's popularity is real but for a different job, and its
   fingerprinting is off-by-default `wreq`. Star count is not evidence about the axis you care about.
   Rejected.
4. **"AGPL might be acceptable."** If bit-cli were AGPL already, `crw` would deserve a second look. But
   it has no fingerprinting, so it fails on the merits regardless. Rejected independently of licence.
5. **"Two TLS stacks might be fine — binary size is only ~2 MB more."** The operator's objection was
   never mainly size; it was a second handshake implementation and its build-toolchain tail. Measured,
   that tail is exactly what breaks static linking. Rejected.
6. **"You never tested against a real fingerprinting origin."** **Conceded, and it is the biggest
   limitation of this survey.** Every result is localhost. A live test against a
   JA4-and-H2-reading origin could reorder the ranking. The sandbox proxy made it impossible here.
7. **"impit at 578 stars with one corporate backer is a bus-factor risk."** Fair, and it is the best
   argument for [§9](#9-forking-impit--the-serious-option): if you are forking anyway, apify going
   quiet hurts less.
8. **"`m,s,a,p` might not matter."** Possible — many origins only read JA3/JA4. But the incoherence
   (Chrome TLS + curl-shaped H2) is arguably a *stronger* signal than a plain client, because no real
   browser produces that combination. I have not measured which origins care. Unresolved; treat as risk.

### Review 4 — final pass over the assembled document

Run after `tlsprobe`, `fpsync`, and the fork and CDP sections were written. Six findings; all are
recorded rather than fixed, because each is a limit of the evidence rather than an error in it.

1. **The §6 baseline is documentation, not a measurement — and §6 carries a major conclusion.**
   impit and `wreq` were both measured on my wire. The "Real Chrome" row they are judged against was
   read from published JA4/Akamai references. So the *comparison between the two clients* is solid
   (same probe, same conditions, and they differ from each other), but the *verdict that wreq matches
   Chrome and impit does not* rests on an unverified reference. `fpsync capture` exists precisely to
   close this and I could not run it — **no browser is installed in my sandbox.** Run
   `fpsync.py capture` on a machine with Chrome before treating §6 as settled. It is the single
   cheapest way to confirm or overturn the most consequential finding here.

2. **§6 compares `wreq`'s Chrome136 profile against a generic "Chrome" reference.** Akamai
   fingerprints differ between Chrome majors — `3:1000` appears in some and not others. The
   pseudo-header-order claim (`m,a,s,p` vs `m,s,a,p`) is version-stable and I stand behind it; the
   SETTINGS-level agreement is looser than the table's ✅ implies. Read that ✅ as "right shape, right
   pseudo-order", not "byte-identical".

3. **The fork effort estimate is a judgement, not a measurement.** I did not attempt the `h2` rebase.
   "Hours" for §9.1-cheap and "roughly a week" for the bundle are informed guesses from reading the
   diff surface. Treat them as planning inputs, not commitments.

4. **HTTP/3 is completely untested.** impit forces `http3` on — that is the entire reason for the
   `reqwest_unstable` rustflag (§8.3) — yet I never exercised it, and `tlsprobe` does not speak QUIC.
   If bit-cli ever negotiates H3, its QUIC/H3 fingerprint is an **unmeasured surface**, and given §7
   showed the H2 path silently broken, H3 deserves suspicion rather than trust. `pingly` covers H3 and
   QUIC; this is the strongest reason to reach for it over `tlsprobe`.

5. **The static binary was never run anywhere but the machine that built it.** `file` reports
   `static-pie linked` and `ldd` reports `statically linked`, which is strong evidence, but I did not
   execute it in a scratch container with no glibc. Verify with
   `docker run --rm -v $PWD:/x gcr.io/distroless/static /x/bit-cli --version` before relying on it.

6. **The static musl binary never completed a successful HTTPS fetch.** Its link extraction was
   verified over plain HTTP, and its ClientHello over a probe that deliberately does not complete a
   handshake. The sandbox's MITM proxy blocks browser-shaped TLS, so **the recommended artifact's
   full happy path — static binary, real HTTPS origin, page fetched, links extracted — is unproven.**
   Each half works; the whole was never run end to end. This is the first thing to try on a normal
   network, and it should take about a minute.

**Net effect on the recommendation: unchanged.** Nothing in this pass undermines the static-linking
result, which is the hard constraint and the most thoroughly measured finding in the survey. Findings
1 and 6 are the two to close first.


### Review 5 — final review before hand-off

Five findings. The last one challenges the premise of the whole survey and is the most useful of them.

1. **The document's confidence is not evenly matched to its evidence.** §1 states the recommendation
   in bold as though settled; §14 and §15 list six load-bearing items that were never verified. A
   reader who stops at the bottom line gets more certainty than the evidence supports. The draft
   banner now says so up front, but the tension is structural: treat §1 as *the current best guess*,
   not a conclusion.

2. **Only three of the nine were ever build-tested.** `impit`, `wreq`, and `koon` were compiled. The
   BoringSSL classification for `nokk`, `obscura`, `aginxbrowser`, and `phrona` is read off their
   `Cargo.toml` files — they depend on `wreq`, `wreq` is BoringSSL, BoringSSL fails static musl. The
   chain is sound but it **is inference, not measurement**. If any of them vendors a different TLS
   path under a feature flag I did not read, my classification is wrong for that project.

3. **The [§9 escape hatch](#the-escape-hatch--do-not-pick-one-client) was never prototyped.** "Put
   both clients behind one `Fetcher` trait" is clean in a document. In practice `reqwest`'s and
   `wreq`'s response types, cookie jars, redirect policies, and streaming bodies differ enough that
   the abstraction will leak somewhere. It is still the right shape; budget more than the paragraph
   implies, and prototype it before committing to it in a plan.

4. **`fpsync drift` compares major versions, which is weaker than it sounds.** A profile named
   `chrome_151` matching stable `151.0.7922.173` reports no drift — but Chrome's fingerprint can
   change *within* a major (a new extension, a reordered list) and the check would not notice. Version
   equality is a proxy for fingerprint equality, and a loose one. The real check is
   `fpsync capture` + `tlsprobe --expect-ja4` against a live browser; `drift` is only a cheap early
   warning, and should not be the thing you rely on.

5. **The survey never asks whether T-244 needs any of this.** Everything here assumes browser
   impersonation is required. That assumption came from the TODO and I did not challenge it — but it
   is worth challenging, because it is the difference between a week of fork work and an afternoon.
   Many torrent indexers serve their pages to a plain HTTP client with a sensible `User-Agent`
   perfectly happily; the TLS-fingerprinting arms race is concentrated among large commercial origins
   behind Cloudflare and Akamai. **Before building any of this: take the ten sites T-244 actually
   needs to read, fetch each with plain `reqwest` and a normal UA, and count how many fail.** If the
   answer is zero, the correct implementation of T-244 is an HTML parser and nothing else, and this
   entire survey is a contingency plan rather than a roadmap. If it is three, you know exactly which
   tier you need and why. That measurement costs an hour and could save the entire §9 effort — it is
   the single highest-value thing on this list, and it should have been step one.


---

## 15. What to double-check

**Do this before anything else, per [Review 5.5](#review-5--final-review-before-hand-off):** fetch
the ten sites T-244 actually targets with plain `reqwest` and a normal User-Agent, and count the
failures. If none fail, T-244 needs an HTML parser and nothing in this document. One hour, and it may
save the entire §9 effort.

**Then the two from [Review 4](#review-4--final-pass-over-the-assembled-document):**

1. **Run `fpsync.py capture` on a machine with Chrome.** §6's verdict is judged against a documented
   reference, not a browser I ran. One command settles it.
2. **Run the static musl binary against a real HTTPS origin.** Its halves are verified; the whole
   path never was, because the sandbox proxy blocks browser-shaped TLS. About a minute on a normal
   network.

Then:

* **macOS and Windows were not tested at all.** Every build number is Linux x86_64. `aws-lc-sys`
  supports both, but *verify* — cross-platform static linking is the premise of this recommendation and
  only one third of it is proven. macOS never supports fully-static binaries (libSystem must be
  dynamic), so "standalone" there means "no non-system dylibs".
* **Reproduce the §7 h2 finding inside impit's own workspace** before acting on it (Review 3.1).
* **No live-origin testing** (Review 3.6).
* **Star counts** read from GitHub web on 2026-08-25; the GitHub API was blocked in my sandbox.
* **`koon`'s binary size is not reported** — my probe dead-stripped it.
* **No security audit, no `cargo-deny` run.** Several ship a `deny.toml` (`obscura`, `crw`, `ParallaX`,
  `phrona`).
* **The published "real Chrome" Akamai value in §6 is from documentation, not a browser I ran.**
  Capture your own with `fpsync.py capture` before making it a golden value.
* **HTTP/3 is entirely unmeasured** despite impit forcing it on — see Review 4.4. Use `pingly` for it.
* **`tlsprobe`'s Huffman decoder does a linear table scan per bit.** Correct and fast enough for
  header blocks; do not lift it into a hot path without replacing it with a lookup tree.
