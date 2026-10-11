# Local L1 A/B execution pilot

Research revision: `18bc3d4605c58c0b243a0d60b60fab163f54d0b5`. Upstream: `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

These are execution-component measurements with in-memory state. They exclude storage prewarming, state-root tasks and complete payload validation. They do not establish full-validator or physical disk-I/O performance.

Failed records retained: **0**. Only complete successful pairs enter the statistics.

| Fixture | B workers | Pairs | A median ms | B median ms | Paired speedup (95% CI) | A CPU ms | B CPU ms |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| oracle-reads-16-seed-42 | 1 | 30 | 0.273 | 0.296 | 0.94x (0.92–0.97) | 0.277 | 0.319 |
| oracle-reads-16-seed-42 | 2 | 30 | 0.280 | 0.296 | 0.93x (0.90–0.97) | 0.283 | 0.356 |
| oracle-reads-16-seed-42 | 4 | 30 | 0.283 | 0.301 | 0.92x (0.88–0.94) | 0.287 | 0.417 |
| oracle-reads-64-seed-42 | 1 | 30 | 1.089 | 1.127 | 0.97x (0.95–0.98) | 1.091 | 1.211 |
| oracle-reads-64-seed-42 | 2 | 30 | 1.076 | 1.094 | 0.98x (0.97–1.00) | 1.080 | 1.228 |
| oracle-reads-64-seed-42 | 4 | 30 | 1.163 | 1.244 | 0.95x (0.92–0.98) | 1.164 | 1.520 |
| transfers-16-seed-42 | 1 | 30 | 0.285 | 0.320 | 0.91x (0.88–0.92) | 0.288 | 0.350 |
| transfers-16-seed-42 | 2 | 30 | 0.273 | 0.317 | 0.86x (0.85–0.91) | 0.275 | 0.377 |
| transfers-16-seed-42 | 4 | 30 | 0.285 | 0.311 | 0.91x (0.89–0.93) | 0.288 | 0.433 |
| transfers-64-seed-42 | 1 | 30 | 1.086 | 1.148 | 0.95x (0.93–0.96) | 1.089 | 1.236 |
| transfers-64-seed-42 | 2 | 30 | 1.085 | 1.145 | 0.95x (0.94–0.96) | 1.088 | 1.292 |
| transfers-64-seed-42 | 4 | 30 | 1.091 | 1.152 | 0.95x (0.93–0.96) | 1.097 | 1.429 |

A speedup below 1 means B was slower. The interval bootstraps the median of paired A/B ratios with 2,000 resamples (seed 42); it does not account for host drift or background load.

CSV includes quartile spread, database-call counts, process peak RSS and derived local transaction capacity. Process peak RSS includes fixture loading, initialization and same-variant warmups; it is not isolated execution memory. Physical I/O and cache hit/miss counters are unsupported and remain null. No tail-latency claim is made.
