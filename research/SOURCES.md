# Source map for the A/B pilot

Observed 12 October 2026. All Reth source references below are pinned to `1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`.

- [Root manifest](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/Cargo.toml): workspace, Rust 1.95, release profile and dependency versions. The upstream lockfile was preserved; only the runner's dependency entry was added.
- [BAL executor and tests](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal/execute.rs): public `execute_block`, private reference-generation and serial/shadow test helpers. The new runner adapts those patterns without modifying the executor. Helpers return reconstructed BAL; this is not complete consensus validation.
- [Payload validator](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_validator.rs): selects regular/BAL execution, orchestrates prewarming/receipt/state-root work, compares reconstructed BAL commitments and checks final roots. This is the pending L2 integration boundary.
- [Engine arguments](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/node/core/src/args/engine.rs): full A mode disables BAL parallel execution, BAL parallel state root and BAL batch I/O. Those switches do not remove BAL validity rules. The L1 runner does not exercise the full engine configuration.
- [Runtime](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/tasks/src/runtime.rs): explicit BAL streaming thread count and lazy worker pools. The runner initializes its execution pool before timing; prewarm/state-root pools are not used.
- [Workers](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal/worker.rs): workers consume received state hints and return results for canonical commitment. A first-access collector must observe qualifying execution accesses independently of those hints. No collector is implemented by this A/B pilot.
- [Prewarm pool](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal_prewarm_pool.rs): separate warming pool and cleanup path, to map and join in M2/M5.
- [EIP-7928](https://eips.ethereum.org/EIPS/eip-7928): contextual protocol reference. This implementation uses pinned dependencies, not a claim that the web specification remains unchanged.

The original PRD contains the broader related-work sources. Its performance and novelty claims still require verification and experiments. No historical gas-feasibility formula or protocol sidecar was introduced here.
