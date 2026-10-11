# Continuation

Repository: `sabunjpeg/reth`, branch `research/bal-first-access`. Read `AGENTS.md`, `research/PRD.md`, `research/README.md`, `research/STATUS.md` and `research/SOURCES.md` first.

The artifact includes the original L1 execution pilot and the L2 local full-validator A/B/B-C pilot. Run the documented locked release build, runner/engine correctness tests and a small reproduction before extending it. Treat fixtures and reference records as revision-specific evidence. Do not substitute L1 timing for full-validator latency, logical lookups for disk I/O, or a tiny page-cached synthetic parent for cold production state.

The default-off `bal-research` feature exposes canonical execution-access observations, a per-block prefetch cancellation hook and cache-reset helpers. C validates the experimental `ReadIndexV1` envelope independently of received hints; CAttr keeps stock prefetch cleanup. B retains pinned guards and scheduling. BStock is observer-disabled in the same research build, not an unmodified binary. Do not weaken the stock item bound or invent a historical gas-feasibility formula.

The requested local rerun is complete: 720 L1 records and 720 L2 records, zero failures. L2 producing source is `bd77b1d2f9709f55c3d488de61987167ddd230c0`; retained inputs/records and their interpretation are under `results/reference/disk-pilot/`. Valid A/B and B/C show no speedup. With 7,460 early-claim phantom reads C releases resources sooner (1.21x paired speedup), despite a later elapsed-time detection; CAttr is slower than B. Preserve this distinction between canonical detection and cancellation savings. Cancellation counts vary with scheduling, so use ranges as well as medians.

Next: broaden boundary correctness (nested calls, creation/selfdestruct, withdrawals), add held-out workloads/seeds and define controlled storage/cache conditions. Freeze a final sample-size plan before a paper campaign. Keep valid overhead separate from invalid detection, cleanup and skipped work, including late-index negative cases. Full batch-job skips and physical I/O are unsupported; keep their metrics null.

Cloud can build and run the same small correctness/smoke commands from this committed branch. Prepare Rust 1.95, clang/libclang, pkg-config and Python, and test the environment rather than relying on cached dependencies. Cloud resource limits and uncontrolled performance must be recorded separately from Mac/VPS measurements.
