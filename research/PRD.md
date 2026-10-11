# BAL First Access Research PRD

Version 1.0 • 11 October 2026 • Implementation brief for Codex Desktop and Codex Cloud

Build a reproducible research artifact in a Reth fork to evaluate whether first-access attribution for BAL storage reads reduces invalid-block detection time and wasted validator work. Use ordinary and trading-related EVM workloads. The deliverable is a tested mechanism, a benchmark environment, and evidence for an 8–9 page design science research paper. A speedup is a hypothesis, not an acceptance requirement.

## 1 Research purpose

The primary research question is: **Under which workload, storage and scheduling conditions does first-access attribution reduce rejection cost enough to justify its metadata and valid-block overhead?**

Evaluate three distinct propositions: attribution detects some invalid declarations earlier; earlier detection stops enough work to matter; the benefit survives comparison with existing safeguards and pays for the extension. Negative findings are useful when the experiment is sound.

First-access indexing already appears in the Ethereum Research discussion. Do not claim that the index itself is novel. The potential DSR contribution is an implemented adaptation, a measured tradeoff, and design knowledge about execution order, access semantics and cancellation. Check related work again before framing the final contribution. [S2]

## 2 Problem and evidence

EIP-7928 declares read-only storage keys without transaction attribution. Its security section still describes spurious declarations that can cause unnecessary prefetching before rejection. It bounds BAL items and recommends sound early detection, explicitly accounting for system activity that does not consume block gas. This establishes a research problem; it does not establish the size of the cost in the selected client. [S1]

PR 11223 merged a gas-budget mitigation discussion on 17 February 2026. Later specification changes superseded that particular recipe. Treat the current specification and pinned implementation as the baseline; do not copy the old remaining-gas formula or periodic interval without a soundness argument. [S3, S4]

First reproduce the behavior in stock Reth. A mitigation may already reject a particular fixture cheaply, or prefetch may finish before detection. Record those outcomes. Do not weaken the baseline to manufacture an improvement.

Trading workloads motivate shared oracle reads, shared pool state and routing dependencies. These are execution workloads, not measurements of trader profit, spreads, network throughput or finality. The workload paper is literature to inspect for relevant workload characteristics. [S10]

## 3 Scope and deliverables

Implement one Rust research project inside one Reth fork. Deliver deterministic fixtures, the candidate verifier, measurements, cancellation tests, raw results, analysis scripts and reproduction instructions. Reuse Reth and REVM execution rather than writing a toy EVM or a new blockchain.

The initial scope excludes mainnet synchronization, a live adversarial network, production consensus changes, a DLOB application, multiple client implementations and guided prefetch scheduling. BALrog may become a later input mutator; it is not the execution client or an MVP dependency. [S11]

Separate three evaluation levels:

| Level | Environment | Permitted claim |
| --- | --- | --- |
| L0 | Pure index and encoding tests | Metadata rules and verifier behavior |
| L1 | In-memory Reth executor fixtures | Execution equivalence and component cost |
| L2 | Complete payload validation with local disk state | Local validation, rejection and resource cost |

Only L2 results support the paper's primary validator-performance claims. Neither a mock database nor an executor helper alone establishes a disk-I/O or complete-block validation result.

## 4 Repository and version policy

Use upstream `paradigmxyz/reth` with starting commit:

`1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607`

The root manifest at this revision declares Rust 1.95 as its minimum version. Verify the commit, dependency lockfile, native build requirements and relevant APIs in M0. Do not assume a toolchain file exists or silently switch to latest. If the revision cannot build in the target environment, record the exact blocker and justify any replacement revision before collecting comparable data. [S5]

Use branch `research/bal-first-access`. Retain upstream licensing. Follow existing `AGENTS.md` instructions and repository conventions; do not replace root instructions with this brief. Add the runner to the Cargo workspace only after identifying the required internal APIs.

