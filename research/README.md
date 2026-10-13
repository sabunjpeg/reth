# Reth BAL A/B and B/C local pilots

This branch contains a working local **L1** pilot. A calls the serial executor; B calls the pinned BAL execution helper. Both use actual EVM execution, identical Amsterdam fork rules and freshly restored in-memory parent state. Transfers are a control; `oracle-reads` invokes a shared storage-reading contract.

The original L1 producing revision leaves execution source under `crates/` unchanged. Its timings exclude the full payload validator, BAL storage prewarming and state-root computation. The branch now also contains the **L2** disk-backed full-validator pilot below, with default-off research hooks and an experimental C sidecar. Historical L1 records retain their original producing revision.

## Disk-backed A/B and B/C

The L2 runner creates a tiny real local MDBX parent database, calculates complete state/receipt/request/BAL commitments, and passes an Ethereum engine payload through `BasicEngineValidator`, `EthBeaconConsensus` and the stock Ethereum payload converter. No networked node or mainnet synchronization is involved.

- A disables BAL parallel execution, parallel state root and batch I/O on the same Amsterdam fork. Ordinary transaction prewarming still follows stock defaults.
- B keeps stock guards, prefetch ordering, parallel execution and state-root processing. Shared research counters observe canonical completion and storage requests.
- C additionally verifies `ReadIndexV1` at canonical completion boundaries and skips pending prefetch after a mismatch. The existing abort guards stop further execution/root work; active EVM/database operations finish.
- CAttr uses the same first-access check with ordinary prefetch cleanup, separating detection from cancellation for representative cases.
- BStock disables observers and provider counters in the research build for an instrumentation control. It is not a separately built unmodified client.

The collector reads actual transaction-result storage records consumed by the pinned REVM BAL builder, including reverted accesses. Received hints and reference traces are not passed to the verifier. The experimental envelope binds block, BAL and index hashes; B ignores the sidecar and C validates it. See `spec.md` for encoding, bounds and semantics. The standard BAL format is unchanged.

Build and reproduce a small correctness case:

```sh
CARGO_BUILD_JOBS=4 cargo test --locked -p bal-research-runner --release
CARGO_BUILD_JOBS=4 cargo test --locked -p reth-engine-tree --features bal-research --release --lib tree::payload_processor -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo build --locked -p bal-research-runner --release
target/release/bal-research-runner validate-disk --fixture research/fixtures/oracle-reads-64.json --workers 4 --phantoms 100 --output research/results/local/reproduction-new
```

For comparable measurements, commit the source and run from a clean checkout:

```sh
python3 research/analysis/run_disk.py research/configs/disk-pilot.json research/results/local/disk-pilot-new
python3 research/analysis/summarize_disk.py research/results/local/disk-pilot-new
```

The matrix uses 64-transaction transfers and shared-oracle fixtures, four execution workers, five warmups and 30 randomized paired repetitions. It compares valid A/B and B/C, plus phantom B/C with 10% and 50% of the remaining permitted BAL item budget and first-access claims at 1, 32 and 65. High-count early/late cases also include CAttr. One valid fixture compares BStock/B. All guards stay enabled; phantom inputs preserve genuine entries and recompute BAL and dependent block commitments.

Each measurement uses a fresh child process/database. Between warmups/replays, the harness recreates the tree acceptance cache, execution cache and state overlay while retaining initialized thread pools. Database preparation, payload construction and producer tracing are untimed. Sidecar verification, full validation and cleanup barriers are timed. Detection is recorded at the decisive canonical mismatch or stock consensus error; completion follows joined execution/prewarm work and drained root/proof queues. The report also includes the validator's return time, so cleanup cost remains visible.

Storage counters count provider requests and skipped storage operations. `phantom_lookups_skipped` counts slots, while whole batch-job skipping is unsupported and `prewarm_jobs_skipped` is null. Rejected partial gas is unknown and null; `fixture_expected_gas_used` is separate. Sidecar bytes count only C/CAttr consumption. Process peak RSS includes initialization/warmups. Physical I/O and cache hit/miss counters remain unsupported. A tiny disk-backed database is likely page-cached; this does not establish cold-state or production performance.

The supported correctness scope includes ordinary transactions, shared reads, reverted reads, no-op writes, out-of-gas before/after access, untouched EIP-2930 hints, pre/post system activity, false early/late indices, canonical ordering and cancellation followed by valid execution. Broad nested calls, creation/selfdestruct, withdrawal workloads and production deployment remain outside this pilot.

