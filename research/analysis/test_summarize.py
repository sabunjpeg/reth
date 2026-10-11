"""Regression coverage for pair integrity and failed-run accounting."""

import unittest
from summarize import paired_groups


class PairIntegrityTests(unittest.TestCase):
    def record(self, variant, verdict="execution_ok"):
        return dict(comparison_pair="pair", variant=variant, verdict=verdict, fixture_hash="fixture",
                    output_digest="output", gas_used=21000, configured_bal_workers=2, fixture_id="control",
                    execution_order=0 if variant == "A" else 1)

    def test_failed_member_excludes_entire_pair(self):
        self.assertEqual(dict(paired_groups([self.record("A"), self.record("B", "failed")])), {})

    def test_missing_member_is_not_silently_dropped(self):
        with self.assertRaisesRegex(AssertionError, "missing pair member"):
            paired_groups([self.record("A")])

    def test_output_mismatch_blocks_summary(self):
        b = self.record("B")
        b["output_digest"] = "different"
        with self.assertRaisesRegex(AssertionError, "pair mismatch: output_digest"):
            paired_groups([self.record("A"), b])


if __name__ == "__main__":
    unittest.main()
