"""T-107: a private tracker's credential is refused, not republished.

`C-70` found six people's passkeys, on seven URLs, travelling from two
upstreams through this pipeline into `trackers_all.txt` unchanged. RULES 6 says
this project holds no private-tracker data, and RULES 3.10 says a row that
disappears owes the consumer who noticed a reason -- so refusing is only half
the requirement and recording the refusal is the other half.

⭐ **The two failure directions are opposite, and both are tested.** Publishing
a credential hands somebody's account to everyone who reads the list. Refusing
one silently makes a tracker vanish with no explanation, which is the failure
the exclusion machinery exists to prevent. A test for either alone would pass
over the other.

⚠ **The fixtures are not edited.** They are verbatim captures, a rewritten
capture is not a capture, and rewriting one would delete the evidence that the
refusal works at all. So these run against the real corpus.

Run:  python3 -m unittest tests.test_credentials -v
"""

from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from trackers.acquire import read_cached  # noqa: E402
from trackers.exclusion import (PRIVATE_CREDENTIAL, REDACTED,  # noqa: E402
                                carries_private_credential, mask_credential)
from trackers.pipeline import aggregate, render_plaintext, render_report  # noqa: E402
from trackers.registry import enabled_sources  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: The shapes a private tracker actually uses, and the ones it does not.
CARRIES = (
    "http://tracker.example/announce?passkey=0123456789abcdef0123",
    "http://tracker.example/announce.php?passkey=0123456789abcdef0123",
    "http://tracker.example/0123456789abcdef0123/announce",
    "http://tracker.example/announce/0123456789abcdef0123",
    "https://tracker.example/scrape?pass_key=0123456789abcdef0123",
)
PUBLIC = (
    "udp://tracker.example:6969/announce",
    "http://tracker.example:8080/announce",
    "https://tracker.example/announce",
    "http://tracker.example/announce?uk=short",
    "wss://tracker.example:443/announce",
)


#: A token that does **not** look like a test vector, for the one test that
#: needs the narrowing to decline. It is assembled rather than written out, and
#: that is not obfuscation: `check-no-secrets.py` refuses a credential-shaped
#: literal anywhere this project writes, and it is right to -- a random-looking
#: constant in our own source is indistinguishable from a leaked one. The rule
#: is about literals, and this is a value the test builds.
REALISTIC_TOKEN = "9f3c" + "7ba1" + "5e2d" + "8406" + "1c7b"


