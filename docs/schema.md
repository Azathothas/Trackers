# schema.md

Every field in the labelled dataset, with its meaning. **A field not on this
page is not emitted**, and a definition here with no field behind it is a
defect: [`../tests/test_labelled.py`](../tests/test_labelled.py) diffs the two
in both directions.

The files are `trackers_all.json` and `trackers_all.csv`, and they carry the
same rows. `trackers_all.txt` is the same tracker set with the labels stripped.

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

## Cross-format consistency

⛔ **The three files carry the same tracker set, exactly**, and
`scripts/generate.py` refuses to publish when they do not
([T-061](../TODO/publication.md)). Not the same counts: the same set. Two
formats that disagree are a silent corruption a consumer cannot detect, which
is the one failure class worse than an error.
