# schema.md

Every field in the labelled dataset, with its meaning. **A field not on this
page is not emitted**, and a definition here with no field behind it is a
defect: [`../tests/test_labelled.py`](../tests/test_labelled.py) diffs the two
in both directions.

The files are `trackers_all.json` and `trackers_all.csv`, and they carry the
same rows. `trackers_all.txt` is the same tracker set with the labels stripped.

**Five category files** sit beside them, each with a rule you can audit:

| file | membership |
| --- | --- |
| `common.txt` | the other categories merged, plus every tracker whose most recent observation was `live`. ⭐ **Start here.** |
| `anime.txt` | provenance from a source the registry classifies `anime`. |
| `stable.txt` | measured only: at least 5 observations at a success rate of 0.95 or better. ⛔ **Empty until the history is deep enough**, and empty is the honest answer -- a reputation-seeded file pretending to be measured is not. |
| `foss.txt` | derived from FOSS-ecosystem provenance, plus a seed labelled as curated. Neither half has content yet. |
| `hardcoded.txt` | the maintainer's manual list, in their order, never sorted or ranked. |

⚠ **An empty category file is not a defect.** `report.md` says for each one
whether the rule matched nothing or the evidence it needs does not exist yet,
because those are different states.

⛔ **The labels are the product.** Taken unfiltered this list is longer and
less likely to answer than the baseline it aggregates; filtering it by what
measured live is what makes it worth using
([`../HISTORY/gates.md`](../HISTORY/gates.md)). A consumer who takes the
plaintext alone has taken the worse half.

---

## The vantage limitation, before the fields

⚠ **Health here is measured from one cloud provider's address space and from
one authoring host.** Neither is a residential connection, which is where most
consumers of this data sit. A tracker recorded `unknown` may answer you
perfectly well, and `unmeasurable` is a statement about our position rather
than about the tracker.

⛔ **Nothing is `dead` yet and nothing can be.** Saying it needs three
observations of one tracker, and the history is younger than that.

---

## Fields

| field | type | meaning |
| --- | --- | --- |
| `url` | string | The announce URL as published, after normalization. This is the string a client uses. |
| `transport` | string | `udp`, `http`, `https`, `ws` or `wss`. The wire protocol. |
| `network` | string | `clearnet`, `i2p`, `onion` or `yggdrasil`. Which network reaches the host. ⛔ Separate from `transport` on purpose: `udp://tracker.i2p/announce` is both, and one combined column re-imports the defect that classifier exists to prevent. |
| `host` | string | Hostname or IP literal, exactly as the URL carried it. |
| `port` | integer or null | `null` where the URL named none. ⚠ Not defaulted: `udp` has no default-port convention, so filling one in would invent an endpoint. |
| `sources` | list of strings | Which upstream lists contributed this URL, sorted. In CSV, joined with `;` -- never `,`, which is the delimiter. |
| `health_state` | string | `live`, `dead`, `degraded`, `unmeasurable`, `unknown` or `error`. `unknown` means never checked or too few observations; `error` means our probe broke; `unmeasurable` means this vantage cannot reach it at all. **Only `live` and `dead` are claims about the tracker.** |
| `measurement_rung` | string or null | How far the last observation got: `dns`, `connected`, `tls`, `transport_response`, `protocol_valid`, `tracker_semantic`, or `no_usable_address`. `null` where nothing has been observed. ⚠ A liveness claim without a rung is unfalsifiable, which is why this travels beside `health_state`. |
| `failure` | string or null | Why the last observation stopped where it did. `null` on success or with no observation. Values whose subject is **us** rather than the tracker -- `no_usable_address`, `blocked_by_policy`, `deadline_exceeded`, `probe_error`, `unsupported`, `excluded_by_operator`, `exclusion_undetermined`, `resolver_divergence`, `dns_undetermined` -- can never produce `dead`. |
| `checks` | integer | Lifetime observations of this tracker. `0` means never checked. |
| `successes` | integer | Of those, how many reached the rung that proves a tracker on its transport. |
| `success_rate` | number or null | Exponentially weighted success rate, 0 to 1. ⛔ **`null`, never `0`, for a tracker nobody has checked** -- a new tracker and one that failed every check are different facts, and a zero says the second about the first. |
| `first_seen` | string or null | ISO 8601 UTC. When this URL first appeared in any source we accepted. |
| `last_seen` | string or null | ISO 8601 UTC. The most recent observation of any kind. |
| `last_success` | string or null | ISO 8601 UTC, or `null` if it has never succeeded. |
| `last_failure` | string or null | ISO 8601 UTC, or `null` if it has never failed. |
| `observed_from` | string or null | The environment class the observations came from, for example `github-actions-hosted`. `null` where nothing has been observed. ⚠ One string rather than the whole vantage block: the probe version and IP families are identical for every row of a run and travel with the run's own health record. |
| `unmeasurable_reason` | string or null | Why this vantage cannot measure it, for example an i2p address with no router here. `null` for anything measurable. |

## Fields that are deliberately absent

⛔ **A field is not added because it sounds useful.** Each of these was a
candidate in [T-060](../TODO/publication.md) and each is missing for a reason a
consumer can check:

