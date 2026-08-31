"""The execution profile: RULES 15's mechanism, and its refusals.

These tests exist because a profile is exactly the kind of feature that decays
into a comment. Each one asserts a property RULES 15 states normatively, so the
rule and the code cannot drift apart silently.
"""

from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from trackers import vantage                                       # noqa: E402
from trackers.profile import (CI, ENV_VAR, LOCAL, UnknownProfile,  # noqa: E402
                              budget_for, detect)


class TestDetection(unittest.TestCase):
    """RULES 15.1: `ci` is the default, and `local` is opted into."""

    def test_an_empty_environment_is_ci(self):
        """A run that says nothing is `ci`, on any host, including a laptop."""
        self.assertEqual(detect({}), CI)

    def test_an_empty_string_is_ci(self):
        self.assertEqual(detect({ENV_VAR: ""}), CI)
        self.assertEqual(detect({ENV_VAR: "   "}), CI)

    def test_local_must_be_asked_for_explicitly(self):
        self.assertEqual(detect({ENV_VAR: "local"}), LOCAL)
        self.assertEqual(detect({ENV_VAR: "LOCAL"}), LOCAL)
        self.assertEqual(detect({ENV_VAR: " Local "}), LOCAL)

    def test_ci_variables_do_not_escalate_the_profile(self):
        """Detection reads one variable and does not sniff a CI vendor's.

        Inferring `local` from "no CI variables found" would auto-escalate the
        budget on any machine that happens not to set them, which is the
        failure the ordering in RULES 15.1 exists to prevent.
        """
        for env in ({"CI": "true"}, {"GITHUB_ACTIONS": "true"},
                    {"CI": "false"}, {"GITHUB_ACTIONS": "false"},
                    {"HOSTNAME": "my-laptop"}):
            with self.subTest(env=env):
                self.assertEqual(detect(env), CI)

    def test_a_typo_raises_rather_than_falling_back(self):
        """Silently running `ci` when somebody asked for `locl` is quiet wrongness."""
        for bad in ("locl", "LOCALHOST", "prod", "yes", "1"):
            with self.subTest(value=bad):
                with self.assertRaises(UnknownProfile):
                    detect({ENV_VAR: bad})

    def test_the_error_names_the_rule(self):
        with self.assertRaises(UnknownProfile) as caught:
            detect({ENV_VAR: "locl"})
        self.assertIn("section 15", str(caught.exception))


class TestBudgets(unittest.TestCase):
    """RULES 15.2 and 15.4: what each profile permits."""

    def test_ci_is_the_tighter_profile_on_every_axis_that_costs_a_third_party(self):
        ci, local = budget_for(CI), budget_for(LOCAL)
        self.assertLessEqual(ci.max_concurrency, local.max_concurrency)
        self.assertFalse(ci.full_corpus_sweep)
        self.assertTrue(local.full_corpus_sweep)
        self.assertIsNotNone(ci.sample_size)
        self.assertIsNone(local.sample_size)
        self.assertTrue(ci.conditional_requests_required,
                        "a 304 is the cheapest correct answer available (T-104)")

    def test_ci_withholds_the_capabilities_it_measured_it_lacks(self):
        """RULES 15.4: skipped for a measured reason, never absent from the code."""
        ci = budget_for(CI)
        self.assertFalse(ci.attempt_ipv6, "C-04: measured false on both images")
        self.assertFalse(ci.attempt_router_networks, "C-37: no router present")

    def test_local_is_not_permitted_to_be_worse(self):
        """The whole point of RULES 15.4: the local profile never has less reach."""
        ci, local = budget_for(CI), budget_for(LOCAL)
        for field in ("full_corpus_sweep", "attempt_ipv6",
                      "attempt_router_networks"):
            with self.subTest(field=field):
                self.assertGreaterEqual(int(getattr(local, field)),
                                        int(getattr(ci, field)))

    def test_source_snapshots_are_shared_in_both_profiles(self):
        """Fetching one upstream twice in one run is waste, not a budget."""
        for name in (CI, LOCAL):
            with self.subTest(profile=name):
                self.assertTrue(budget_for(name).share_source_snapshots)

    def test_a_budget_records_itself(self):
        record = budget_for(CI).as_record()
        self.assertEqual(record["profile"], CI)
        self.assertIn("attempt_ipv6", record)

    def test_an_unknown_profile_has_no_budget(self):
        with self.assertRaises(UnknownProfile):
            budget_for("prod")


class TestVantageCarriesTheProfile(unittest.TestCase):
    """RULES 3.4: a number without its vantage is the confident-wrongness failure."""

    def test_the_profile_is_in_the_health_record(self):
        v = vantage.detect(budget=budget_for(LOCAL))
        self.assertEqual(v.as_dict()["execution_profile"], LOCAL)

    def test_ci_never_lists_ipv6_even_where_a_route_exists(self):
        """The `ci` profile withholds the family regardless of the routing table.

        A host running these tests may well have IPv6. Under `ci` the probe
        must still not try, and the *reason* must be recorded rather than the
        family silently missing.
        """
        v = vantage.detect(budget=budget_for(CI))
        self.assertNotIn("ipv6", v.ip_families)
        self.assertFalse(v.can_attempt_ipv6)
        if v.ipv6_route_present:
            self.assertIn("withheld by the ci profile", v.ip_families_method,
                          "a withheld family owes the reader its reason")

    def test_a_measured_egress_failure_outranks_the_local_profile(self):
        """RULES 15.4 does not let a profile override a measurement.

        `C-04` measured a runner with an IPv6 stack AND a route that still
        cannot get a packet out. Believing the routing table over that would
        record healthy trackers as dead.
        """
        v = vantage.detect(ipv6_egress=False, budget=budget_for(LOCAL))
        self.assertNotIn("ipv6", v.ip_families)


if __name__ == "__main__":
    unittest.main()
