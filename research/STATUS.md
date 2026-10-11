# Status — 12 October 2026

Fork: https://github.com/sabunjpeg/reth

Branch: `research/bal-first-access`, starting at `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

## Current scope

Implementing the authorized A/B execution pilot. C, cancellation, phantom mutations and full disk-backed payload validation are not implemented. The L1 adapters do not invoke the entire A/B optimization bundle. All outputs and reports label this restriction.

- M0 A/B source/build audit: pinned commit verified, Rust 1.95 verified, baseline BAL tests passed, execution and surrounding validator mapped. Upstream dependency entries are unchanged; only the runner entry was added to Cargo.lock. Source under `crates/` and `testing/` remains unchanged. The full M0 first-access collector feasibility gate remains open for the later C work.
- M1: deterministic transfer and shared-oracle fixtures, real serial/BAL execution, direct equivalence checks, timers/counters, isolated sample processes, bounded child cleanup, JSONL/manifests and analysis implemented. Final paired campaign is the next action after committing the tested source revision.
- M2 onward: pending. L2 needs complete valid payloads and local disk-backed state before validator-performance or physical-I/O claims.

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

## Next action

Commit the tested source, run `research/configs/pilot.json` from that clean revision, run the counter-disabled control, inspect all 720 + 60 records and commit small reference outputs with their producing revision. Then start M2's full-validator/disk integration described in README and CODEX_HANDOFF.
