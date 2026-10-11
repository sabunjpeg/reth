//! Feature-gated research observation, independent of received BAL hints.

use alloy_primitives::{Address, U256};
use revm::state::{bal::Bal, EvmState};

/// An actual qualifying execution storage access.
pub type AccessKey = (Address, U256);

/// Optional per-block research instrumentation and first-access verification.
pub trait ResearchObserver: std::fmt::Debug + Send + Sync {
    /// Whether actual access collection is required by the candidate.
    fn collect_accesses(&self) -> bool;

    /// Completes one canonical boundary, never a worker completion boundary.
    fn complete(
        &self,
        index: u32,
        accesses: &[AccessKey],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Whether pending block prefetch should be cancelled.
    fn cancelled(&self) -> bool;

    /// Records a dispatched prefetch operation, or one skipped after cancellation.
    fn prefetch(&self, address: Address, key: U256, completed: bool, skipped: bool);

    /// Records a decisive stock rejection without changing the error.
    fn rejected(&self, phase: &'static str);

    /// Shares the pinned execution-provider counters; prefetch requests are counted separately.
    fn execution_stats(
        &self,
        _stats: std::sync::Arc<crate::tree::instrumented_state::StateProviderStats>,
    ) {
    }
}

/// Extracts the same loaded storage records consumed by the pinned REVM BAL builder.
///
/// The transaction result includes qualifying reads retained after reverts and no-op writes.
/// It does not include untouched access-list warming entries. These are access records, not a
/// set inferred from changed final storage values.
pub fn transaction_accesses(state: &EvmState) -> Vec<AccessKey> {
    state
        .iter()
        .flat_map(|(address, account)| account.storage.keys().map(move |key| (*address, *key)))
        .collect()
}

/// Extracts accumulated pre/post-system access records from the independently rebuilt BAL.
pub fn builder_accesses(bal: &Bal) -> Vec<AccessKey> {
    bal.accounts
        .iter()
        .flat_map(|(address, account)| {
            account.storage.storage.keys().map(move |key| (*address, *key))
        })
        .collect()
}
