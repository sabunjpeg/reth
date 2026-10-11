# Continuation

Repository: `sabunjpeg/reth`, branch `research/bal-first-access`. Read `AGENTS.md`, `research/PRD.md`, `research/README.md`, `research/STATUS.md` and `research/SOURCES.md` first.

The current artifact is the local L1 A/B execution pilot. Run the documented locked release build, correctness tests and smoke configuration before extending it. Treat the committed fixtures and reference records as revision-specific evidence. Do not substitute L1 timing for full-validator latency, logical lookups for disk I/O, or execution success for valid-payload acceptance.

Next: implement M2's disk-backed full-payload path with complete headers/commitments, stock guards, equivalent restored parent state, matched thread/cache policies, acceptance-cache resets and measured cleanup. Preserve the unmodified pinned baseline and prove valid L2 acceptance before introducing correctly committed phantom reads. C remains outside the present implementation.

Cloud can build and run the same small correctness/smoke commands from this committed branch. Prepare Rust 1.95, clang/libclang, pkg-config and Python, and test the environment rather than relying on cached dependencies. Cloud resource limits and uncontrolled performance must be recorded separately from Mac/VPS measurements.