| Path relative to the Reth repository | Purpose |
| --- | --- |
| `research/PRD.md` | Research requirements and acceptance gates |
| `research/CODEX_HANDOFF.md` | Desktop and Cloud continuation prompts |
| `research/STATUS.md` | Completed work, blockers and next action |
| `research/SOURCES.md` | Source URLs and observation dates |
| `research/runner/` | Proposed Rust fixture and benchmark runner |
| `research/fixtures/` | Small committed fixtures and generators |
| `research/configs/` | Versioned experiment configurations |
| `research/analysis/` | Summary and figure generation |
| `research/results/` | Small reference outputs and run manifests |

Large databases and build artifacts stay outside Git. Include snapshot generation instructions and checksums. Record upstream SHA, research SHA and any dirty diff for every run; paper runs require a clean recorded revision.

## 5 Comparison configurations

All configurations use the same client revision, fork rules, parent state and blocks. Labels here replace the older plan's B0, B1 and D1 labels.

| ID | Configuration | Comparison purpose |
| --- | --- | --- |
| A | BAL-enabled fork with BAL parallel execution, BAL parallel state root and BAL batch I/O disabled | Conventional execution context on the same fork |
| B | Stock BAL path, retaining all guards | Primary baseline |
| B2 | B plus a sound gas-feasibility mitigation, if absent and feasible | Optional stronger control |
| C_attr | Selected B or B2 baseline plus first-access verification; ordinary rejection cleanup retained | Isolate attribution and detection |
| C | C_attr plus cancellation of pending block work | Measure the complete candidate and released resources |

The pinned Reth source exposes these A switches: [S6]

```text
--engine.disable-bal-parallel-execution
--engine.disable-bal-parallel-state-root
--engine.disable-bal-batch-io
```

A still validates BALs. A versus B therefore measures the implementation's BAL optimization bundle, not the complete effect of introducing EIP-7928 to a pre-BAL network. Record automatic changes in warming behavior and other effective defaults.

If B already implements a sound feasibility guard, retain it and merge B2 into B. Otherwise implement B2 only after deriving a safe bound from the pinned fork, including relevant system activity. If a sound bound cannot be established within scope, defer B2 explicitly and limit comparison claims. C must inherit the selected baseline's safeguards.

Match B and C on execution workers, prewarm workers, cache sizes, database, state-root policy and dispatch order. Initial prefetch admission and ordering remain unchanged. Cancellation is a separately identified intervention. Include C_attr for representative cases to distinguish detection from cleanup savings. Feature ablations beyond this are optional.

Gate the candidate behind explicit configuration. In the research build, B must preserve stock execution and cleanup behavior apart from documented measurement instrumentation. Check its outputs against the unmodified pinned revision and measure representative instrumentation overhead. Record any dependency patch that affects the baseline; do not describe an altered fallback implementation as stock Reth.

## 6 Candidate metadata

Define a versioned experimental sidecar, **ReadIndexV1**, containing exactly one entry for each key in the supplied BAL's `storage_reads` collections. Each entry contains:

| Field | Representation |
| --- | --- |
| `address` | 20-byte address |
| `storage_key` | Unsigned 256-bit key |
| `first_access_index` | Unsigned 32-bit canonical block access index |

Use index 0 for completed pre-execution activity, 1 through n for transactions in block order, and n+1 for post-execution activity. Worker completion order never determines the index. [S1]

Proposed canonical encoding is RLP `[1, entries]`, with each entry encoded as `[address, storage_key, first_access_index]`. Sort by address bytes and numeric storage key. Enforce canonical integers, exact address width, unique entries, exact coverage and index range. Enforce byte and entry limits before allocation. Record the final encoding and limits in `research/spec.md` before integration.

The research input envelope binds the standard block hash, BAL hash and `keccak256(ReadIndexV1 RLP)` to their bytes. This is an experimental envelope; its sidecar commitment is not an Ethereum consensus commitment. An eventual deployable extension would need a separately reviewed protocol change. Do not silently modify EIP-7928's standard BAL encoding or claim current nodes understand the sidecar.

