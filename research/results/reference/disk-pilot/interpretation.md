# Local A/B, phantom reproduction and B/C results

Measured on 12 October 2026 local time. **720 full-validator measurements, zero failures**, five warmups and 30 randomized paired repetitions per condition. All accepted blocks match the serial reference outputs and complete payload commitment checks. The separate [L1 rerun](../l1-rerun/summary.md) also has 720 measurements and zero failures.

Producing source: `bd77b1d2f9709f55c3d488de61987167ddd230c0`, a clean checkout based on upstream `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`. The release-binary, lockfile, engine-patch and input hashes were audited. Full tables are in [summary.md](summary.md), all measured records in [runs.jsonl](runs.jsonl), configuration/hardware/hashes in [manifest.json](manifest.json), and completion accounting in [completion.json](completion.json).

## Environments

- **A:** the same Amsterdam fork with BAL parallel execution, parallel state root and batch I/O disabled; standard BAL validation and ordinary transaction prewarming remain enabled.
- **B:** pinned BAL execution, root processing, prefetch ordering and all guards, with shared research observations/counters.
- **C:** B plus experimental first-access sidecar verification and cancellation of pending BAL prefetch. Active EVM/database operations finish.
- **CAttr:** the same verification as C, with ordinary prefetch cleanup. This control separates attribution from prefetch cancellation.

This is local Reth execution and complete engine payload validation against a real synthetic MDBX parent, not a running network. B/C use four execution workers and the stock 128-worker BAL prefetch pool. Each sample is a separate process; execution cache/tree overlay reset before every replay. Timed completion includes sidecar checks, full validation and task cleanup. Initialization, payload construction and producer tracing are outside timing.

## Valid blocks: no measured optimization benefit

These are medians for 64-transaction blocks. Each comparison has its own randomized B samples; the B columns should not be combined across comparisons.

| Comparison | Workload | Baseline ms | Target ms | Paired speedup, 95% interval |
| --- | --- | ---: | ---: | --- |
| A/B | Shared oracle reads | 10.739 | 11.333 | 0.96x, 0.92–0.99 |
| A/B | Transfers | 11.303 | 11.616 | 0.97x, 0.95–0.99 |
| B/C | Shared oracle reads | 11.064 | 11.476 | 0.98x, 0.96–1.01 |
| B/C | Transfers | 11.594 | 11.607 | 1.00x, 0.97–1.02 |

B is slightly slower than A here. The B/C intervals include parity; C does not show a valid-block speedup. These small blocks and a tiny likely cached parent do not establish how BAL optimization behaves on large production state. A still validates BALs, so A/B is the optimization bundle comparison on a common fork.

## Problem reproduced in B

The generator adds 1,492 or 7,460 read-only keys that actual execution never touches. It retains genuine entries, stays within the pinned item limit, and recomputes BAL/block commitments. This prevents a cheap bad-hash or item-limit rejection from substituting for the semantic reproduction. Standard transaction/state outputs remain truthful.

**Every B sample rejects with `BlockAccessListHashMismatch` after all 64 transactions and post-execution activity, at canonical index 65. B also completes every phantom prefetch lookup before cleanup settles.** The reproduced problem is wasted provider-request work around a correctly rejected block; it is not acceptance of an invalid block.

C rejects at the false declared index: transaction 1, transaction 32, or post-execution index 65. The last case supplies no earlier canonical boundary than B's final check.

## B/C: pending-work cancellation reduces completion cost

| Phantom keys | Claimed index | B completion ms | C completion ms | Paired speedup, 95% interval | B CPU core-ms | C CPU core-ms |
| ---: | ---: | ---: | ---: | --- | ---: | ---: |
| 1,492 | 1 | 12.054 | 11.008 | 1.07x, 1.03–1.11 | 33.021 | 21.476 |
| 1,492 | 32 | 11.829 | 10.977 | 1.07x, 1.01–1.10 | 33.743 | 23.455 |
| 1,492 | 65 | 12.061 | 11.133 | 1.08x, 1.03–1.12 | 37.094 | 25.137 |
| 7,460 | 1 | 14.824 | 12.018 | 1.21x, 1.14–1.25 | 68.116 | 26.686 |
| 7,460 | 32 | 14.586 | 11.939 | 1.22x, 1.13–1.25 | 60.145 | 28.238 |
| 7,460 | 65 | 14.973 | 11.913 | 1.25x, 1.23–1.29 | 67.752 | 30.879 |

