"""Full-validator ledger integrity and process-failure regression coverage."""

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from run_disk import failed_record
from summarize_disk import load


class DiskLedgerTests(unittest.TestCase):
    def campaign(self, directory):
        fixture = Path(directory) / "fixture.json"
        fixture.write_text("{}")
        fixture_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
        manifest = dict(config=dict(repetitions=1), fixtures=[dict(path="fixture.json", sha256=fixture_hash)],
                        cases=[dict(comparison="ab", path="fixture.json", variants=["a", "b"])])
        raw = json.dumps(manifest).encode()
        digest = hashlib.sha256(raw).hexdigest()
        (Path(directory) / "manifest.json").write_bytes(raw)
        records = []
        for position, variant in enumerate(["A", "B"]):
            records.append(dict(run_id=str(position), comparison_pair="pair", comparison="ab",
                                fixture_path="fixture.json", fixture_sha256=fixture_hash, manifest_sha256=digest,
                                measurement_level="L2", fixture_id="valid", variant=variant, verdict="accepted",
                                t_start_ns=0, t_stop_ns=100, wall_ns=100, validation_return_ns=90,
                                cpu_user_ns=80, cpu_system_ns=10, phantom_count=0, output_digest="output",
                                execution_order=position, block_hash="block", bal_hash="bal", index_hash="index",
                                gas_used=123, configured_bal_workers=4))
        self.write_records(directory, records)
        (Path(directory) / "completion.json").write_text(json.dumps(dict(complete=True, records=2,
                failed_records=0, manifest_sha256=digest)))
        return records

    def write_records(self, directory, records):
        (Path(directory) / "runs.jsonl").write_text("".join(json.dumps(record) + "\n" for record in records))

    def test_complete_pair_and_mismatching_output(self):
        with tempfile.TemporaryDirectory() as directory:
            records = self.campaign(directory)
            self.assertEqual(load(Path(directory))[2], 2)
            records[1]["output_digest"] = "different"
            self.write_records(directory, records)
            with self.assertRaisesRegex(AssertionError, "pair mismatch: output_digest"):
                load(Path(directory))

    def test_manifest_and_fixture_changes_are_detected(self):
        with tempfile.TemporaryDirectory() as directory:
            self.campaign(directory)
            manifest = Path(directory) / "manifest.json"
            manifest.write_bytes(manifest.read_bytes() + b" ")
            with self.assertRaises(AssertionError):
                load(Path(directory))
            self.campaign(directory)
            (Path(directory) / "fixture.json").write_text("changed")
            with self.assertRaises(AssertionError):
                load(Path(directory))

    def test_missing_and_failed_records_block_reporting(self):
        with tempfile.TemporaryDirectory() as directory:
            records = self.campaign(directory)
            self.write_records(directory, records[:1])
            with self.assertRaises(AssertionError):
                load(Path(directory))
            records[1]["verdict"] = "failed"
            self.write_records(directory, records)
            with self.assertRaisesRegex(AssertionError, "failed samples retained"):
                load(Path(directory))

    def test_timeout_stderr_is_serializable_and_unknown_metrics_remain_null(self):
        fixture = dict(phantom_count=1, index_rlp="0x01", execution=dict(fixture_id="test", seed=42,
                       bal_rlp="0xc0", block=dict(body=dict(transactions=[]))))
        error = subprocess.TimeoutExpired("sample", 1, stderr=b"timeout")
        record = json.loads(json.dumps(failed_record(fixture, "c", error)))
        self.assertEqual(record["verdict"], "failed")
        self.assertEqual(record["variant"], "C")
        self.assertEqual(record["stderr"], "timeout")
        self.assertIsNone(record["t_stop_ns"])
        self.assertIsNone(record["physical_read_bytes"])


if __name__ == "__main__":
    unittest.main()
