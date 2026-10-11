# ReadIndexV1 experimental envelope

The research sidecar is canonical RLP `[1, [[address, storage_key, first_access_index], ...]]`. Address width is exactly 20 bytes. Keys are unsigned 256-bit integers; indices are unsigned 32-bit integers. Entries are strictly ordered by address bytes and numeric key, with no duplicates. Coverage must equal the received BAL's `storage_reads` set exactly. Indices range from 0 through transaction count + 1.

The decoder rejects input above 8 MiB before parsing and above 100,000 entries before allocating another entry. The pinned BAL item/gas bound is checked independently. RLP decoding rejects noncanonical integers and lengths; re-encoding must reproduce the input exactly. These are research limits, not Ethereum consensus changes.

The input envelope binds the complete block hash, standard BAL hash and Keccak-256 of the index RLP. Standard BAL encoding and block consensus fields remain unchanged. B validates the standard payload; C additionally validates this experimental envelope.

The verifier advances only at completed canonical boundaries: pre-execution (0), transactions (1..n), and post-execution (n+1). A newly observed key must declare the current index; a declaration due at a completed boundary must already have been observed. Worker completion order never advances the prefix. Reference traces generate sidecars outside timed validation and are assertion oracles only; they are not supplied to the runtime verifier.

The collector reads transaction-result `EvmState` storage records and independent pre/post-system BAL-builder records. The pinned REVM builder consumes those same records. Tests compare independent reference BALs and actual C validation for successful/reverted reads, no-op writes, out-of-gas before/after access and untouched EIP-2930 hints. No received hint or expected trace is passed to the runtime verifier; no REVM patch is required.

C sets an immutable per-block cancellation token on its first decisive mismatch. Subsequent account prefetch dispatch is skipped, queued storage work checks between operations, and active database operations finish. Existing execution/root abort guards remain in use; active EVM streams are not interrupted. CAttr performs the same verification without cancelling prefetch. FIFO task and pool barriers run before the completion timestamp; tokens/providers reset before the next validation.

The demonstrated scope is ordinary transactions and the listed access boundaries on small synthetic Amsterdam fixtures. It does not prove complete recording semantics for every EVM feature or production consensus safety. First-access indexing is prior art; this artifact evaluates an experimental adaptation and its measured tradeoffs.
