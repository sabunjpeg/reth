# Reth BAL A/B execution pilot

This branch contains a working local **L1** pilot. A calls the serial executor; B calls the pinned BAL execution helper. Both use actual EVM execution, identical Amsterdam fork rules and freshly restored in-memory parent state. Transfers are a control; `oracle-reads` invokes a shared storage-reading contract.

The execution source under `crates/` is unchanged. The root Cargo workspace and lockfile add only the research runner. This pilot does not invoke the full payload validator, BAL storage prewarming or state-root computation. Its timings must not be presented as complete validator latency or a measurement of the full BAL optimization bundle. The full A/B engine configuration and disk-backed validator path are the next milestone, M2.

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

M2 requires a disk-backed parent snapshot, correctly committed complete payload, the full payload-validator path, fresh acceptance caches, explicit prewarm/state-root settings, and joined block-attributed background work. Only that path can compare the full A/B bundle and reproduce semantic phantom-read rejection in stock B. First-access metadata, cancellation and C remain later milestones.