| absent | why |
| --- | --- |
| `latency`, latency statistics | The history keeps each observation's outcome and rung, not its round-trip time. A latency column would be a number this project does not retain. |
| `reliability_score`, `score_version` | No scoring model is chosen ([T-044](../TODO/scoring.md), decision D4 open), deliberately: there is not enough history to fit one without fitting it to noise. |
| `confidence` | A confidence over an unchosen model is an adjective with a decimal point. |
| `category` | The five categories do not exist yet ([T-046](../TODO/scoring.md)). |

## Document fields

Outside `trackers`, the JSON document carries:

| field | meaning |
| --- | --- |
| `generated_at` | ISO 8601 UTC, injected rather than read from a clock, so two runs over one input are byte-identical. |
| `code_version` | The version of the code that produced the file. |
| `count` | How many trackers the file carries. Cross-checked against the row count before publication. |
| `fields` | The field list above, in order, so a consumer can detect a schema change without diffing rows. |
| `schema_version` | The field set's version. Bumped when a field is added, removed or changes meaning, so a consumer detects a shape change without diffing rows. |
| `normalization_version` | The version of the rules that produced the URLs. ⛔ Pinned by a golden table in [`../tests/test_versions.py`](../tests/test_versions.py): changing what normalization does without moving this number fails a gate, because an unpinned version is a number somebody remembers to update. |
| `scoring_version` | ⛔ `null`, and that is the honest value: no scoring model is chosen ([T-044](../TODO/scoring.md)), so there is no methodology to version. A `1` here would tell you one exists and is stable. |
| `newest_observation` | When the most recent health observation in this dataset was taken, or `null`. ⛔ **Not the same question as `generated_at`**: the publisher runs after every sweep, including one that failed, so a file can be minutes old and its labels much older. Check both. |
| `publish_interval_seconds` | How often this dataset expects to be regenerated. ⭐ Present so you can decide it has gone stale **without us**: see below. |
| `stale_after_intervals` | How many missed publications before treating the data as stale. Three, not one, because a scheduled run can be late and one was observed 163 minutes late. |
| `digest` | `sha256:` over the rows, canonically serialised. ⭐ Two documents with one digest carry the same data whatever their `generated_at` says, which is how you tell a re-publication from a change. |
| `vantage_note` | The limitation stated in the data itself, because the reader most likely to misread the file will never open this page. |

## `metadata.json`, and why it exists

⛔ **CSV cannot carry document metadata.** A version column repeated on every
row is not a header, and a comment line breaks the format for the readers
people choose CSV for. So the release describes itself in a file of its own,
published beside the data:

```bash
curl -sS https://raw.githubusercontent.com/Azathothas/Trackers/data/metadata.json
```

It carries the four versions above and, for **each published file**, its size
and a `sha256:` digest. ⭐ **That is how a consumer of the CSV or the plaintext
answers the same question a JSON reader answers from the document itself**: hash
the file you hold and compare. RULES 9 -- the requirement is not dropped for a
format that cannot express it, it is met in the strongest form that format
allows.

## Reading it fresh

⚠ **`raw.githubusercontent.com` caches, so a fetch right after a publish can
return the previous document.** Measured on 2026-09-08: a fetch seconds after a
publish returned the older dataset, with no error and nothing to suggest it was
stale. ⭐ **Compare `generated_at` inside the document**, never the time you
fetched it. `C-16` in [`../HISTORY/claims.md`](../HISTORY/claims.md) carries the
measurement.

## Telling whether this project has stopped

⛔ **The worst thing this project could do is stop quietly.** A public
repository's scheduled workflows are disabled after 60 days without activity,
and if that happens every file here keeps serving HTTP 200 with data that no
longer moves.

⭐ **You can detect that without us**, and that is deliberate: the two fields
you need are in the bytes you already hold.

```python
import datetime, json, urllib.request
url = "https://raw.githubusercontent.com/Azathothas/Trackers/data/trackers_all.json"
doc = json.load(urllib.request.urlopen(url))
stamped = datetime.datetime.fromisoformat(doc["generated_at"].replace("Z", "+00:00"))
age = (datetime.datetime.now(datetime.timezone.utc) - stamped).total_seconds()
stale = age > doc["publish_interval_seconds"] * doc["stale_after_intervals"]
```

A CSV or plaintext consumer reads the same two fields from `metadata.json`.
⚠ **A watchdog of ours could not tell you this**, because it would run on the
schedule that stopped.

⛔ **Check `newest_observation` as well as `generated_at`.** They answer
different questions and the difference is where the interesting failure hides:
the publisher runs after every sweep *completion*, so if measurement stops
while publication continues, the file stays fresh and the labels quietly age.
`generated_at` tells you when the file was written; `newest_observation` tells
you when anything was last learned.

## Pin the branch, never a commit

⛔ **A commit SHA on the `data` branch is not a durable reference and breaks by
design.** The branch's history is reset when it grows past a measured ceiling
([T-081](../TODO/operations.md)), which discards commits and keeps every byte of
data -- history lives in tracked files here, so nothing is lost but the commit
graph. ⭐ **Pin the branch**, as every URL on this page does, or a tag.

## Cross-format consistency

⛔ **The three files carry the same tracker set, exactly**, and
`scripts/generate.py` refuses to publish when they do not
([T-061](../TODO/publication.md)). Not the same counts: the same set. Two
formats that disagree are a silent corruption a consumer cannot detect, which
is the one failure class worse than an error.
