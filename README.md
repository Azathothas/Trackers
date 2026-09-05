# trackers

An evidence-driven BitTorrent tracker aggregation and reliability repository.
It fetches public tracker lists from several upstreams, validates and
normalizes them as hostile input, measures tracker health as far as this
execution environment legitimately permits, and is intended to rank by measured
reliability rather than by reputation.

⛔ **Status: a skeleton. Nothing is published.** Aggregation, validation,
normalization and deterministic generation work and are tested. Health
measurement is built and has never been pointed at the corpus. **No dataset
exists at any public URL, and nothing here claims any tracker is alive.**

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
  tracker.
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

Three properties worth stating plainly, because an opt-out that fails quietly
is worse than none:

- **A lookup we cannot complete is not consent.** If DNS does not answer, or
  answers something ambiguous, the tracker is skipped rather than probed.
- **We ask public resolvers, not our own**, because the recorded way this
  mechanism fails in production is an internal resolver that does not follow
  CNAMEs, honouring nothing and reporting nothing.
- ⚠ **It is keyed on a hostname.** If a list publishes your tracker by IP
  address rather than by name, there is no name for us to look up and the
  record cannot protect that entry. That gap is recorded in
  [`TODO/measurement.md`](TODO/measurement.md) rather than papered over.

Asking also works and is honoured; `src/trackers/exclusion.py` is where a
request lands. The DNS route is preferred only because it does not require you
to find us first.

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

- **No torrent client has ever been run against this project's output.** One
  client's parser was read. That is the weakest evidence relative to its
  importance anywhere here.
- **The probe has never been pointed at the corpus.** It is tested against an
  oracle of trackers this project controls, and against nothing else.
- **The value gate is unanswered.** Whether this dataset beats redistributing
  an existing list is not yet measured on liveness. A negative answer is a
  legitimate outcome and would be published as one.
- **The pipeline republishes private-tracker credentials**: six of them reach
  the generated plaintext today. [T-107](TODO/sources.md) is the fix.

**Assume more remain.**
