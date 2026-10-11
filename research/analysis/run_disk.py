#!/usr/bin/env python3
"""Isolated randomized paired full-validator pilot; keeps failed and timed-out samples."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time

PIN = "1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607"

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def command(*args):
    return subprocess.check_output(args, text=True).strip()

def failed_record(fixture, variant, error):
    """Preserve a full nullable measurement schema, including TimeoutExpired's byte stderr."""
    names = {"a": "A", "b": "B", "c": "C", "c-attr": "CAttr", "b-stock": "BStock"}
    stderr = getattr(error, "stderr", None)
    if isinstance(stderr, bytes):
        stderr = stderr.decode("utf-8", errors="replace")
    record = {name: None for name in (
        "t_start_ns", "t_detect_ns", "t_stop_ns", "wall_ns", "validation_return_ns",
        "canonical_index_at_detection", "cpu_user_ns", "cpu_system_ns", "peak_rss_bytes",
        "logical_reads", "cache_hits", "cache_misses", "physical_read_bytes", "gas_used",
        "phantom_lookups_started", "phantom_lookups_completed", "phantom_lookups_skipped",
        "prewarm_jobs_skipped", "execution_workers", "configured_bal_workers", "prewarm_workers",
        "state_root_workers", "output_digest", "index_decode_verify_ns",
    )}
    record.update(
        schema_version=1, measurement_level="L2", verdict="failed", variant=names[variant],
        fixture_id=fixture["execution"]["fixture_id"], seed=fixture["execution"]["seed"],
        phantom_count=fixture["phantom_count"], rejection_reason=str(error),
        rejection_phase="isolated_process", rejection_category="process_failure", stderr=stderr,
        tx_count=len(fixture["execution"]["block"]["body"]["transactions"]),
        bal_bytes=(len(fixture["execution"]["bal_rlp"]) - 2) // 2,
        index_bytes=(len(fixture["index_rlp"]) - 2) // 2 if variant in ("c", "c-attr") else 0,
        database_backend="local_MDBX_Reth_provider_with_RocksDB_and_static_files",
        cache_condition="fresh_execution_cache_and_overlay_each_replay; OS_cache_uncontrolled",
    )
    return record

