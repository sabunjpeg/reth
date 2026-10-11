#!/usr/bin/env python3
"""Audit frozen full-validator paired records and report time/CPU/cancellation outcomes."""
import argparse
from collections import defaultdict
import csv
import hashlib
import json
from pathlib import Path
import statistics
from summarize import bootstrap_median_ci, median_field

NAMES = {"a":"A", "b":"B", "c":"C", "c-attr":"CAttr", "b-stock":"BStock"}

def load(directory):
    manifest_bytes = (directory / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    receipt = json.loads((directory / "completion.json").read_text())
    records = [json.loads(line) for line in (directory / "runs.jsonl").read_text().splitlines()]
    expected_count = sum(len(case["variants"]) for case in manifest["cases"]) * manifest["config"]["repetitions"]
    assert receipt["complete"] and len(records) == receipt["records"] == expected_count
    assert len({r["run_id"] for r in records}) == len(records)
    digest = hashlib.sha256(manifest_bytes).hexdigest()
    assert receipt["manifest_sha256"] == digest
    failures = [r for r in records if r["verdict"] == "failed"]
    assert len(failures) == receipt["failed_records"] == 0, "failed samples retained; no performance claim"
    hashes = {entry["path"]:entry["sha256"] for entry in manifest["fixtures"]}
    expected = {(case["comparison"],case["path"]):{NAMES[v] for v in case["variants"]} for case in manifest["cases"]}
    for path, value in hashes.items():
        assert hashlib.sha256((directory/path).read_bytes()).hexdigest() == value
    pairs = defaultdict(dict)
    for r in records:
        assert r["manifest_sha256"] == digest and r["measurement_level"] == "L2"
        assert r["fixture_sha256"] == hashes[r["fixture_path"]]
        assert r["t_start_ns"] == 0 and r["t_stop_ns"] == r["wall_ns"] > 0
        assert r["cpu_user_ns"] >= 0 and r["cpu_system_ns"] >= 0
        assert 0 <= r["validation_return_ns"] <= r["t_stop_ns"]
        if r["phantom_count"]:
            assert r["verdict"] == "rejected"
            assert 0 <= r["t_detect_ns"] <= r["validation_return_ns"]
            if r["variant"] == "B":
                assert r["rejection_category"] == "bal_hash_mismatch" and r["canonical_index_at_detection"] == r["tx_count"] + 1
                assert r["phantom_lookups_completed"] == r["phantom_count"]
            else:
                claim = int(r["fixture_id"].rsplit("-",1)[1])
                assert r["rejection_phase"] == "first_access_verification" and r["canonical_index_at_detection"] == claim
                assert r["phantom_lookups_completed"] + r["phantom_lookups_skipped"] == r["phantom_count"]
            assert r["gas_used"] is None, "rejected partial gas must not be replaced with fixture gas"
        else:
            assert r["verdict"] == "accepted" and r["output_digest"] is not None
        pair = pairs[r["comparison_pair"]]
        assert r["variant"] not in pair
        pair[r["variant"]] = r
    grouped = defaultdict(list)
    for pair in pairs.values():
        first = next(iter(pair.values()))
        assert set(pair) == expected[(first["comparison"],first["fixture_path"])]
        assert {r["execution_order"] for r in pair.values()} == set(range(len(pair)))
        for field in ("fixture_sha256","block_hash","bal_hash","index_hash","output_digest","gas_used","configured_bal_workers"):
            assert len({r[field] for r in pair.values()}) == 1, f"pair mismatch: {field}"
        grouped[(first["comparison"],first["fixture_id"])].append(pair)
    assert len(grouped) == len(manifest["cases"])
    assert all(len(pairs) == manifest["config"]["repetitions"] for pairs in grouped.values())
    return manifest,grouped,len(records)

def summarize(grouped):
    rows=[]
    for (comparison,fixture),pairs in sorted(grouped.items()):
        baseline = "A" if comparison=="ab" else "BStock" if comparison=="instrumentation" else "B"
        for target in sorted(set(pairs[0])-{baseline}):
            left=[pair[baseline] for pair in pairs]
            right=[pair[target] for pair in pairs]
            ratios=[pair[baseline]["wall_ns"]/pair[target]["wall_ns"] for pair in pairs]
            low,high=bootstrap_median_ci(ratios)
            row=dict(comparison=comparison,fixture=fixture,baseline=baseline,target=target,pairs=len(pairs),median_paired_speedup=statistics.median(ratios),speedup_ci_low=low,speedup_ci_high=high)
            for label, samples in [("baseline",left),("target",right)]:
                for field,column,scale in [("wall_ns","wall_ms",1e6),("t_detect_ns","detect_ms",1e6),("validation_return_ns","validation_return_ms",1e6),("peak_rss_bytes","peak_rss_mib",2**20),("logical_reads","logical_reads",1),("phantom_lookups_completed","phantom_lookups_completed",1),("phantom_lookups_skipped","phantom_lookups_skipped",1),("index_decode_verify_ns","index_decode_verify_ms",1e6),("index_bytes","index_bytes",1)]:
                    row[f"{label}_{column}"]=median_field(samples,field,scale)
                for field in ("phantom_lookups_completed","phantom_lookups_skipped"):
                    values=[r[field] for r in samples if r[field] is not None]
                    row[f"{label}_{field}_min"]=min(values) if values else None
                    row[f"{label}_{field}_max"]=max(values) if values else None
                row[f"{label}_cpu_ms"]=statistics.median((r["cpu_user_ns"]+r["cpu_system_ns"])/1e6 for r in samples)
                row[f"{label}_cleanup_ms"]=None if samples[0]["t_detect_ns"] is None else statistics.median((r["t_stop_ns"]-r["t_detect_ns"])/1e6 for r in samples)
            rows.append(row)
    return rows

def render(manifest,rows,count):
    lines=["# Local full-validator A/B and B/C pilot","",f"Producing revision: `{manifest['research_sha']}`; upstream: `{manifest['upstream_sha']}`.","",f"{count} isolated measurements; {manifest['config']['warmups']} warmups per child and {manifest['config']['repetitions']} randomized paired repetitions per cell. No failed, timed-out or unexpected-verdict samples. Every accepted output matches a serial MDBX reference digest (state, contracts, receipts, gas, requests and BAL); the full engine also validates state and receipt roots.","","A disables BAL execution/root/batch-I/O switches. B retains the pinned stock guards, execution, prewarm ordering and root pipeline. C verifies experimental first-read metadata at canonical completion and cancels pending BAL prefetch between operations; CAttr verifies identically but uses stock prefetch cleanup. BStock disables research observers and execution-provider counters in the same research build; it is not a separate unmodified binary.","","| Comparison | Fixture | Baseline ms | Candidate ms | Paired speedup (95% CI) | Baseline CPU ms | Candidate CPU ms |","| --- | --- | ---: | ---: | --- | ---: | ---: | ---: |"]
    for r in rows:
        lines.append(f"| {r['baseline']}/{r['target']} | {r['fixture']} | {r['baseline_wall_ms']:.3f} | {r['target_wall_ms']:.3f} | {r['median_paired_speedup']:.2f}x ({r['speedup_ci_low']:.2f}–{r['speedup_ci_high']:.2f}) | {r['baseline_cpu_ms']:.3f} | {r['target_cpu_ms']:.3f} |")
    lines += ["","A ratio above one means the candidate releases measured tasks sooner. The interval bootstraps paired ratios (2,000 resamples, seed 42); it does not control host drift, thermals or background load.","","| Phantom fixture | Variant | Detection ms | Cleanup after detection ms | Completed phantom lookups | Skipped phantom lookups | Index verification ms |","| --- | --- | ---: | ---: | ---: | ---: | ---: |"]
    seen=set()
    for r in rows:
        if r['baseline_detect_ms'] is None:continue
        for label,variant in [("baseline",r['baseline']),("target",r['target'])]:
            if (r['fixture'],variant) in seen:continue
            seen.add((r['fixture'],variant))
            decode=r[f'{label}_index_decode_verify_ms']
            lines.append(f"| {r['fixture']} | {variant} | {r[f'{label}_detect_ms']:.3f} | {r[f'{label}_cleanup_ms']:.3f} | {r[f'{label}_phantom_lookups_completed']:g} | {r[f'{label}_phantom_lookups_skipped']:g} | {decode:.3f} |" if decode is not None else f"| {r['fixture']} | {variant} | {r[f'{label}_detect_ms']:.3f} | {r[f'{label}_cleanup_ms']:.3f} | {r[f'{label}_phantom_lookups_completed']:g} | {r[f'{label}_phantom_lookups_skipped']:g} | — |")
    lines += ["","Lookup counts above are medians. Cancellation depends on scheduling; the CSV also reports minimum/maximum completed and skipped lookups. Earlier canonical rejection need not mean an earlier wall-clock error: sidecar processing adds time. Compare detection, completion and CPU separately."]
    lines += ["","Stock B rejects correctly committed phantom reads with `BlockAccessListHashMismatch` after index 65. C rejects at the declared first-access index (1, 32 or 65). Counts are provider requests; completed includes requests returning absent values. They do not measure physical disk bytes. Execution logical-read counts exclude prewarm/proof providers and vary with cache races. Peak RSS is the entire process high-water mark including setup/warmups. Physical I/O, cache hit/miss rates and cold-state behavior are unmeasured.","","The parent is a small synthetic MDBX database on a local Mac. Sidecar verification and named-task/pool drain are inside timing. Database setup, payload construction, fresh execution-cache/overlay setup, producer trace generation and output-oracle checks are outside timing. Named queues are drained through FIFO barriers; existing execution guards join workers, BAL prewarm joins its 128 workers, proof/overlay pools are cleared. Active database reads finish. There is no cancellation of an already executing EVM instruction stream.","","Phantom sizes use 10% and 50% of the remaining pinned `gas_limit / ITEM_COST` item allowance. Standard BAL and block commitments are recomputed, leaving transaction/state commitments truthful. Larger metadata and late claims can erase or reverse early-rejection gains. Producer trace and encoding times appear separately in the manifest; these prototype trace timings include an independent serial execution and do not estimate an integrated block-builder overhead."]
    return "\n".join(lines)+"\n"

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path)
    args=parser.parse_args()
    manifest,groups,count=load(args.directory)
    rows=summarize(groups)
    with (args.directory/"summary.csv").open("w",newline="") as file:
        writer=csv.DictWriter(file,fieldnames=list(rows[0]),lineterminator="\n")
        writer.writeheader();writer.writerows(rows)
    (args.directory/"summary.md").write_text(render(manifest,rows,count))
    print(args.directory/"summary.md")
