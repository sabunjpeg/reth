#!/usr/bin/env python3
"""Validate a run ledger and summarize paired L1 measurements using Python's standard library."""

import argparse
import csv
import json
import random
import statistics
from collections import defaultdict
from pathlib import Path


def load_run(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    completion = json.loads((directory / "completion.json").read_text())
    records = [json.loads(line) for line in (directory / "runs.jsonl").read_text().splitlines()]
    assert completion["complete"], "campaign incomplete"
    assert len(records) == completion["records"], "record count differs from completion receipt"
    config = manifest["config"]
    assert len(records) == len(config["fixtures"]) * len(config["workers"]) * config["repetitions"] * 2, "missing planned records"
    assert len({record["run_id"] for record in records}) == len(records), "duplicate run IDs"
    failed = [record for record in records if record["verdict"] != "execution_ok"]
    assert len(failed) == completion["failed_records"], "failed records are not accounted for"
    for record in records:
        assert record["manifest_hash"] == completion["manifest_hash"], "mixed run manifests"
        assert record["measurement_level"] == "L1", "this summary only supports L1"
        if record["verdict"] == "execution_ok":
            assert record["wall_ns"] > 0
            assert record["t_stop_ns"] - record["t_start_ns"] == record["wall_ns"]
            assert record["cpu_user_ns"] >= 0 and record["cpu_system_ns"] >= 0
    return manifest, records, failed


def paired_groups(records):
    pairs = defaultdict(dict)
    for record in records:
        pair = pairs[record["comparison_pair"]]
        assert record["variant"] not in pair, "duplicate variant within pair"
        pair[record["variant"]] = record
    groups = defaultdict(list)
    for pair in pairs.values():
        assert set(pair) == {"A", "B"}, "missing pair member"
        if any(record["verdict"] != "execution_ok" for record in pair.values()):
            continue
        a, b = pair["A"], pair["B"]
        for field in ("fixture_hash", "output_digest", "gas_used", "configured_bal_workers"):
            assert a[field] == b[field], f"pair mismatch: {field}"
        assert {a["execution_order"], b["execution_order"]} == {0, 1}, "invalid randomized order"
        groups[(a["fixture_id"], a["configured_bal_workers"])].append((a, b))
    return groups


def median_field(records, field, scale=1):
    values = [record.get(field) for record in records]
    return None if any(value is None for value in values) else statistics.median(values) / scale


def bootstrap_median_ci(values, seed=42):
    rng = random.Random(seed)
    medians = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(2000))
    return medians[49], medians[1949]