Producer/reference execution generates truthful indices from observed qualifying accesses. Time index generation and encoding separately from validator processing. The fixture's expected trace is an assertion oracle only; do not pass it to the runtime verifier.

Example: transaction 1 touches oracle key X, transaction 2 touches X again, and transaction 3 touches configuration key Y. Declare X at 1 and Y at 3. If Y is never accessed but claims index 2, the extended format becomes provably invalid after canonical transaction 2 completes. A declaration naming the final index may provide little early benefit.

## 7 Access collection and ordered verification

The verifier needs independent records of actual qualifying EVM storage accesses. Prefetch requests, database cache fills and listed-but-untouched transaction access-list keys are not proof of execution access. Include qualifying accesses whose state changes later revert. Reuse the pinned EIP's recording semantics and existing upstream tests rather than inferring an access set from final state writes.

At the selected revision, BAL worker results do not expose an explicit transaction access trace. Resolve this integration in M0: identify a safe REVM hook or existing recording mechanism; test its semantics; establish its overhead. A local pinned REVM patch is acceptable if necessary and documented. Do not invent unavailable methods or derive observations from the received hints. [S7]

Aggregate observations at canonical completion boundaries:

1. Complete and validate pre-execution activity; resolve index 0.
2. Workers execute transactions against the baseline's state versions and return local qualifying access observations.
3. The canonical handler consumes results in transaction order and performs ordinary admission and execution checks.
4. For every observed indexed key at canonical index i, reject if it declares a later first index than i.
5. Once index i has completed, reject any key declaring i that has not been observed by that point.
6. Complete post-execution activity and resolve index n+1.
7. Retain complete BAL reconstruction and all normal final validation.

A transaction that finishes early in wall-clock time cannot advance the canonical prefix. Missing results cannot justify rejection. Duplicate touches retain their earliest canonical index.

The soundness obligation is conditional on truthful recording: a valid sidecar's declared first index equals its first qualifying access, so neither a completed missing claim nor an earlier actual access can occur. Encoding tests and semantic boundary tests must establish the assumptions. This is not a formal proof of the whole client.

A valid standard BAL with a false sidecar is invalid under C's experimental format only. Report that category separately from a phantom BAL read that B and C both reject. Do not describe B's lack of sidecar validation as a bug.

## 8 Cancellation and complete validation

Reth's executor helper returns a reconstructed BAL; complete rejection also depends on the surrounding validator. Map the full path before choosing timers. Preserve final BAL, gas, receipts, requests and state-root checks. [S8, S9]

The pinned prewarm pool uses a separate worker pool with a default of 128 threads. Its end-of-block signaling waits behind queued warming work. An early logical verdict therefore does not automatically release resources. Existing execution abort checks also do not imply immediate interruption of an active EVM or database operation. [S7, S12]

For C, use a per-block cancellation token or equivalent generation identifier. Stop new dispatch, skip pending jobs, check cancellation between database operations and acknowledge all required cleanup. Permit active noninterruptible operations to finish. Attribute work to its block so tasks cannot leak into the next measurement or cancel another block.

Cancellation must be safe during normal completion, failure, worker disconnect and runner shutdown. Add deterministic tests for queued jobs, active jobs, out-of-order results and consecutive blocks. Preserve the original error as the primary verdict when cleanup also fails. Avoid deadlocks and unbounded queues introduced by instrumentation.

## 9 Fixtures and correctness gates

Generate tiny deterministic initial states using local test keys and fixed seeds. Fixtures contain parent/fork context, full transaction bodies, required system contracts and complete expected outputs. L2 fixtures need valid headers and commitments; upstream executor tests with header-only containers are not automatically valid payloads.

Create a truthful reference block, then derive invalid counterparts. Add phantom keys that reference execution never touches, retain every genuine entry, remain within the pinned item limit, sort canonically, and recompute the BAL commitment and dependent block hash. Generate the corresponding experimental envelope. Preserve non-BAL execution outputs when the mutation changes declarations alone.

