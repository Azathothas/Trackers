# Probes and tools

Reproduction artifacts for [`../RESEARCH.md`](../RESEARCH.md).

> **Draft.** These were written to answer specific questions quickly, not as finished software.
> Known gaps are listed in [the root README](../README.md#todo--known-gaps-in-these-tools).
> Public domain — see [LICENSE](../LICENSE). No attribution required.

| Path | What it is |
|---|---|
| `tlsprobe/` | **The oracle.** A TLS + HTTP/2 fingerprint capture server: JA3, JA4, JA4_r, Akamai HTTP/2, HPACK header order. `--json` and `--expect-*` make it a CI assertion. |
| `fpsync.py` | **Staying current.** Vendor version APIs, profile drift detection (exit 1), and ground-truth fingerprint capture from a real installed browser. Python stdlib only. |
| `ja3.py` | Revision 1's throwaway script. **Superseded by `tlsprobe`** — it hardcoded the JA4 SNI marker and had no HTTP/2. Kept only so r1's numbers can be traced. |
| `fixture-page.html` | Test page: one `.torrent` link, one magnet link, one unrelated link, one off-host `.torrent`. |
| `probes/t-impit/` | impit build probe — fetch, then extract every `.torrent` href / `magnet:` URI + anchor text. **Note the mandatory `[patch.crates-io]` block.** |
| `probes/t-wreq/` | wreq probe, same job. Builds on glibc; **fails** on `x86_64-unknown-linux-musl`. |

## Build and run

```bash
rustup target add x86_64-unknown-linux-musl && apt-get install -y musl-tools

# impit — static build (succeeds: static-pie, ~6.8 MB)
cd probes/t-impit
RUSTFLAGS='--cfg reqwest_unstable' cargo build --release --target x86_64-unknown-linux-musl
file target/x86_64-unknown-linux-musl/release/t-impit

# wreq — static build (fails: needs x86_64-linux-musl-g++, absent from apt)
cd ../t-wreq && cargo build --release --target x86_64-unknown-linux-musl
```

`impit` needs `RUSTFLAGS='--cfg reqwest_unstable'`; `wreq` needs rustc ≥ 1.98.

## Capturing a fingerprint

```bash
cd tlsprobe && cargo build --release

# TLS + HTTP/2, human-readable
./target/release/tlsprobe --port 8443
NO_PROXY=127.0.0.1 my-client https://127.0.0.1:8443/     # client must skip cert verification

# ClientHello only — no handshake, so no cert bypass needed
./target/release/tlsprobe --raw --port 8443

# CI assertion (exit 1 on mismatch)
./target/release/tlsprobe --once \
  --expect-akamai '1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p'
```

**Assert JA4, never JA3.** JA4 sorts before hashing so it survives extension shuffling; JA3 preserves
order and will flake. See [RESEARCH.md §4](../RESEARCH.md#extension-order--r1-was-wrong).

**Capture JA4 in `--raw` mode.** For `impit`, disabling certificate verification also changes its
`signature_algorithms`, so a JA4 captured through a terminated handshake is not the shipping JA4.
Use `--raw` for JA4 and terminated mode for the Akamai fingerprint.

## Tracking browser releases

```bash
python3 fpsync.py versions                                  # current stable, from vendor APIs
python3 fpsync.py drift --impit-src /path/to/impit          # exit 1 if profiles are behind
python3 fpsync.py upstream                                  # what wreq-util ships
python3 fpsync.py capture --tlsprobe tlsprobe/target/release/tlsprobe   # ground truth
python3 fpsync.py report --json                             # everything
```

`--json` works before or after the subcommand.
