# Status — 12 October 2026

Fork: https://github.com/sabunjpeg/reth

Branch: `research/bal-first-access`, starting at `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

## Current scope

The original L1 A/B execution pilot is implemented and measured. The requested extension now implements local disk-backed complete-payload A/B, semantic phantom reproduction in B, and experimental first-access/cancellation variants CAttr/C. The L1 and L2 paths are reported separately. This is a restricted development pilot, not production consensus software or a final paper campaign.

- M0: pinned commit/Rust 1.95 verified, baseline BAL tests passed and full validator mapped. Existing upstream dependency entries are unchanged. The L2 extension adds default-off `bal-research` engine hooks; the original L1 producing revision remains unmodified. The collector uses independent actual transaction-result/pre/post access records without a REVM patch, with tests for the declared ordinary-transaction boundary scope.
- M1: deterministic transfer and shared-oracle fixtures, real serial/BAL execution, direct equivalence checks, timers/counters, isolated sample processes, bounded child cleanup, JSONL/manifests and analysis complete. The final campaign contains 720 A/B records (360 pairs); its counter-disabled control contains 60 records (30 pairs). Both completed with zero failures from clean research revision `d19763ef1f5aaac9a6dffce6d3fd684370821420`.
- M2: real local MDBX parent state and complete state/receipt/request/BAL commitments generated. Valid A/B/C payloads are accepted with matching serial-reference outputs. Correctly committed phantom reads pass structural/item-bound checks and stock B rejects their reconstructed BAL mismatch after execution.
- M3/M4: bounded canonical ReadIndexV1 codec and canonical-prefix verifier integrated, including wrong-index, coverage/order/range/size and actual access-boundary tests. False sidecars on valid standard blocks are rejected only by the candidate, while B accepts.
- M5: per-block prefetch cancellation and stock execution/root abort guards used; active reads finish and pending reads skip. Deterministic active/queued/next-block tests and consecutive complete-payload checks pass. Existing stock item/gas guards are retained; a stronger periodic feasibility bound is deferred because its sound system-activity accounting has not been established.
- M6: the 30-pair local disk campaign is ready to run from the frozen clean revision. Reference outputs will identify their producing source and retain failed/timeout records.

## Validation

Initial stock test command, before research integration:

```sh
CARGO_BUILD_JOBS=4 cargo test --locked -p reth-engine-tree --release --lib tree::payload_processor::bal::execute::tests -- --test-threads=1
```

Result: **19 passed, 0 failed**, including serial/BAL shadow comparisons, revert/SSTORE behavior, gas admission and error handling.

Research checks:

```sh
CARGO_BUILD_JOBS=4 cargo test --locked -p bal-research-runner --release
CARGO_BUILD_JOBS=4 cargo build --locked -p bal-research-runner --release
CARGO_BUILD_JOBS=4 cargo clippy --locked -p bal-research-runner --release --all-targets
cargo +1.95 fmt -p bal-research-runner -- --check
python3 -m unittest discover -s research/analysis -p 'test_*.py'
zepter run check
make lint-toml
```

Runner tests: **5 passed**, covering deterministic 16-transaction fixtures under 1/2/4 BAL workers, wrong commitments/reference outputs, actual storage-call instrumentation and a timed-out child followed by another sample. Python analysis tests: **3 passed**, including failed-pair exclusion, missing-pair rejection and output mismatch rejection. Clippy, package formatting, Zepter and TOML formatting pass. Clippy emits an existing upstream dead-code warning at `crates/config/src/config.rs:555`; the runner has no warnings. A strict `-D warnings` attempt failed on that existing warning. Workspace-wide formatting with stable Rust also reports existing upstream formatting differences because the repository's rustfmt settings include nightly-only options. Only the runner was formatted; upstream source was preserved.

Smoke runner and analysis exercised end to end with eight recorded samples and no failures. This pre-freeze development smoke is not paper evidence.

Machine: Apple M4 Pro, 14 cores (10 performance/4 efficiency), 24 GiB RAM, macOS arm64. Rust 1.95.0. Builds used four jobs. Host background load, power/thermal conditions and affinity are uncontrolled. Existing native tools successfully built the selected dependency graph.

## Recorded pilot

The complete reference ledgers, manifests, completion receipts and generated summaries are saved in `results/reference/pilot/` and `results/reference/counterfree/`. Reproduce their analysis with:

```sh
python3 research/analysis/summarize.py research/results/reference/pilot --uninstrumented research/results/reference/counterfree
python3 research/analysis/summarize.py research/results/reference/counterfree
```

A median execution time was 0.282–0.292 ms for the 16-transaction fixtures and 1.111–1.152 ms for 64 transactions. B's paired A/B speedup was 0.87–0.98 across these conditions, so this small L1 pilot did not show a speedup. B also used more CPU time. Signer recovery, per-block setup and in-memory worker database cloning are included; this observation does not establish the effect of disk I/O or the full optimization bundle.

The representative counter-disabled campaign had slightly higher median latency than the counter-enabled campaign (about 2–3%). This uncontrolled cross-campaign result does not resolve instrumentation overhead; it is consistent with host/campaign noise. A controlled instrumentation comparison is needed before making an overhead claim.

The final source matches warmup instrumentation to the measured mode and initializes the fixed chain configuration once before timing. Earlier development campaigns under `results/local/` are ignored and are not reference evidence. The manifests identify the clean producing code revision; the later results commit does not change the runner.

## Next action

Freeze the tested L2 source, run `research/configs/disk-pilot.json`, audit all records and save its raw inputs/results. After this pilot, broaden boundary correctness and held-out workloads and define controlled storage/cache conditions before a final paper campaign.

## L2 extension validation

```sh
CARGO_BUILD_JOBS=4 cargo test --locked -p bal-research-runner --release
CARGO_BUILD_JOBS=4 cargo test --locked -p reth-engine-tree --features bal-research --release --lib tree::payload_processor -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo test --locked -p reth-engine-tree --release --lib tree::payload_processor::bal::execute::tests -- --test-threads=1
python3 -m unittest discover -s research/analysis -p 'test_*.py'
```

Runner: **13 passed**, including C validation for reverted/no-op/OOG accesses, valid standard blocks with false early/late sidecars, stock hash/decode/item-limit guards, canonical-prefix rules, and rejection followed by valid execution. Engine with research hooks: **55 passed**, including the original executor suite, out-of-order/disconnected-worker handling and a deterministic active-read/queued-skip/next-block prefetch test. Feature-disabled executor suite: **19 passed**. Analysis: **7 passed**, including manifest/fixture integrity, complete pair checks, failure accounting and serializable timeout records. Package formatting, dependency/TOML checks and Clippy pass; existing upstream configuration dead-code warning remains documented above. The locked release binary is rebuilt before measurement.

One access test initially used the obsolete 21,000 transaction intrinsic cost. The pinned Amsterdam call actually uses 15,000 gas before bytecode; the test now uses 15,003 and explicitly verifies failure before the storage read. Bad block hashes are checked concurrently with execution, so their control does not assume zero speculative canonical work. These corrected tests preserve the stock semantics rather than changing B.