The completed local campaign has 720 measurements and zero failures. Valid blocks do not show an A/B or B/C speedup. B completes all phantom prefetch requests before cleanup settles; C reduces high-count completion time through cancellation, while CAttr remains slower than B. Earlier canonical rejection is not an earlier elapsed-time verdict in that case. Read the [measured interpretation](results/reference/disk-pilot/interpretation.md), [full tables](results/reference/disk-pilot/summary.md) and [machine-readable summary](results/reference/disk-pilot/summary.csv). The separate [L1 rerun](results/reference/l1-rerun/summary.md) also retains 720 measurements.

Audit and regenerate the saved summaries without running the benchmark again:

```sh
python3 research/analysis/summarize_disk.py research/results/reference/disk-pilot
python3 research/analysis/summarize.py research/results/reference/l1-rerun
```

## Cloud pilot

The `BAL cloud pilot` workflow runs this same disk-backed configuration on a GitHub-hosted Ubuntu 24.04 VM. Pushes to `research/cloud-bal-pilot` start the job. It selects Rust 1.95.0, runs the Python, candidate and engine-integration tests, builds the release runner, then collects and audits the 720-measurement campaign. Two compilation jobs limit build memory; the experiment still uses four execution workers and the stock BAL prefetch pool.

The artifact contains the campaign manifest, generated fixtures, raw records, completion receipt, summaries, correctness logs and host resource snapshots. Artifacts are retained for seven days; download them to preserve the evidence. The manifest records native Linux CPU/RAM and available CPU affinity, alongside the source, binary, lockfile and fixture hashes. No credentials are needed by the benchmark.

This remains a tiny synthetic-state pilot with uncontrolled OS cache and underlying host sharing. Compare A/B/C within the cloud campaign; the earlier Mac results have a different host and should not be pooled into its timing statistics. A completed Actions job alone is insufficient: audit the downloaded campaign with `summarize_disk.py` and check the correctness logs.

## Setup

Starting revision: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`. Branch: `research/bal-first-access`.

The root manifest requires Rust **1.95**, even though the pinned installation guide still states 1.88. Use the manifest. Native requirements are clang/libclang and pkg-config; the pinned guide recommends `brew install llvm pkg-config` on macOS and `apt-get install libclang-dev pkg-config build-essential` on Ubuntu. Successful compilation on the initial Mac used its existing Command Line Tools and pkg-config; no LLVM/JIT feature is enabled.

From the repository root:

```sh
rustup toolchain install 1.95
rustup override set 1.95
CARGO_BUILD_JOBS=4 cargo test --locked -p reth-engine-tree --release --lib \
  tree::payload_processor::bal::execute::tests -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo test --locked -p bal-research-runner --release