def summarize(records):
    rows = []
    for (fixture, workers), pairs in sorted(paired_groups(records).items()):
        a = [pair[0] for pair in pairs]
        b = [pair[1] for pair in pairs]
        ratios = [first["wall_ns"] / second["wall_ns"] for first, second in pairs]
        ci_low, ci_high = bootstrap_median_ci(ratios)
        row = {
            "fixture": fixture, "B_workers": workers, "pairs": len(pairs),
            "median_paired_speedup": statistics.median(ratios),
            "speedup_ci_low": ci_low, "speedup_ci_high": ci_high,
        }
        for variant, samples in (("A", a), ("B", b)):
            row[f"{variant}_median_ms"] = median_field(samples, "wall_ns", 1e6)
            times = sorted(sample["wall_ns"] / 1e6 for sample in samples)
            row[f"{variant}_q1_ms"] = times[(len(times) - 1) // 4]
            row[f"{variant}_q3_ms"] = times[3 * (len(times) - 1) // 4]
            row[f"{variant}_cpu_ms"] = statistics.median(
                (sample["cpu_user_ns"] + sample["cpu_system_ns"]) / 1e6 for sample in samples
            )
            row[f"{variant}_logical_reads"] = median_field(samples, "logical_reads")
            row[f"{variant}_storage_reads"] = median_field(samples, "storage_reads")
            row[f"{variant}_process_peak_rss_mib"] = median_field(samples, "peak_rss_bytes", 2**20)
            row[f"{variant}_local_tx_per_s"] = statistics.median(
                sample["tx_count"] * 1e9 / sample["wall_ns"] for sample in samples
            )
        rows.append(row)
    return rows


def render(manifest, rows, failed, overhead):
    lines = [
        "# Local L1 A/B execution pilot", "",
        f"Research revision: `{manifest['research_sha']}`. Upstream: `{manifest['upstream_sha']}`.", "",
        "These are execution-component measurements with in-memory state. They exclude storage prewarming, "
        "state-root tasks and complete payload validation. They do not establish full-validator or physical disk-I/O performance.", "",
        f"Failed records retained: **{len(failed)}**. Only complete successful pairs enter the statistics.", "",
        "| Fixture | B workers | Pairs | A median ms | B median ms | Paired speedup (95% CI) | A CPU ms | B CPU ms |",
        "| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |",
    ]
    for row in rows:
        lines.append(
            f"| {row['fixture']} | {row['B_workers']} | {row['pairs']} | "
            f"{row['A_median_ms']:.3f} | {row['B_median_ms']:.3f} | "
            f"{row['median_paired_speedup']:.2f}x ({row['speedup_ci_low']:.2f}–{row['speedup_ci_high']:.2f}) | "
            f"{row['A_cpu_ms']:.3f} | {row['B_cpu_ms']:.3f} |"
        )
    lines += ["", "A speedup below 1 means B was slower. The interval bootstraps the median of paired A/B ratios "
              "with 2,000 resamples (seed 42); it does not account for host drift or background load.", "",
              "CSV includes quartile spread, database-call counts, process peak RSS and derived local transaction capacity. "
              "Process peak RSS includes fixture loading, initialization and same-variant warmups; it is not isolated execution memory. "
              "Physical I/O and cache hit/miss counters are unsupported and remain null. No tail-latency claim is made."]
    if overhead:
        lines += ["", "## Counter instrumentation check", "",
                  "Separate counter-enabled and counter-disabled campaigns on the same fixture/configuration. "
                  "This is a pilot estimate, subject to campaign-order and host noise; it is not a controlled paired overhead result.", "",
                  "| Fixture | B workers | Variant | Median latency change with counters |",
                  "| --- | ---: | --- | ---: |"]
        for row in overhead:
            lines.append(f"| {row['fixture']} | {row['workers']} | {row['variant']} | {row['overhead_pct']:+.2f}% |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--uninstrumented", type=Path)
    args = parser.parse_args()
    manifest, records, failed = load_run(args.directory)
    rows = summarize(records)
    overhead = []
    if args.uninstrumented:
        other_manifest, other_records, other_failed = load_run(args.uninstrumented)
        assert not other_failed, "uninstrumented campaign failed"
        assert manifest["config"]["collect_counters"] and not other_manifest["config"]["collect_counters"]
        for field in ("binary_keccak256", "research_sha", "host_id", "lockfile_keccak256"):
            assert manifest[field] == other_manifest[field], f"instrumentation control differs: {field}"
        for field in ("warmups", "repetitions", "order_seed"):
            assert manifest["config"][field] == other_manifest["config"][field]
        fixture_hashes = {entry["path"]: entry["keccak256"] for entry in manifest["fixtures"]}
        assert all(fixture_hashes.get(entry["path"]) == entry["keccak256"]
                   for entry in other_manifest["fixtures"]), "instrumentation control uses different fixture bytes"
        counterfree = {(row["fixture"], row["B_workers"]): row for row in summarize(other_records)}
        for row in rows:
            if (row["fixture"], row["B_workers"]) in counterfree:
                reference = counterfree[(row["fixture"], row["B_workers"])]
                for variant in ("A", "B"):
                    overhead.append({"fixture": row["fixture"], "workers": row["B_workers"], "variant": variant,
                                     "overhead_pct": 100 * (row[f"{variant}_median_ms"] / reference[f"{variant}_median_ms"] - 1)})
    assert rows, "no complete successful pairs"
    with (args.directory / "summary.csv").open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    (args.directory / "summary.md").write_text(render(manifest, rows, failed, overhead))
    (args.directory / "counter-overhead.json").write_text(json.dumps(overhead, indent=2) + "\n")
    print(args.directory / "summary.md")
    if failed:
        raise SystemExit("campaign contains failed records; summary is incomplete evidence")


if __name__ == "__main__":
    main()
