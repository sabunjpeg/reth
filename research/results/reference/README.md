# Recorded local pilots

## Requested rerun and full-validator comparison

The requested follow-up on 12 October 2026 retained two complete campaigns, **1,440 records total and zero failures**:

- `l1-rerun/`: 720 execution-component A/B records from clean source `18bc3d4605c58c0b243a0d60b60fab163f54d0b5`. B remains slightly slower on these tiny in-memory fixtures.
- `disk-pilot/`: 720 complete-payload local disk-backed records from clean source `bd77b1d2f9709f55c3d488de61987167ddd230c0`. Includes valid A/B and B/C, six phantom B/C conditions, representative CAttr controls and a valid BStock/B instrumentation control. All generated payloads and sidecars are retained. Read [interpretation.md](disk-pilot/interpretation.md) and [summary.md](disk-pilot/summary.md).

The L2 binary, lockfile and engine patch match the manifest hashes. Copied inputs, raw records, manifests and receipts are byte-identical to the local campaigns. Analysis revision `4c2ce4fc1afbed60e61d1eb57c535651ccb783b6` clarifies CSV units/ranges without changing benchmark code or raw measurements. Summary columns use milliseconds and MiB where named; raw records use integer nanoseconds and bytes.

```sh
python3 research/analysis/summarize.py research/results/reference/l1-rerun
python3 research/analysis/summarize_disk.py research/results/reference/disk-pilot
```

No database or build artifact is committed. Storage is generated locally by the runner. The L2 parent is a tiny synthetic MDBX state with uncontrolled OS caching, so these observations establish local validation/cancellation behavior rather than physical I/O or production speedups.

## Original L1 pilot

Measured on 12 October 2026 local time on an Apple M4 Pro with 24 GiB RAM. These are development execution-component measurements, not full-validator or disk-I/O evidence.

Producing revision: `d19763ef1f5aaac9a6dffce6d3fd684370821420`, with a clean Git checkout. Upstream: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`. Both campaigns used the same release binary and unchanged upstream execution code. Each manifest records its binary, lockfile, fixture hashes, runtime/toolchain, machine and measurement definitions.

- `pilot/`: four fixtures, 1/2/4 B workers, five warmups, 30 randomized pairs per condition; 720 records and zero failures.
- `counterfree/`: the 64-transaction oracle fixture, four B workers, counters disabled; 60 records and zero failures.

The main pilot shows B slightly slower on these small in-memory fixtures. It does not establish validator performance. The separate counter-disabled control cannot isolate overhead from campaign-order and host noise.

Regenerate the summaries from the repository root:

```sh
python3 research/analysis/summarize.py research/results/reference/pilot --uninstrumented research/results/reference/counterfree
python3 research/analysis/summarize.py research/results/reference/counterfree
```

The ledgers and manifests are copied byte-for-byte from the original campaigns. Summaries use LF line endings. The producing revision predates the commit that adds these results; it identifies the runner and fixtures that actually executed.
