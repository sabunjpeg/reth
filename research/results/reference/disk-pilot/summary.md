# Local full-validator A/B and B/C pilot

Producing revision: `bd77b1d2f9709f55c3d488de61987167ddd230c0`; upstream: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

720 isolated measurements; 5 warmups per child and 30 randomized paired repetitions per cell. No failed, timed-out or unexpected-verdict samples. Every accepted output matches a serial MDBX reference digest (state, contracts, receipts, gas, requests and BAL); the full engine also validates state and receipt roots.

A disables BAL execution/root/batch-I/O switches. B retains the pinned stock guards, execution, prewarm ordering and root pipeline. C verifies experimental first-read metadata at canonical completion and cancels pending BAL prefetch between operations; CAttr verifies identically but uses stock prefetch cleanup. BStock disables research observers and execution-provider counters in the same research build; it is not a separate unmodified binary.

| Comparison | Fixture | Baseline ms | Candidate ms | Paired speedup (95% CI) | Baseline CPU ms | Candidate CPU ms |
| --- | --- | ---: | ---: | --- | ---: | ---: | ---: |
| A/B | oracle-reads-64-seed-42 | 10.739 | 11.333 | 0.96x (0.92–0.99) | 14.160 | 27.564 |
| A/B | transfers-64-seed-42 | 11.303 | 11.616 | 0.97x (0.95–0.99) | 15.002 | 35.554 |
| B/C | oracle-reads-64-seed-42 | 11.064 | 11.476 | 0.98x (0.96–1.01) | 26.518 | 27.814 |
| B/C | oracle-reads-64-seed-42-phantoms-1492-claim-1 | 12.054 | 11.008 | 1.07x (1.03–1.11) | 33.021 | 21.476 |
| B/C | oracle-reads-64-seed-42-phantoms-1492-claim-32 | 11.829 | 10.977 | 1.07x (1.01–1.10) | 33.743 | 23.455 |
| B/C | oracle-reads-64-seed-42-phantoms-1492-claim-65 | 12.061 | 11.133 | 1.08x (1.03–1.12) | 37.094 | 25.137 |
| B/C | oracle-reads-64-seed-42-phantoms-7460-claim-1 | 14.824 | 12.018 | 1.21x (1.14–1.25) | 68.116 | 26.686 |
| B/CAttr | oracle-reads-64-seed-42-phantoms-7460-claim-1 | 14.824 | 15.586 | 0.94x (0.91–0.96) | 68.116 | 66.903 |
| B/C | oracle-reads-64-seed-42-phantoms-7460-claim-32 | 14.586 | 11.939 | 1.22x (1.13–1.25) | 60.145 | 28.238 |
| B/C | oracle-reads-64-seed-42-phantoms-7460-claim-65 | 14.973 | 11.913 | 1.25x (1.23–1.29) | 67.752 | 30.879 |
| B/CAttr | oracle-reads-64-seed-42-phantoms-7460-claim-65 | 14.973 | 15.457 | 0.96x (0.94–1.00) | 67.752 | 67.484 |
| B/C | transfers-64-seed-42 | 11.594 | 11.607 | 1.00x (0.97–1.02) | 31.768 | 32.933 |
| BStock/B | oracle-reads-64-seed-42 | 11.364 | 11.381 | 0.98x (0.97–1.04) | 28.532 | 28.093 |

A ratio above one means the candidate releases measured tasks sooner. The interval bootstraps paired ratios (2,000 resamples, seed 42); it does not control host drift, thermals or background load.

| Phantom fixture | Variant | Detection ms | Cleanup after detection ms | Completed phantom lookups | Skipped phantom lookups | Index verification ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| oracle-reads-64-seed-42-phantoms-1492-claim-1 | B | 9.838 | 2.099 | 1492 | 0 | — |
| oracle-reads-64-seed-42-phantoms-1492-claim-1 | C | 9.710 | 1.185 | 0 | 1492 | 0.152 |
| oracle-reads-64-seed-42-phantoms-1492-claim-32 | B | 9.711 | 2.241 | 1492 | 0 | — |
| oracle-reads-64-seed-42-phantoms-1492-claim-32 | C | 9.741 | 1.300 | 0 | 1492 | 0.154 |
| oracle-reads-64-seed-42-phantoms-1492-claim-65 | B | 9.726 | 2.242 | 1492 | 0 | — |
| oracle-reads-64-seed-42-phantoms-1492-claim-65 | C | 9.783 | 1.402 | 0 | 1492 | 0.153 |
| oracle-reads-64-seed-42-phantoms-7460-claim-1 | B | 9.860 | 5.010 | 7460 | 0 | — |
| oracle-reads-64-seed-42-phantoms-7460-claim-1 | C | 10.403 | 1.438 | 0 | 7460 | 0.666 |
| oracle-reads-64-seed-42-phantoms-7460-claim-1 | CAttr | 10.451 | 5.306 | 7460 | 0 | 0.665 |
| oracle-reads-64-seed-42-phantoms-7460-claim-32 | B | 10.014 | 4.690 | 7460 | 0 | — |
| oracle-reads-64-seed-42-phantoms-7460-claim-32 | C | 10.522 | 1.384 | 0 | 7460 | 0.653 |
| oracle-reads-64-seed-42-phantoms-7460-claim-65 | B | 9.896 | 5.129 | 7460 | 0 | — |
| oracle-reads-64-seed-42-phantoms-7460-claim-65 | C | 10.404 | 1.633 | 0 | 7460 | 0.662 |
| oracle-reads-64-seed-42-phantoms-7460-claim-65 | CAttr | 10.393 | 5.149 | 7460 | 0 | 0.659 |

Lookup counts above are medians. Cancellation depends on scheduling; the CSV also reports minimum/maximum completed and skipped lookups. Earlier canonical rejection need not mean an earlier wall-clock error: sidecar processing adds time. Compare detection, completion and CPU separately.

Stock B rejects correctly committed phantom reads with `BlockAccessListHashMismatch` after index 65. C rejects at the declared first-access index (1, 32 or 65). Counts are provider requests; completed includes requests returning absent values. They do not measure physical disk bytes. Execution logical-read counts exclude prewarm/proof providers and vary with cache races. Peak RSS is the entire process high-water mark including setup/warmups. Physical I/O, cache hit/miss rates and cold-state behavior are unmeasured.

The parent is a small synthetic MDBX database on a local Mac. Sidecar verification and named-task/pool drain are inside timing. Database setup, payload construction, fresh execution-cache/overlay setup, producer trace generation and output-oracle checks are outside timing. Named queues are drained through FIFO barriers; existing execution guards join workers, BAL prewarm joins its 128 workers, proof/overlay pools are cleared. Active database reads finish. There is no cancellation of an already executing EVM instruction stream.

Phantom sizes use 10% and 50% of the remaining pinned `gas_limit / ITEM_COST` item allowance. Standard BAL and block commitments are recomputed, leaving transaction/state commitments truthful. Larger metadata and late claims can erase or reverse early-rejection gains. Producer trace and encoding times appear separately in the manifest; these prototype trace timings include an independent serial execution and do not estimate an integrated block-builder overhead.