def run(config_path, output):
    config = json.loads(config_path.read_text())
    assert set(config) == {"schema_version", "binary", "fixtures", "workers", "warmups", "repetitions", "order_seed", "timeout_seconds"}, "unknown or missing configuration field"
    assert config["schema_version"] == 1 and config["fixtures"], "unsupported or empty configuration"
    assert 1 <= config["workers"] <= 64 and 1 <= config["warmups"] <= 100
    assert 1 <= config["repetitions"] <= 10_000 and 1 <= config["timeout_seconds"] <= 3600
    assert not output.exists(), "choose a new output directory"
    assert not command("git", "status", "--porcelain", "--untracked-files=normal"), "freeze and commit code before measurement"
    subprocess.run(["git", "merge-base", "--is-ancestor", PIN, "HEAD"], check=True)
    binary = Path(config["binary"]).resolve()
    output.mkdir(parents=True)
    cases = []
    generation = []
    for workload, source in config["fixtures"].items():
        directory = output / "fixtures" / workload
        subprocess.run([str(binary), "generate-disk", "--fixture", source, "--output", str(directory)], check=True, timeout=60)
        report = json.loads((directory / "generation.json").read_text())
        generation.append({"workload": workload, **report})
        for case in report["cases"]:
            path = directory / case["file"]
            if case["phantom_count"] == 0:
                cases += [("ab", path, ["a", "b"]), ("bc", path, ["b", "c"])]
                if workload == "oracle":
                    cases.append(("instrumentation", path, ["b-stock", "b"]))
            else:
                variants = ["b", "c"]
                tx_count = len(json.loads(path.read_text())["execution"]["block"]["body"]["transactions"])
                if case["fraction_remaining_item_budget"] == 0.5 and case["claim"] in (1, tx_count + 1):
                    variants.append("c-attr")
                cases.append(("bc", path, variants))
    run_id = f"{time.time_ns()}-{os.getpid()}"
    manifest = {
        "schema_version":1, "measurement_level":"L2", "run_id":run_id, "config":config,
        "upstream_sha":PIN, "research_sha":command("git","rev-parse","HEAD"),
        "binary_sha256":sha(binary), "lockfile_sha256":sha("Cargo.lock"),
        "engine_patch_sha256":hashlib.sha256(command("git","diff",PIN,"--","crates").encode()).hexdigest(),
        "host_id":platform.node(), "os":platform.platform(), "rustc":command("rustc","--version"),
        "hardware":command("sysctl","-n","machdep.cpu.brand_string"), "ram_bytes":int(command("sysctl","-n","hw.memsize")),
        "cpu_cores":os.cpu_count(), "cargo":command("cargo","--version"),
        "build_profile":"release; enforced by the measured child", "rustflags":os.environ.get("RUSTFLAGS", ""),
        "git_status":"", "storage":"fresh synthetic Reth TestDB per child in " + tempfile.gettempdir(),
        "environment":"local Mac pilot; CPU affinity, thermals, power and background load uncontrolled",
        "workers":{"BAL_execution":config["workers"], "BAL_prefetch":128, "A_transaction_prewarm":1,
                   "cpu_pool":1, "storage_pool":1, "proof_account_pool":1, "proof_storage_pool":1, "overlay_pool":1},
        "unsupported_counters":{"physical_read_bytes":"no physical I/O accounting", "cache_hits":"not separately instrumented",
                                "cache_misses":"not separately instrumented", "prewarm_jobs_skipped":"per-slot skipping measured; partially skipped batches are not counted as whole jobs",
                                "rejected_gas_used":"partial/failing execution gas is unavailable; fixture_expected_gas_used is supplied separately"},
        "logical_reads":"execution-provider account/storage/code requests only; prewarm requests separately counted; proof requests excluded",
        "phantom_work":"declared phantom storage requests started, returned or skipped before dispatch/between operations; not disk bytes",
        "peak_rss":"per-child process-lifetime high-water mark including setup and warmups; platform getrusage units converted to bytes",
        "fixtures":[{"path":str(path.relative_to(output)),"sha256":sha(path)} for _,path,_ in cases],
        "cases":[{"comparison":comparison,"path":str(path.relative_to(output)),"variants":variants} for comparison,path,variants in cases],
        "generation":generation,
        "timer":"experimental sidecar verification plus full Ethereum engine payload validation and FIFO named-task/pool drain; excludes database/genesis initialization, payload construction, cache reset, warmups, output oracle and process startup",
        "baseline":"pinned stock guards and scheduling; default-off feature adds optional observations. BStock bypasses observers and execution counters in the research build, rather than being a separate unmodified binary. B/C share counters and all stock guards",
        "candidate":"canonical first-storage-read verification; aborts stock execution/root via existing guards; additionally stops subsequent account dispatch and skips pending BAL prefetch between reads; active DB operations finish",
        "scope":"local synthetic tiny parent database, OS cache uncontrolled, no network, no cold-state or physical I/O claims",
    }
    raw = json.dumps(manifest, indent=2).encode()
    (output / "manifest.json").write_bytes(raw)
    manifest_hash = hashlib.sha256(raw).hexdigest()
    failures = []
    sequence = 0
    with (output / "runs.jsonl").open("w") as file:
        for comparison,path,variants in cases:
            fixture = json.loads(path.read_text())
            for repetition in range(config["repetitions"]):
                order = sorted(variants, key=lambda v: hashlib.sha256(f"{config['order_seed']}:{comparison}:{fixture['execution']['fixture_id']}:{repetition}:{v}".encode()).digest())
                pair = f"{comparison}:{fixture['execution']['fixture_id']}:{repetition}"
                for position,variant in enumerate(order):
                    try:
                        result = subprocess.run([str(binary), "disk-sample", "--fixture",str(path), "--variant",variant,"--workers",str(config["workers"]),"--warmups",str(config["warmups"])], capture_output=True, text=True, timeout=config["timeout_seconds"], check=True)
                        record = json.loads(result.stdout)
                    except (subprocess.SubprocessError, ValueError) as error:
                        record = failed_record(fixture, variant, error)
                        failures.append(record)
                    record.update(run_id=f"{run_id}:{sequence}", comparison=comparison,comparison_pair=pair,repetition=repetition,execution_order=position,fixture_path=str(path.relative_to(output)),fixture_sha256=sha(path),manifest_sha256=manifest_hash,host_id=manifest["host_id"])
                    file.write(json.dumps(record)+"\n")
                    file.flush()
                    sequence += 1
            print(f"{comparison}: {fixture['execution']['fixture_id']}: {config['repetitions']} pairs complete", flush=True)
    (output / "completion.json").write_text(json.dumps({"complete":True,"records":sequence,"failed_records":len(failures),"run_id":run_id,"manifest_sha256":manifest_hash},indent=2))
    assert not failures, f"{len(failures)} failed records retained"

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    run(args.config,args.output)
