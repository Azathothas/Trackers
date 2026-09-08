"""T-060 and T-061: the labelled dataset, and the three formats agreeing.

The `Prove` clauses, in order:

  T-060  every emitted field appears in `docs/schema.md` with a definition,
         enforced by a diff of the field set against the schema.
  T-061  mutating one format and not the others fails publication.

⛔ **The diff runs in both directions.** A field emitted with no definition is
one consumers will misread; a definition with no field behind it is a promise
the data does not keep. Checking one direction catches half.

No network. Run:  python3 -m unittest tests.test_labelled -v
"""

from __future__ import annotations

import csv
import io
import json
import os
import re
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

from trackers.labelled import (CSV_FIELDS, FIELDS, render_csv,  # noqa: E402
                               render_json, row_for)
from trackers.model import HealthState  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.state import TrackerHistory  # noqa: E402

SCHEMA = os.path.join(REPO, "docs", "schema.md")


def schema_fields() -> set[str]:
    """The fields `docs/schema.md` defines, from its main table alone.

    ⚠ Scoped to the table between the `## Fields` heading and the next one, so
    the deliberately-absent table below it cannot be read as a definition. That
    would make the check pass by naming a field as missing.
    """
    with open(SCHEMA, encoding="utf-8") as fh:
        text = fh.read()
    section = re.search(r"^## Fields$(.*?)^## ", text, re.M | re.S)
    assert section, "docs/schema.md has no '## Fields' section"
    return set(re.findall(r"^\| `([a-z_]+)` \|", section.group(1), re.M))


def a_tracker(url: str = "udp://tracker.example:6969/announce"):
    return parse(url)


class TheSchemaIsTheContract(unittest.TestCase):
    """T-060's `Prove` clause."""

    def test_every_emitted_field_is_defined(self):
        undefined = sorted(set(FIELDS) - schema_fields())
        self.assertEqual(undefined, [],
                         f"emitted with no definition in docs/schema.md: "
                         f"{undefined}. A field nobody can define is a field "
                         f"consumers will misread.")

    def test_every_defined_field_is_emitted(self):
        unemitted = sorted(schema_fields() - set(FIELDS))
        self.assertEqual(unemitted, [],
                         f"defined in docs/schema.md and never emitted: "
                         f"{unemitted}. That is a promise the data does not "
                         f"keep.")

    def test_a_row_carries_exactly_the_field_set(self):
        row = row_for(a_tracker())
        self.assertEqual(sorted(row), sorted(FIELDS))

    def test_the_document_advertises_its_own_field_list(self):
        doc = json.loads(render_json([a_tracker()], provenance={},
                                     generated_at="1970-01-01T00:00:00Z",
                                     code_version="0.0.0"))
        self.assertEqual(doc["fields"], list(FIELDS),
                         "a consumer detects a schema change from this list, "
                         "so it must be the list actually emitted")

    def test_the_vantage_limitation_travels_in_the_data(self):
        """RULES 17: the reader most likely to misread this will never open a
        methodology page."""
        doc = json.loads(render_json([a_tracker()], provenance={},
                                     generated_at="1970-01-01T00:00:00Z",
                                     code_version="0.0.0"))
        self.assertIn("residential", doc["vantage_note"])


