# Recorded local L1 pilot

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
