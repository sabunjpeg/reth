# Local L1 A/B execution pilot

Research revision: `d19763ef1f5aaac9a6dffce6d3fd684370821420`. Upstream: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

These are execution-component measurements with in-memory state. They exclude storage prewarming, state-root tasks and complete payload validation. They do not establish full-validator or physical disk-I/O performance.

Failed records retained: **0**. Only complete successful pairs enter the statistics.

| Fixture | B workers | Pairs | A median ms | B median ms | Paired speedup (95% CI) | A CPU ms | B CPU ms |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| oracle-reads-16-seed-42 | 1 | 30 | 0.287 | 0.304 | 0.94x (0.93–0.97) | 0.290 | 0.329 |
| oracle-reads-16-seed-42 | 2 | 30 | 0.282 | 0.305 | 0.90x (0.86–0.95) | 0.283 | 0.362 |
| oracle-reads-16-seed-42 | 4 | 30 | 0.282 | 0.307 | 0.91x (0.89–0.96) | 0.285 | 0.419 |
| oracle-reads-64-seed-42 | 1 | 30 | 1.125 | 1.166 | 0.98x (0.96–1.00) | 1.128 | 1.249 |
| oracle-reads-64-seed-42 | 2 | 30 | 1.111 | 1.134 | 0.98x (0.96–1.00) | 1.114 | 1.272 |
| oracle-reads-64-seed-42 | 4 | 30 | 1.118 | 1.139 | 0.98x (0.97–0.98) | 1.122 | 1.359 |
| transfers-16-seed-42 | 1 | 30 | 0.291 | 0.311 | 0.93x (0.90–0.96) | 0.293 | 0.334 |
| transfers-16-seed-42 | 2 | 30 | 0.292 | 0.312 | 0.91x (0.88–0.94) | 0.293 | 0.372 |
| transfers-16-seed-42 | 4 | 30 | 0.290 | 0.320 | 0.87x (0.83–0.92) | 0.292 | 0.449 |
| transfers-64-seed-42 | 1 | 30 | 1.152 | 1.220 | 0.96x (0.93–0.98) | 1.153 | 1.308 |
| transfers-64-seed-42 | 2 | 30 | 1.138 | 1.188 | 0.97x (0.94–0.99) | 1.141 | 1.337 |
| transfers-64-seed-42 | 4 | 30 | 1.129 | 1.206 | 0.94x (0.92–0.96) | 1.133 | 1.499 |

A speedup below 1 means B was slower. The interval bootstraps the median of paired A/B ratios with 2,000 resamples (seed 42); it does not account for host drift or background load.

CSV includes quartile spread, database-call counts, process peak RSS and derived local transaction capacity. Process peak RSS includes fixture loading, initialization and same-variant warmups; it is not isolated execution memory. Physical I/O and cache hit/miss counters are unsupported and remain null. No tail-latency claim is made.

## Counter instrumentation check

Separate counter-enabled and counter-disabled campaigns on the same fixture/configuration. This is a pilot estimate, subject to campaign-order and host noise; it is not a controlled paired overhead result.

| Fixture | B workers | Variant | Median latency change with counters |
| --- | ---: | --- | ---: |
| oracle-reads-64-seed-42 | 4 | A | -2.07% |
| oracle-reads-64-seed-42 | 4 | B | -2.68% |
