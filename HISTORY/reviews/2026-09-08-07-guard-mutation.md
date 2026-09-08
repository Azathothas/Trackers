# 2026-09-08-07 guard mutation

**Lens 2**, `docs/methodology/reviews.md`: *can my new guard actually fail?*

Every guard added this session had the defect it exists to catch planted, and
the exit code read **unpiped**. Twenty-six mutations in total. Two survived,
and both were findings.

---

## The mutations, and what each proved

| planted defect | tests that failed |
| --- | --- |
| a resolver divergence recorded as `dns_failure` | 3 |
| `RESOLVER_DIVERGENCE` dropped from `ABOUT_US` | 1 |
| unspecified addresses counted as resolution | 3 |
| the second lookup never asked | 10 |
| the null-address filter removed from `usable` | 1 |
| `only_unspecified` never fires | 2 |
| the reclassification gate removed | 1 |
| loopback-from-a-name allowed again | 1 |
| the socket's peer never read | 1 |
| a UDP result claiming its address was observed | 1 |
| the post-connect guard never fires | 1 |
| the gone rule checked after degrading | 3 |
| the trend always taken from the ring | 1 |
| the new-tracker floor removed | 7 |
| the intermittent band merged away | 3 |
| `min interval` ignored | 4 |
| `max` of the stated intervals becomes `min` | 1 |
| a zero interval believed | 2 |
| schedule violations never reported | 2 |
| the DNS ceiling always satisfied | 1 |
| literals charged a DNS lookup again | 2 |
| the per-observation dedupe removed | 3 |
| the rolling tag renamed to `latest` | 1 |
| the ISO week taken from the calendar year | 1 |
| a timestamp without a zone accepted | 1 |
| an empty output allowed to take a channel | 1 |

## The two that survived

### 1. A test whose name claimed more than it checked ⛔ fixed

`test_a_null_address_alongside_a_routable_one_still_probes` asserted only that
a `Resolution` carrying one null and one routable address reported
`only_unspecified is False`. That is a property of a two-line helper. **The
address selection it was named for was never exercised**, so removing the null
filter from `usable` failed nothing.

Rewritten to stage a mixed answer through `getaddrinfo` and assert
`resolved_ip` -- the one observable that separates "used the routable address"
from "used the null one" on a platform where both connect. The mutation now
fails it.

⭐ This is the specific shape the lens names, and it was mine, written an hour
earlier.

### 2. The sweep-level idempotence guard was covered by the other layer ⛔ fixed

`test_the_updater_refuses_a_sweep_it_has_already_folded` drove the real command
three times and passed **with the header guard disabled**, because the
per-observation guard caught the duplicate first. The test could not tell the
two layers apart, so it was not a test of the layer it was written for.

The version that survives fills the ring past `RING_SIZE` before replaying the
old sweep, which is the only case the header layer owns: the evidence has
rolled off and the per-observation guard is blind. The mutation now fails it.

### 3. A guard that could not fail, in the module written to be a guard ⛔ removed

`secondhand.annotate` ended with:

```python
if "health_state" in health_record:
    annotated["health_state"] = health_record["health_state"]
```

Deleting it changed nothing, and no test noticed, **because `dict(record)` had
already copied the value**. It read as the enforcement of the module's central
rule and enforced nothing.

⭐ The negative case is what settles it, and the lens asks for it explicitly: a
mutation that makes `annotate` genuinely promote an observer's signal to
`health_state` **is** caught, by
`test_annotating_a_health_record_leaves_its_verdict_alone`. So the rule holds;
the two lines were theatre and are gone, with a comment saying why so they are
not reinstated as an improvement.

## What this pass did not look at

* Guards that predate this session. Their mutation results are in the reviews
  of 2026-09-05 and 2026-09-08's earlier passes.
* The two new experiments' `--expect` flags were driven in the direction that
  passes and, for `26`, in the direction that fails; `33 --expect-egress` has
  **not** been driven against a failing control, because doing so means
  breaking this host's IPv6 egress. That is written into the entry rather than
  claimed here.