| Case | Required outcome |
| --- | --- |
| Valid ordinary and trading fixtures | A, B and C agree on outputs; C accepts truthful indices |
| Phantom keys with early, middle and late claims | B and C reject semantically; record phase and cost |
| True key with earlier or later false first index | C rejects extended metadata; B validates the standard block |
| Shared key across transactions | Canonical earliest index remains stable under reordered completions |
| Reverts, no-op writes and reads with later changes | Correct recording and final BAL equivalence |
| Out-of-gas before and after qualifying access | Match pinned upstream access semantics |
| Pre/post system activity and withdrawals | Complete-fork correctness before broad claims |
| Duplicate, missing, unordered or oversized index entries | Reject at structural validation |
| Bad hashes, malformed BAL or oversized BAL | Separate cheap-rejection controls |
| Cancellation followed by another valid block | No stale jobs, cross-block cancellation or output contamination |

For valid inputs compare state roots, receipts roots and receipts, gas, logs, execution requests, reconstructed BAL and execution outcomes. Any unexplained difference blocks performance interpretation. A restricted ordinary-transaction pilot is allowed; label its scope until the full boundary suite passes.

## 10 Measurements

Use a monotonic clock and timestamps in integer nanoseconds internally. The full-validation timer starts before metadata decode, commitments, prechecks or prefetch scheduling. Capture detection when a decisive error is established. Capture completion only after the verdict and all block-attributed work required for isolation have settled.

| Metric | Definition | Comparison |
| --- | --- | --- |
| Valid validation latency | t_stop minus t_start for accepted L2 blocks | A/B and B/C |
| Detection latency | t_detect minus t_start for invalid blocks | B/C_attr/C |
| Resource release latency | t_stop minus t_start | B/C |
| Cleanup duration | t_stop minus t_detect | B/C_attr/C |
| Process CPU work | User plus system CPU through t_stop, all threads | A/B and B/C |
| Phantom state work | Started, completed and skipped lookups for mutated keys | B/C |
| Storage work | Logical lookups, cache hits/misses and physical read bytes where supported | Disk experiments |
| Peak memory | Per-process peak RSS with collection method recorded | Secondary overhead |
| Metadata overhead | Encoded sidecar bytes, compressed bytes with fixed codec, decode/verify time | C cost |
| Producer overhead | Reference/index collection, generation and encoding time | Separate cost |

Compute paired A/B speedup as `T_A / T_B` and valid C overhead as `100 * (T_C / T_B - 1)`. Define local processing throughput as accepted transactions divided by validation seconds; label it local processing capacity. CPU core-ms equals CPU seconds times 1000. Do not substitute elapsed time for CPU work.

Instrument shared work counters consistently. Keep expensive tracing out of timed runs; use it in correctness runs. Check instrumentation overhead with representative stock runs and record it. Report C's essential collection cost even if an optional diagnostic collector is disabled in B.

Database lookups and returned value bytes do not equal physical disk reads. Unsupported physical-I/O counters are null with a reason, not zero. Disk state alone does not establish a cold OS page cache. Measure completed work through cleanup, including work after detection.

## 11 Run records and reproducibility

Export one JSONL record per run, plus a run manifest. At minimum record:

```text
schema_version, run_id, host_id, measurement_level, fixture_id, fixture_hash,
seed, repetition, comparison_pair, execution_order, variant, verdict,
rejection_reason, rejection_phase, canonical_index_at_detection,
t_start_ns, t_detect_ns|null, t_stop_ns,
wall_ns, cpu_user_ns|null, cpu_system_ns|null, peak_rss_bytes|null,
logical_reads, cache_hits|null, cache_misses|null, physical_read_bytes|null,
phantom_lookups_started, phantom_lookups_completed, prewarm_jobs_skipped,
tx_count, gas_used, phantom_count, bal_bytes, index_bytes,
execution_workers, prewarm_workers, state_root_workers,
cache_condition, database_backend, output_digest, manifest_hash
```