class AnUnprobedTrackerIsUnknown(unittest.TestCase):
    """⛔ The distinction the whole dataset is for."""

    def test_never_checked_is_unknown_with_a_null_rate(self):
        row = row_for(a_tracker())
        self.assertEqual(row["health_state"], HealthState.UNKNOWN.value)
        self.assertIsNone(row["success_rate"],
                          "a rate of 0.0 says 'failed every check' about a "
                          "tracker nobody has checked")
        self.assertEqual(row["checks"], 0)
        self.assertIsNone(row["measurement_rung"])
        self.assertIsNone(row["observed_from"])

    def test_three_failed_observations_reach_dead(self):
        """⛔ The defect this exists to prevent, and it shipped.

        `_health_of` returned the last observation's state, where the sample
        count is 1 by construction, so no accumulation could ever reach `dead`
        and `MIN_SAMPLES_FOR_DEATH` was decorative. On 2026-09-09 the published
        dataset carried 28 trackers with three observations and no successes,
        every one of them `unknown`.
        """
        from trackers.probe import MIN_SAMPLES_FOR_DEATH
        url = "udp://gone.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for index in range(MIN_SAMPLES_FOR_DEATH):
            history = history.observe(
                state="unknown", ok=False,
                observed_at=f"2026-09-0{1 + index}T00:00:00Z",
                rung="dns", failure="timeout")
        row = row_for(parse(url), history=history)
        self.assertEqual(row["health_state"], HealthState.DEAD.value)
        self.assertEqual(row["checks"], MIN_SAMPLES_FOR_DEATH)

    def test_one_short_of_the_threshold_is_not_dead(self):
        """⚠ The boundary from below, so the threshold cannot quietly become
        one."""
        from trackers.probe import MIN_SAMPLES_FOR_DEATH
        url = "udp://maybe.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for index in range(MIN_SAMPLES_FOR_DEATH - 1):
            history = history.observe(
                state="unknown", ok=False,
                observed_at=f"2026-09-0{1 + index}T00:00:00Z",
                rung="dns", failure="timeout")
        self.assertEqual(row_for(parse(url), history=history)["health_state"],
                         HealthState.UNKNOWN.value)

    def test_an_unmeasurable_tracker_never_reaches_dead_however_many_checks(self):
        """⛔ RULES 3.1, asserted on the published state rather than only in
        the state machine."""
        url = "http://tracker.i2p/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for index in range(9):
            history = history.observe(
                state="unmeasurable", ok=False,
                observed_at=f"2026-09-{1 + index:02d}T00:00:00Z",
                rung="none", failure="unsupported")
        self.assertEqual(row_for(parse(url), history=history)["health_state"],
                         HealthState.UNMEASURABLE.value)

    def test_a_refusal_aimed_at_us_never_reaches_dead(self):
        """A 403 may be about our identity rather than the tracker (T-012)."""
        url = "http://refuses.example:80/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for index in range(9):
            history = history.observe(
                state="unknown", ok=False,
                observed_at=f"2026-09-{1 + index:02d}T00:00:00Z",
                rung="transport_response", failure="blocked_by_policy")
        self.assertNotEqual(row_for(parse(url), history=history)["health_state"],
                            HealthState.DEAD.value)

    def test_a_tracker_that_failed_every_check_is_distinguishable(self):
        history = TrackerHistory.new("udp://tracker.example:6969/announce",
                                     "2026-09-09T00:00:00Z")
        history = history.observe(state="unknown", ok=False,
                                  observed_at="2026-09-09T00:00:00Z",
                                  rung="dns", failure="timeout")
        row = row_for(a_tracker(), history=history)
        self.assertEqual(row["success_rate"], 0.0)
        self.assertEqual(row["checks"], 1)
        self.assertEqual(row["failure"], "timeout")

    def test_an_unmeasurable_network_says_so_without_an_observation(self):
        """RULES 3.1. Structural, so it does not need a failed probe to prove
        it -- and asking would measure our own reachability."""
        row = row_for(parse("http://tracker.i2p/announce"))
        self.assertEqual(row["health_state"], HealthState.UNMEASURABLE.value)
        self.assertTrue(row["unmeasurable_reason"])

    def test_the_two_axes_stay_two_columns(self):
        row = row_for(parse("udp://tracker.i2p/announce"))
        self.assertEqual(row["transport"], "udp")
        self.assertEqual(row["network"], "i2p")

    def test_a_url_with_no_port_carries_null_not_a_default(self):
        row = row_for(parse("http://tracker.example/announce"))
        self.assertIsNone(row["port"],
                          "filling in a default port invents an endpoint the "
                          "list never named")


class TheFormatsAgree(unittest.TestCase):
    """T-061."""

    def setUp(self):
        self.trackers = [parse(u) for u in (
            "udp://a.example:6969/announce",
            "http://b.example:80/announce",
            "https://c.example:443/announce")]
        self.provenance = {"udp://a.example:6969/announce": ["one", "two"]}

    def test_json_and_csv_carry_the_same_urls_in_the_same_order(self):
        doc = json.loads(render_json(self.trackers, provenance=self.provenance,
                                     generated_at="1970-01-01T00:00:00Z",
                                     code_version="0.0.0"))
        rows = list(csv.DictReader(io.StringIO(
            render_csv(self.trackers, provenance=self.provenance))))
        self.assertEqual([t["url"] for t in doc["trackers"]],
                         [r["url"] for r in rows])

    def test_the_csv_header_is_the_field_list(self):
        header = next(csv.reader(io.StringIO(
            render_csv(self.trackers, provenance=self.provenance))))
        self.assertEqual(header, list(CSV_FIELDS))

    def test_sources_join_on_a_character_that_is_not_the_delimiter(self):
        rows = list(csv.DictReader(io.StringIO(
            render_csv(self.trackers, provenance=self.provenance))))
        first = [r for r in rows if r["url"].startswith("udp://a.")][0]
        self.assertEqual(first["sources"], "one;two")

    def test_an_absent_value_is_an_empty_cell_not_the_word_none(self):
        text = render_csv(self.trackers, provenance=self.provenance)
        self.assertNotIn("None", text)

    def test_csv_writes_lf_and_not_crlf(self):
        """RULES 15.5. The default is CRLF, so the same instrument would emit
        different bytes than the JSON writer beside it."""
        text = render_csv(self.trackers, provenance=self.provenance)
        self.assertNotIn("\r", text)

    def test_two_renders_of_one_input_are_byte_identical(self):
        """RULES 3.6, asserted on the formats the consumer receives."""
        args = dict(provenance=self.provenance,
                    generated_at="1970-01-01T00:00:00Z", code_version="0.0.0")
        self.assertEqual(render_json(self.trackers, **args),
                         render_json(self.trackers, **args))
        self.assertEqual(render_csv(self.trackers, provenance=self.provenance),
                         render_csv(self.trackers, provenance=self.provenance))


if __name__ == "__main__":
    unittest.main()
