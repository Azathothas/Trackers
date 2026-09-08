"""T-062: a consumer can tell what they received, and a version that moves says so.

Four questions the published data must answer about itself: **which dataset,
when it was generated, under what rules, and in what shape.** Three versions
carry that, deliberately independent because they change for different reasons
-- a field can be added without any normalization rule moving.

⛔ **The second half of the `Prove` clause is the one with teeth**: changing
normalization semantics without bumping the version must fail a gate. That is
what the golden table below is. An unpinned version is a number somebody
remembers to update, which is to say a number that is eventually wrong while
looking authoritative.

⚠ **When this test fails, updating the table is usually the wrong fix.** Either
the behaviour change was unintended -- restore it -- or it was intended, in
which case `NORMALIZATION_VERSION` goes up **and then** the table is updated in
the same change, so the number and the behaviour move together.

No network. Run:  python3 -m unittest tests.test_versions -v
"""

from __future__ import annotations

import json
import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers import (NORMALIZATION_VERSION, SCHEMA_VERSION,  # noqa: E402
                      SCORING_VERSION)
from trackers.labelled import (FIELDS, digest_of, metadata_for,  # noqa: E402
                               render_json, row_for)
from trackers.normalize import parse  # noqa: E402

#: What normalization version 1 promises, as `(input, url, host, port)`.
#: Each row is a decision somebody made, not a sample of behaviour.
GOLDEN_V1 = (
    # Case is folded on the host and kept everywhere else.
    ("UDP://Tracker.Example:6969/Announce",
     "udp://tracker.example:6969/Announce", "tracker.example", 6969),
    # ⛔ An explicit port survives, including one that matches the scheme's
    # default. `udp` has no default-port convention, so dropping it would
    # invent an endpoint.
    ("http://tracker.example:80/announce",
     "http://tracker.example:80/announce", "tracker.example", 80),
    # A URL with no port keeps none rather than acquiring one.
    ("http://tracker.example/announce",
     "http://tracker.example/announce", "tracker.example", None),
    # Surrounding whitespace is not part of a URL.
    ("  udp://tracker.example:1337/announce  ",
     "udp://tracker.example:1337/announce", "tracker.example", 1337),
    # An IPv6 literal keeps its brackets, which are what separate the address
    # from the port.
    ("udp://[2001:db8::1]:6969/announce",
     "udp://[2001:db8::1]:6969/announce", "2001:db8::1", 6969),
    # A hostname suffix decides the network, and the scheme does not.
    ("http://tracker.i2p/announce", "http://tracker.i2p/announce",
     "tracker.i2p", None),
)


class NormalizationIsPinnedToItsVersion(unittest.TestCase):
    """⛔ The gate the `Prove` clause asks for."""

    def test_version_1_still_does_what_version_1_promised(self):
        self.assertEqual(
            NORMALIZATION_VERSION, 1,
            "the golden table below describes version 1. If the version moved "
            "deliberately, update the table in the same change; if it moved by "
            "accident, that is the finding.")
        for raw, url, host, port in GOLDEN_V1:
            with self.subTest(raw=raw):
                tracker = parse(raw)
                self.assertEqual(tracker.url, url)
                self.assertEqual(tracker.host, host)
                self.assertEqual(tracker.port, port)

    def test_the_table_covers_more_than_one_transport(self):
        """⚠ A golden table of one shape pins one path. This is the assertion
        that the table itself is worth having."""
        schemes = {raw.split(":")[0].lower().strip() for raw, *_ in GOLDEN_V1}
        self.assertGreaterEqual(len(schemes), 2, schemes)