Keep platform support and counter definitions in the manifest. Use delta counters scoped to the run rather than process lifetime totals. Timeouts, crashes and failed cleanup produce explicit failed records; never silently drop them or report a timeout as a successful rejection.

The manifest records client and research revisions, source snapshots, compiler and build flags, lockfile hash, workload parameters, hardware, OS, RAM, storage, thread pools, CPU affinity or override policy, cache preparation, compression version, sample counts and run order. Seeds and fixture files must reconstruct the same workload independently of the verifier.

## 12 Workloads and experiment protocol

Begin with transfers as a control and a tiny EVM contract that reads shared oracle-like storage. Add deterministic swap-like pool updates and state-dependent routing after the basic mechanism works. Commit bytecode and its source/build provenance. Use actual EVM execution, not sleeps or modeled transaction durations.

Start with 16 and 64 transactions; extend to 256 if useful. Use 1, 2 and 4 execution workers where available. Distinguish execution workers from prewarm and state-root pools. For invalid fixtures use zero, 10% and 50% of the remaining permitted BAL item budget for phantom declarations, with exact counts recorded. Sweep early, middle and late claimed indices and low/high state sharing.

Avoid a full factorial campaign initially. Tune using pilot fixtures, then freeze mechanisms and configurations before evaluating held-out seeds and workload combinations. Include computation-heavy and mostly independent accesses if needed to expose overhead or limited opportunity for benefit.

Use five warmups and 30 measured repetitions per pilot cell. Final cells use at least 100 paired repetitions, or a documented sample-size plan frozen before final runs. Report median, spread, paired confidence intervals and only supported tails; do not infer a reliable p99 from a small sample.

Randomize paired variant order. Restore equivalent parent state and reset validator acceptance caches so repeated blocks cannot use a known-block fast path. Record snapshot restore and cache preparation. Define application-cache-fresh and warm procedures separately; call a condition OS-cold only if actually controlled and documented.

Record thermal/power behavior and background load where practical. Report Mac and VPS independently. Shared VPS noise and uncontrolled page caches are limitations. Codex Cloud runs are development and correctness evidence by default; do not pool them with machine benchmarks or present them as controlled validator performance.

## 13 Milestones and acceptance criteria

| Milestone | Work | Acceptance gate |
| --- | --- | --- |
| M0 | Verify pin, build stock tests, map full validation and access hooks | Recorded build/test evidence, source map and feasible collector plan |
| M1 | In-memory A/B runner, deterministic fixture and run export | One truthful fixture produces matching outputs and labeled L1 records |
| M2 | Full payload fixture and semantic phantom reproduction in stock B | Valid L2 acceptance, committed phantom rejection, measured phases and disk-state path |
| M3 | ReadIndexV1 codec and pure verifier | Canonical encoding, bounds, wrong-index and ordering tests pass |
| M4 | Integrate actual access collection and C_attr | Correctness matrix passes within declared scope; final checks retained |
| M5 | Per-block cancellation, C and strong-control decision | Cleanup isolation tests pass; B/B2 decision justified; paired disk pilot succeeds |
| M6 | Freeze and execute campaign, analyze outputs | Raw records, manifests, plots, limitations and reproduction guide are complete |

Complete the gates in dependency order and keep changes reviewable. Continue through routine reversible work without requesting approval at every milestone. Stop dependent work only for an unresolved correctness issue, unavailable capability or a material scope decision; document the blocker and continue independent work where useful.

An M0 access-hook feasibility spike is allowed before the runner. A mock verifier may support L0 tests but cannot satisfy M4. If stock B has no reproducible costly case after representative tests, report that finding and reassess the question rather than proceeding with a contrived weak baseline.

## 14 Desktop and Cloud requirements

Use the same Git repository as the durable handoff. Keep PRD, status, source pins, setup commands and tests in committed files. An initial local checkout is sufficient for Desktop; Cloud needs an accessible repository and a prepared environment. See `CODEX_HANDOFF.md` for launch prompts and the official setup reference. [S13]

