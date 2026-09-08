# trackers

An evidence-driven BitTorrent tracker aggregation and reliability repository.
It fetches public tracker lists from several upstreams, validates and
normalizes them as hostile input, measures tracker health as far as this
execution environment legitimately permits, and is intended to rank by measured
reliability rather than by reputation.

**Status: published, and thinly labelled.** Aggregation, validation,
normalization and deterministic generation work and are tested, the health
sweep runs every three hours, and the dataset is on the `data` branch:

```bash
curl -sS https://raw.githubusercontent.com/Azathothas/Trackers/data/trackers_all.json
```

⛔ **Take the JSON or the CSV, not the plaintext**, unless you intend to accept
every entry unmeasured. [`docs/schema.md`](docs/schema.md) defines every field,
and it travels with the data at
`raw.githubusercontent.com/Azathothas/Trackers/data/schema.md`.

⚠ **Most rows read `unknown` today**, which is honest rather than final: 282 of
1334 carry an observation. ⛔ **Nothing reads `dead`**, because saying that
needs three observations of one tracker.

---

## ⭐ Is this worth using instead of an existing list? Partly, and here is the number

This project exists only if it beats redistributing
[`ngosang/trackerslist`](https://github.com/ngosang/trackerslist). That is a
gate it is measured against rather than a claim, the instrument is
[`experiments/27-value-gate.py`](experiments/27-value-gate.py), and it runs in
the gate on every push so this section cannot go quietly stale.

**The answer is two-sided, and the unflattering half is first.**

⛔ **This list is worse than the baseline if you take it unfiltered.** It is
**13.4x longer**, and only **12.8%** of it answered our probe against the
baseline's **63.6%**. Piping our plaintext into a client instead of theirs
gets you five times more entries that did nothing for you.

⭐ **This list is better than the baseline if you take the measurement with
it.** The trackers we carry and the baseline does not contain an estimated
**107 live ones [67-169]**, against **63** in the whole baseline -- counted, not
estimated, because every one of its 99 was probed. Filtering ours to what was
measured live yields between **2.1x and 3.7x** as many working trackers,
comparing our worst case against their best.

**So: the value is in the labels, not in the URLs.** Publishing this plaintext
without the health data alongside it would make this project the thing it
exists to improve on, and the closest prior art is measured beside it in
[`HISTORY/gates.md`](HISTORY/gates.md) -- including the three lines it publishes
that are not URIs, two of them carrying a stranger's private-tracker
credential.

⚠ **Every figure above is a floor from one datacenter**, with **one
observation per tracker**. The baseline's 63 is a census of all 99; the 107 is
scaled from a 183-tracker sample and carries the interval shown. A tracker that
timed out is `unknown`, not dead, so both percentages understate both lists.
Read the next section before quoting any of them.

---

## ⚠ Read this before you read a number from this project

Every network measurement here was taken from **GitHub-hosted Actions runners
in one cloud provider's address space**. That is one vantage point, and no
amount of statistics removes its consequences.

- ⛔ **"Dead from a GitHub runner" is not "dead."** Trackers can treat
  datacenter ranges differently from residential ones. A tracker that is
  healthy for you may measure as unreachable here.
- ⛔ **The runners have no IPv6 egress**, measured on both images. Every
  IPv6-only tracker is therefore reported `unmeasurable` and **never** `dead`.
  Reporting it dead would be a statement about the probe, not about the
  tracker. ⭐ **It is measurable from somewhere else, and six of the sixteen
  are alive**: `TRACKERS_PROFILE=local python3
  experiments/33-ipv6-only-liveness.py` probes them from a machine that has
  IPv6, and a second arm reaches the HTTP ones through a relay that has one.
  Anything the relay saw is recorded as **second-hand**, never as our own
  measurement.
- **I2P, Yggdrasil, Tor and WebTorrent trackers are not measured at all.** They
  need routers or protocols this environment does not have. `unmeasurable`
  again.
- **Sample sizes are small.** Every figure carries its conditions in
  [`HISTORY/corpus-baseline.md`](HISTORY/corpus-baseline.md) and in the
  instrument that produced it.

If you need residential-vantage data, this project cannot give it to you, and
says so rather than pretending otherwise. ⭐ **You can measure from somewhere
better**: `TRACKERS_PROFILE=local` runs the same code with a wider budget.

## Conduct toward tracker operators

⛔ **It never announces.** The probe stops at BEP 15 connect and HTTP scrape.
There is no announce code path at all, which is the enforcement rather than a
policy somebody has to remember.

Whether this project's requests should identify themselves is an **open
question**, not a settled policy, and the reasoning is in
[`TODO/RULES.md`](TODO/RULES.md) section 4.1. The line that does not move: an
exclusion an operator has already given is honoured, and nothing here tries to
work around one.

### Stopping this project from contacting your tracker

⭐ **Publish a BEP 34 TXT record on your tracker's hostname.** It needs no
contact with us, works for every other client that honours it, and is checked
before anything here opens a socket to you:

```
tracker.example.  IN  TXT  "BITTORRENT DENY ALL"
```

That record means *the host runs no trackers*, and this project then sends
nothing at all -- no probe, no DNS beyond the TXT lookup itself. To keep some
endpoints and refuse the rest, name the ones you do run; **everything you do
not name is refused**, which is what the specification means by the record
being exhaustive:

```
tracker.example.  IN  TXT  "BITTORRENT UDP:1337 TCP:80"
```

Three things that decide whether it protects you:

- **A lookup we cannot complete is not consent.** If DNS does not answer, or
  answers ambiguously, the tracker is skipped rather than probed.
- **We ask public resolvers, not our own**, so an internal resolver that does
  not follow CNAMEs cannot make your record invisible to us.
- ⚠ **It is keyed on a hostname.** Where a list publishes your tracker by IP
  address there is no name to look up, and the record cannot protect that
  entry. [`TODO/measurement.md`](TODO/measurement.md) carries the gap.

### If you would rather tell us directly

**Open an issue** at
[`github.com/Azathothas/Trackers/issues`](https://github.com/Azathothas/Trackers/issues)
naming the hostname. It is honoured whatever it says about whether the tracker
works: that is not ours to second-guess.

An upstream list that blacklists you for a reason reading as an operator
request also reaches us, indirectly: this project enforces those and declines
to adopt another project's *measurement opinions* about you.

⚠ **Both need a human here to act. The DNS route does not**, which is why it is
preferred and listed first.

## Running it

Python 3.11 or newer, standard library only. Nothing to install, and everything
below runs offline on any host.

```bash
python3 scripts/check-gate.py
```

That is the whole local gate: the checks, the test suite, an offline census
and two end-to-end generations.
[`scripts/README.md`](scripts/README.md) says what each part owns.

```bash
python3 scripts/generate.py --offline --out out
```

Builds the dataset from the pinned fixtures into `out/`, deterministically:
two runs over identical inputs are byte-identical, and CI asserts it.

It writes `report.md` beside the list, and its *Refused entries* section names
every URL a source offered that was not published, with the reason. Two kinds
are refused: an upstream exclusion this project honours (an operator's request,
or safety), and ⭐ **a URL carrying somebody's private-tracker passkey**, which
is listed with the credential removed. Seven of those are in the current
corpus. A row that vanishes without an explanation is the thing that section
exists to prevent.

⚠ On a Windows host `python3` may resolve to a stub that exits without running.
Use `python`, and see
[`docs/conventions/shell.md`](docs/conventions/shell.md) section 6.

## Where to go next

| you are | read |
| --- | --- |
| ⭐ **working on this repository**, human or agent | [`docs/AGENTS.md`](docs/AGENTS.md), in full. It is the router and it assumes no prior context |
| deciding whether to trust the method | [`TODO/RULES.md`](TODO/RULES.md) sections 1 and 3, and [`HISTORY/corrections.md`](HISTORY/corrections.md), which tabulates every claim this project has withdrawn |
| looking for a specific document | [`docs/README.md`](docs/README.md) |
| checking where a number came from | [`HISTORY/corpus-baseline.md`](HISTORY/corpus-baseline.md) and [`experiments/README.md`](experiments/README.md) |

## Licence

**0BSD.** Use, copy, modify, redistribute, fork and integrate this project
**without attribution or credit**. [`LICENSE`](LICENSE) grants permission "for
any purpose with or without fee" and carries no notice-retention proviso, so
the claim and the licence agree.

⚠ The reference corpus under [`references/`](references/) is **other people's
code**, kept at captured commits as evidence. Each carries its own licence,
three of them copyleft, and nothing is copied from any of them.
[`references/PROVENANCE.md`](references/PROVENANCE.md) has the table.

## Known weaknesses

⭐ The current list is [`HISTORY/corrections.md`](HISTORY/corrections.md), which
carries a severity per correction so the error rate is checkable rather than
asserted. The largest gaps:

- **The published labels are thin.** 1334 trackers are published and 282 of
  them carry a real observation, so most rows honestly read `unknown`. That
  improves every time the sweep runs, and until it does the dataset is a list
  with a few hundred labels rather than a labelled list.
- **One torrent client has been run against this output, not five.** aria2
  1.37.0 accepts it; qBittorrent, Transmission, Deluge and BiglyBT are
  **absent, not passing** ([T-035](TODO/claims.md)).
- **No tracker here can be called dead, and none is.** Saying so needs three
  observations and the history is younger than that, so the honest state for a
  tracker that did not answer is `unknown`.
- **Every measurement comes from a datacenter or from one authoring host.**
  Neither is a residential connection, which is where the consumers of this
  data actually sit. A tracker that answers you may not answer us.
- **Whether our own identity gets us refused is unmeasured.** If trackers
  filter the client string we send, a "did not answer" is partly a fact about
  us ([T-012](TODO/claims.md)).
- **Nothing is ranked.** The scoring model is unchosen, deliberately: there is
  not enough history yet to fit one without fitting it to noise
  ([T-044](TODO/scoring.md)).
- **I2P, Yggdrasil and WebTorrent trackers are `unmeasurable` from here**, which
  is a statement about this vantage and never about them.

**Assume more remain.**

⚠ **This list was itself stale on 2026-09-09**, and all four of its entries had
become false: a client had been run, the corpus had been swept three times, the
value gate had been answered in this same document 170 lines above, and the
credential leak was fixed. A page that carries the honest self-assessment is
the worst one to let drift, so it is checked when the value gate is.