class EveryPublishedArtefactSaysWhatItIs(unittest.TestCase):
    """T-062's first half, across all three formats."""

    def setUp(self):
        self.trackers = [parse("udp://a.example:6969/announce"),
                         parse("http://b.example:80/announce")]
        self.doc = json.loads(render_json(
            self.trackers, provenance={}, generated_at="2026-09-09T00:00:00Z",
            code_version="0.1.0+norm1"))

    def test_the_json_carries_all_four(self):
        for key in ("generated_at", "schema_version", "normalization_version",
                    "scoring_version"):
            with self.subTest(key=key):
                self.assertIn(key, self.doc)
        self.assertEqual(self.doc["schema_version"], SCHEMA_VERSION)
        self.assertEqual(self.doc["normalization_version"],
                         NORMALIZATION_VERSION)

    def test_the_scoring_version_is_null_while_no_model_exists(self):
        """⛔ RULES 1.5. A `1` would tell a consumer a methodology exists and is
        stable, which would be a fabricated number in the one place a consumer
        cannot check it."""
        self.assertIsNone(SCORING_VERSION)
        self.assertIsNone(self.doc["scoring_version"])

    def test_the_digest_changes_with_the_data_and_not_with_the_clock(self):
        """⭐ What lets a consumer tell a re-publication from a change."""
        later = json.loads(render_json(
            self.trackers, provenance={}, generated_at="2026-09-09T12:00:00Z",
            code_version="0.1.0+norm1"))
        self.assertEqual(self.doc["digest"], later["digest"],
                         "the digest moved because the clock did")
        self.assertNotEqual(self.doc["generated_at"], later["generated_at"])

        changed = json.loads(render_json(
            self.trackers[:1], provenance={},
            generated_at="2026-09-09T00:00:00Z", code_version="0.1.0+norm1"))
        self.assertNotEqual(self.doc["digest"], changed["digest"])

    def test_the_digest_covers_exactly_the_rows_published(self):
        """⭐ Recomputed from the document's own rows, which is what a consumer
        can do: the digest is only worth anything if it describes the data in
        the file rather than something the writer had in hand.

        ⚠ Not recomputed from the trackers passed in. The renderer sorts, so a
        test that hashed its own input order would fail for a reason that has
        nothing to do with the digest -- and that is how it first failed.
        """
        self.assertEqual(self.doc["digest"], digest_of(self.doc["trackers"]))

    def test_the_digest_notices_a_row_edited_in_place(self):
        """The negative half: a digest that did not move when a value did would
        be decoration."""
        tampered = json.loads(json.dumps(self.doc["trackers"]))
        tampered[0]["health_state"] = "live"
        self.assertNotEqual(self.doc["digest"], digest_of(tampered))

    def test_metadata_describes_every_file_by_digest(self):
        """⛔ How a CSV consumer answers the same question. CSV cannot carry
        document metadata: a version column repeated on every row is not a
        header, and a comment line breaks the format."""
        payloads = {"trackers_all.csv": b"url,transport\n",
                    "trackers_all.txt": b"udp://a.example:6969/announce\n"}
        meta = json.loads(metadata_for(
            payloads, generated_at="2026-09-09T00:00:00Z",
            code_version="0.1.0+norm1", count=2, digest="sha256:abc"))
        self.assertEqual(meta["schema_version"], SCHEMA_VERSION)
        self.assertEqual(meta["normalization_version"], NORMALIZATION_VERSION)
        self.assertIsNone(meta["scoring_version"])
        self.assertEqual(sorted(meta["files"]),
                         ["trackers_all.csv", "trackers_all.txt"])
        for name, entry in meta["files"].items():
            with self.subTest(file=name):
                self.assertTrue(entry["digest"].startswith("sha256:"))
                self.assertEqual(entry["bytes"], len(payloads[name]))

    def test_a_schema_change_is_visible_without_diffing_rows(self):
        """The `fields` list and `schema_version` answer the same question at
        two costs; a consumer should not have to read 1334 rows to see a
        column appear."""
        self.assertEqual(self.doc["fields"], list(FIELDS))
        self.assertIsInstance(self.doc["schema_version"], int)


if __name__ == "__main__":
    unittest.main()