CARGO_BUILD_JOBS=4 cargo build --locked -p bal-research-runner --release
python3 -m unittest discover -s research/analysis -p 'test_*.py'
```

Use fewer build jobs on memory-limited machines. No RPC key, mainnet state, funded real account or networked node is needed.

## Generate fixtures and check equivalence

Small reference fixtures are committed. Regenerate all four with:

```sh
target/release/bal-research-runner generate --workload transfers --tx-count 16 --seed 42 --output research/fixtures/transfers-16.json
target/release/bal-research-runner generate --workload oracle-reads --tx-count 16 --seed 42 --output research/fixtures/oracle-reads-16.json
target/release/bal-research-runner generate --workload transfers --tx-count 64 --seed 42 --output research/fixtures/transfers-64.json
target/release/bal-research-runner generate --workload oracle-reads --tx-count 64 --seed 42 --output research/fixtures/oracle-reads-64.json
target/release/bal-research-runner check --fixture research/fixtures/oracle-reads-64.json --workers 4
```

Fixture files contain starting accounts/storage/code, complete signed transactions, an execution header, canonical BAL RLP and the expected output digest. Synthetic test keys are derived deterministically from seed and transaction number; they are publicly reproducible and have no funding outside the fixture. Regeneration must reproduce the same file bytes.

Reference execution builds the BAL before any timed validation. After stamping its commitment, reference execution runs again to check that stamping did not change the outcome. The generator has not constructed a complete valid payload: in particular, the state-root commitment is not produced by this in-memory pilot. Do not treat executor success as L2 acceptance.

The oracle runtime bytecode is `6000545000`: `PUSH1 0; SLOAD; POP; STOP`. It is hand-assembled directly from those instructions, with no external Solidity build or modeled delay. Its initial slot 0 is 123456. Source/build provenance is also in `fixtures/oracle.asm`.

## Run the pilot

```sh
target/release/bal-research-runner pilot --config research/configs/smoke.json --output research/results/local/smoke-1
target/release/bal-research-runner pilot --config research/configs/pilot.json --output research/results/local/pilot-1
target/release/bal-research-runner pilot --config research/configs/counter-overhead.json --output research/results/local/counterfree-1
python3 research/analysis/summarize.py research/results/local/pilot-1 --uninstrumented research/results/local/counterfree-1
```

Choose a new output directory for each campaign; the runner refuses to overwrite an existing one. The pilot first checks full A/B execution equivalence for every fixture/worker condition. It then records 30 pairs per condition with deterministic randomized A/B order. Each sample runs in a fresh child process, performs five same-variant warmups, restores state and measures one replay. Worker pools are initialized before timing. Warmups initialize pools, not a persistent parent-state cache.

The default campaign contains 4 fixtures × 3 worker counts × 30 pairs = **720 measured records**. A uses one execution worker. B uses 1, 2 or 4. The additional counter-disabled campaign is a representative check on the 64-transaction oracle fixture with four B workers. Counter-disabled execution uses raw InMemoryDB, with no counting wrapper. Its comparison with the main campaign is an estimate subject to run-order and background noise, not a controlled paired instrumentation-overhead experiment.

The runner writes `manifest.json`, `runs.jsonl` and `completion.json`. The analysis script writes `summary.csv`, `summary.md` and `counter-overhead.json`. Local outputs are ignored by Git. `results/reference/` contains small recorded reference outputs, with the code revision that produced them. A recorded run is evidence for that revision, not whichever commit later adds its results.

## Measurement definitions

- Wall time: per-block execution setup through return of the serial or joined BAL helper. Parent-state reconstruction, file loading, process startup, reference generation, runtime/chain-configuration initialization, warmups and output comparisons are outside the timer. The fixed fork configuration is initialized once per process during warmup, as it is in a client. Per-worker database clones, signer recovery, EVM setup, BAL conversion where used, pre/post hooks and BAL reconstruction are inside it.
- CPU: deltas of `RUSAGE_SELF` user/system time over the timed replay, covering all process threads. CPU core-ms is their sum divided by 1,000,000.
- Logical reads: calls to `CountedDb.basic`, `storage`, `code_by_hash` and `block_hash`. They include repeated worker requests. They are neither SLOAD instruction counts nor disk reads. Counter-enabled runs share the same relaxed atomic instrumentation on both adapters.
- Peak memory: per-child `getrusage` process-lifetime high-water mark through measurement, including fixture loading, runtime construction and same-variant warmups. It is not a delta or isolated execution peak. macOS reports bytes; Linux reports KiB, converted to bytes.
- Physical reads and cache hits/misses: unsupported for this pilot and exported as null, with reasons in the manifest. BAL/revm cache accesses that do not reach `CountedDb` are not counted as database calls.
- Throughput: transactions divided by execution seconds, labeled local processing capacity. It is not network capacity.
- Speedup: median of matched A/B wall-time ratios. Below 1 means B is slower. The report includes a seeded percentile bootstrap interval and quartile spread, not p95/p99 claims from this small sample.

The JSON verdict `execution_ok` means execution and saved-output assertions passed. It does not mean the full block was accepted. The check command additionally compares complete bundle state, receipts (including logs), gas, requests and reconstructed BAL directly. The stable digest covers account/storage changes, code, receipts, gas, requests and BAL; transient revert bookkeeping is compared directly but excluded from the digest.

Timeouts, nonzero child exits, panics and output mismatches produce explicit failed records. The campaign returns an error if any failed record exists. Analysis requires a completion receipt, all planned records, complete unique pairs, matching outputs and consistent manifest references; failed pairs remain visible and do not enter performance statistics.

## Limits and next step

The Mac and its background workload, thermal/power behavior and affinity are uncontrolled. In-memory database cloning can dominate tiny fixtures and differs from a real Reth provider. Peak RSS includes warmups. These are development pilot measurements, not paper-ready validator claims.

Use the L2 path for full local validation and phantom-rejection comparisons. Extend its small correctness scope and evaluate larger/held-out workloads, storage conditions and machines before making paper-level generalizations. The local 30-pair campaign remains a development pilot, not a final sample-size plan.
