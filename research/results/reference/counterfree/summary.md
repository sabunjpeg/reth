# Local L1 A/B execution pilot

Research revision: `d19763ef1f5aaac9a6dffce6d3fd684370821420`. Upstream: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

These are execution-component measurements with in-memory state. They exclude storage prewarming, state-root tasks and complete payload validation. They do not establish full-validator or physical disk-I/O performance.

Failed records retained: **0**. Only complete successful pairs enter the statistics.

| Fixture | B workers | Pairs | A median ms | B median ms | Paired speedup (95% CI) | A CPU ms | B CPU ms |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| oracle-reads-64-seed-42 | 4 | 30 | 1.142 | 1.170 | 0.97x (0.94–0.99) | 1.147 | 1.415 |

A speedup below 1 means B was slower. The interval bootstraps the median of paired A/B ratios with 2,000 resamples (seed 42); it does not account for host drift or background load.

CSV includes quartile spread, database-call counts, process peak RSS and derived local transaction capacity. Process peak RSS includes fixture loading, initialization and same-variant warmups; it is not isolated execution memory. Physical I/O and cache hit/miss counters are unsupported and remain null. No tail-latency claim is made.