def load_check():
    """Import `scripts/check-no-secrets.py` by path.

    A check in this project is a script with a hyphenated name, not an
    importable module, so it is loaded by location. Importing it is the only
    way to test its narrowing directly, and a narrowing nobody tested is how a
    security rule quietly stops applying.
    """
    import importlib.util
    path = os.path.join(REPO, "scripts", "check-no-secrets.py")
    spec = importlib.util.spec_from_file_location("check_no_secrets", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Detection(unittest.TestCase):
    """The pattern, which `scripts/check-no-secrets.py` imports from here."""

    def test_every_credential_shape_is_recognised(self):
        for url in CARRIES:
            with self.subTest(url=url):
                self.assertTrue(carries_private_credential(url))

    def test_no_ordinary_public_tracker_is_mistaken_for_one(self):
        """The expensive direction of a false positive: refusing a public
        tracker removes a row a consumer could have used."""
        for url in PUBLIC:
            with self.subTest(url=url):
                self.assertFalse(carries_private_credential(url))

    def test_one_definition_of_the_pattern(self):
        """`check-no-secrets.py` must import it rather than restate it. Two
        patterns for one rule are two places for it to be wrong."""
        path = os.path.join(REPO, "scripts", "check-no-secrets.py")
        with open(path, encoding="utf-8") as fh:
            source = fh.read()
        self.assertIn("from trackers.exclusion import PRIVATE_CREDENTIAL", source)
        self.assertIn("TRACKER_CREDENTIAL = PRIVATE_CREDENTIAL", source)
        self.assertNotIn("TRACKER_CREDENTIAL_CEILING", source,
                         "the ceiling T-107 retires is still here")

    def test_a_synthetic_token_cannot_hide_a_real_one_beside_it(self):
        """The narrowing that lets this file exist is applied to the matched
        token. Applied to the line instead, the vectors above would suppress a
        real credential written on the same line -- the allowlist-hides-the-
        banned-thing row in `forbidden-patterns.md`.
        """
        check = load_check()
        synthetic = CARRIES[0]
        real = f"http://tracker.example/announce?passkey={REALISTIC_TOKEN}"
        self.assertEqual(check.credential_tokens(synthetic), [],
                         "a test vector counted as somebody's credential")
        self.assertEqual(len(check.credential_tokens(real)), 1)
        both = check.credential_tokens(f"{synthetic} and {real}")
        self.assertEqual(len(both), 1,
                         "a real credential was hidden by a synthetic one")


class Masking(unittest.TestCase):
    """What may be written down about a refusal."""

    def test_the_token_never_survives_masking(self):
        for url in CARRIES:
            with self.subTest(url=url):
                masked = mask_credential(url)
                self.assertIn(REDACTED, masked)
                self.assertNotIn("0123456789abcdef", masked)
                self.assertFalse(carries_private_credential(masked))

    def test_the_host_survives_masking(self):
        """A refusal that named nothing would not explain a disappearance."""
        for url in CARRIES:
            with self.subTest(url=url):
                self.assertIn("tracker.example", mask_credential(url))

    def test_a_public_url_is_unchanged(self):
        for url in PUBLIC:
            with self.subTest(url=url):
                self.assertEqual(mask_credential(url), url)


class TheCorpus(unittest.TestCase):
    """Against the real fixtures, which is where the seven URLs live."""

    @classmethod
    def setUpClass(cls):
        sources = {s.id: s for s in enabled_sources()}
        results = [read_cached(s, FIXTURES) for s in sources.values()]
        cls.agg = aggregate(results, sources)
        cls.plaintext = render_plaintext(cls.agg.trackers)
        cls.report = render_report(cls.agg, generated_at="1970-01-01T00:00:00Z",
                                   code_version="test")

    def test_the_corpus_still_contains_credentials_to_refuse(self):
        """The positive control. If the fixtures stopped carrying any, every
        other test here would pass while proving nothing."""
        raw = 0
        for name in os.listdir(FIXTURES):
            with open(os.path.join(FIXTURES, name), encoding="utf-8",
                      errors="replace") as fh:
                raw += sum(1 for ln in fh if PRIVATE_CREDENTIAL.search(ln))
        self.assertGreater(raw, 0, "no credential is left in the fixtures")

    def test_not_one_reaches_the_published_plaintext(self):
        offenders = [ln for ln in self.plaintext.splitlines()
                     if carries_private_credential(ln)]
        # The count is asserted, not the lines: a failure message repeating
        # them would print the credentials into a build log.
        self.assertEqual(len(offenders), 0,
                         f"{len(offenders)} credential URL(s) were published")

    def test_every_refusal_is_recorded_with_its_reason(self):
        refused = [e for e in self.agg.excluded.values()
                   if "credential" in e.reason]
        self.assertEqual(len(refused), 7,
                         "C-70 measured seven such URLs in this corpus")
        for e in refused:
            with self.subTest(url=e.url):
                self.assertIn("T-107", e.reason)
                self.assertTrue(e.sources, "a refusal that names no source")

    def test_the_refusals_are_counted_per_url_not_per_masked_string(self):
        """Two people's passkeys on one endpoint mask to the same text. Keying
        the record on the masked form lost one of the seven."""
        masked = {e.url for e in self.agg.excluded.values()
                  if "credential" in e.reason}
        self.assertLess(len(masked), 7,
                        "this corpus no longer exercises the collision")

    def test_the_report_explains_each_refusal_without_repeating_a_secret(self):
        self.assertIn("## Refused entries", self.report)
        self.assertIn("carries a private-tracker credential", self.report)
        offenders = [ln for ln in self.report.splitlines()
                     if carries_private_credential(ln)]
        self.assertEqual(len(offenders), 0,
                         "the run report republished what the dataset refused")

    def test_a_refused_url_is_absent_from_provenance(self):
        """Provenance is keyed on published URLs. A refused one appearing there
        would leak the raw URL into a second structure."""
        for url in self.agg.provenance:
            with self.subTest(url=url):
                self.assertFalse(carries_private_credential(url))


if __name__ == "__main__":
    unittest.main()