For 7,460 phantoms claiming index 1, C reduces median completion time by about **19%** and median CPU work by about **61%**. The paired speedup is a separate median of matched ratios and need not equal the ratio of campaign medians.

The causal control gives a narrower conclusion than "early rejection is faster":

| Same high-count early fixture | B | CAttr | C |
| --- | ---: | ---: | ---: |
| Canonical rejection index | 65 | 1 | 1 |
| Median detection ms | 9.860 | 10.451 | 10.403 |
| Median cleanup after detection ms | 5.010 | 5.306 | 1.438 |
| Median completion ms | 14.824 | 15.586 | 12.018 |
| Median completed phantom lookups | 7,460 | 7,460 | 0 |
| Median CPU core-ms | 68.116 | 66.903 | 26.686 |

**C detects at an earlier canonical transaction but later in elapsed time**, because metadata processing costs time. Attribution alone is slower than B: CAttr speedup 0.94x (0.91–0.96). The measured benefit appears when C skips pending prefetch work and settles cleanup sooner. High-count C/CAttr process a 193,733-byte sidecar; median decode/commitment/structural verification is about 0.66 ms, followed by canonical verification during execution.

Cancellation is scheduling-dependent. C's median completed phantom count is zero in every condition, but high-count early samples range from **0–3,752** completed requests, and middle samples from **0–7,105**. Late samples complete zero in this campaign. These results show that pending work remained at detection on this host; they do not guarantee zero wasted work on a different schedule or storage system.

## Controls and limits

The observer-disabled BStock/B valid control has a paired speedup of 0.98x with interval 0.97–1.04. It does not resolve a significant instrumentation penalty in this sample, and it is a control within the research build rather than a separately compiled unmodified client. Feature-disabled stock executor tests also pass. Existing structural, commitment and item/gas safeguards stay enabled. A stronger periodic gas-feasibility control is explicitly deferred pending sound system-activity accounting.

Physical read bytes and cache hit/miss rates are unmeasured. Lookup counters represent provider requests, including absent-value returns, and cannot be read as physical disk operations. Peak RSS includes process setup and warmups. The Mac's CPU affinity, load, power and thermals are uncontrolled. Intervals bootstrap 30 paired ratios and do not address these external conditions. Sidecar producer trace timing includes an independent serial execution, so it is not an integrated block-builder overhead estimate.

C is an experimental sidecar/verifier and cancellation mechanism. Its envelope is not an Ethereum consensus commitment. Correctness coverage is restricted to the documented ordinary-transaction scope; nested calls, creation/selfdestruct, withdrawal workloads, larger populated state, held-out workloads and controlled cache/storage conditions remain for broader research.

## Reproduce

From the Reth repository root, audit the saved records and regenerate the tables:

```sh
python3 research/analysis/summarize_disk.py research/results/reference/disk-pilot
python3 -m unittest discover -s research/analysis -p 'test_*.py'
```

To collect a new campaign from a clean committed checkout, rebuild the release runner and choose a new directory:

```sh
CARGO_BUILD_JOBS=4 cargo build --locked -p bal-research-runner --release
python3 research/analysis/run_disk.py research/configs/disk-pilot.json research/results/local/disk-pilot-new
python3 research/analysis/summarize_disk.py research/results/local/disk-pilot-new
```

The source that produced these records is identified above; subsequent presentation/results commits do not change the measured runner. Rust checks passed before measurement: 13 runner tests, 55 engine tests with research hooks and 19 stock executor tests with the feature disabled. All eight current Python analysis tests pass.