Prepare Cloud with the pinned Rust toolchain, upstream native dependencies and Python for analysis. Derive native package requirements from the pinned build documentation and successful compilation. Configure dependency network access as required; do not rely on undocumented cache state. No chain RPC key, funded account or mainnet state is needed for the synthetic pilot.

Cloud setup must test the selected build and stock tests before publishing the environment. Start with conservative build parallelism on memory-limited hosts. Use small committed fixtures for smoke tests; never synchronize a full chain as an automatic setup step. Record resource limits and exact failures when the large Reth workspace cannot fit.

Commit important continuation state. New cloud tasks use separate workspaces, so uncommitted changes in another task are not the handoff contract. Update `STATUS.md` after each substantial milestone with changes, exact test commands/results, remaining uncertainty and next action. [S13]

## 15 Definition of done

The artifact is complete when another person can check out the recorded revision, build it using documented setup, regenerate fixtures, run correctness tests and the small A/B/C pilot, and regenerate summary outputs from raw data. The final campaign can require additional machine time, but its procedure and inputs must be reproducible.

Paper-ready evidence includes: a clearly bounded problem; an artifact specification and rationale; a fair baseline; correctness evidence; measured detection and cleanup costs; valid-block and metadata overhead; negative or late-index cases; source/version provenance; and limits on generalization.

The paper should concentrate on one mechanism. Allocate roughly one page to the problem and DSR question, one to related work, two to design, one to methodology, two to results and one to discussion/conclusion, adapting to the required venue template. The PRD is an engineering brief, not the paper manuscript.

## 16 Risks and decisions

| Risk | Required response |
| --- | --- |
| Access collection unavailable or semantically incomplete | Resolve in M0; document a pinned dependency patch or restrict claims |
| Current guards already mitigate tested fixtures | Preserve them; test other justified cases or report limited benefit |
| Phantom prefetch finishes before detection | Report no saved I/O; do not equate detection speed with protection |
| Late claims defer detection | Include as an explicit adverse case |
| System accesses invalidate a simple gas bound | Require sound accounting; defer unsupported B2 |
| Index metadata changes the protocol contract | Describe experimental envelope and compatibility cost |
| Cancellation races or stale work contaminate measurements | Block campaign until deterministic isolation tests pass |
| Cloud build or shared-host noise limits reproducibility | Record limits; use Cloud for functional work and separate host results |

References and dated source observations are in `SOURCES.md`. The primary links are included below so this PRD can also be read on its own.

## 17 References

| ID | Source |
| --- | --- |
| S1 | [EIP-7928](https://eips.ethereum.org/EIPS/eip-7928) |
| S2 | [Early Rejection of Adversarial BALs](https://ethresear.ch/t/early-rejection-of-adversarial-bals/23995) |
| S3 | [EIPs PR 11223](https://github.com/ethereum/EIPs/pull/11223) |
| S4 | [Storage-read gas-feasibility amendment](https://github.com/ethereum/EIPs/commit/d2a64c2d4cc44f2f507577d0ebfb110dcc21d358) |
| S5 | [Pinned Reth manifest](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/Cargo.toml) |
| S6 | [Pinned engine arguments](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/node/core/src/args/engine.rs) |
| S7 | [Pinned BAL workers](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal/worker.rs) |
| S8 | [Pinned BAL executor](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal/execute.rs) |
| S9 | [Pinned payload validator](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_validator.rs) |
| S10 | [arXiv 2606.19869](https://arxiv.org/abs/2606.19869) |
| S11 | [BALrog](https://github.com/nerolation/balrog) |
| S12 | [Pinned prewarm pool](https://github.com/paradigmxyz/reth/blob/1a4dbbe11264fd52e6e9fbe768aa83f9a6b73607/crates/engine/tree/src/tree/payload_processor/bal_prewarm_pool.rs) |
| S13 | [Official Codex Cloud environments guide](https://learn.chatgpt.com/docs/environments/cloud-environments) |
