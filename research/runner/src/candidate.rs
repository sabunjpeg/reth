//! Actual execution access collection and the optional experimental candidate.

use crate::{
    fixture::{evm_config, Fixture, ORACLE},
    read_index::{Key, PrefixVerifier},
};
use alloy_evm::{
    block::{BlockExecutor, TxResult},
    Evm,
};
use alloy_primitives::{Address, U256};
use eyre::Result;
use reth_engine_tree::tree::{
    instrumented_state::StateProviderStats,
    payload_processor::bal::research::{builder_accesses, transaction_accesses, ResearchObserver},
};
use reth_evm::ConfigureEvm;
use revm::database::State;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Debug)]
pub(crate) struct Observer {
    start: Instant,
    verifier: Mutex<Option<PrefixVerifier>>,
    collect_accesses: bool,
    cancel_on_reject: bool,
    cancel: AtomicBool,
    detection: Mutex<Option<(u64, u32, &'static str)>>,
    completed: AtomicU32,
    phantom_count: usize,
    prefetch_started: AtomicU64,
    prefetch_completed: AtomicU64,
    prefetch_skipped: AtomicU64,
    phantom_started: AtomicU64,
    phantom_completed: AtomicU64,
    phantom_skipped: AtomicU64,
    execution_stats: Mutex<Option<Arc<StateProviderStats>>>,
}

impl Observer {
    pub(crate) const fn new(
        start: Instant,
        verifier: Option<PrefixVerifier>,
        cancel: bool,
        phantoms: usize,
    ) -> Self {
        Self {
            start,
            collect_accesses: verifier.is_some(),
            verifier: Mutex::new(verifier),
            cancel_on_reject: cancel,
            cancel: AtomicBool::new(false),
            detection: Mutex::new(None),
            completed: AtomicU32::new(0),
            phantom_count: phantoms,
            prefetch_started: AtomicU64::new(0),
            prefetch_completed: AtomicU64::new(0),
            prefetch_skipped: AtomicU64::new(0),
            phantom_started: AtomicU64::new(0),
            phantom_completed: AtomicU64::new(0),
            phantom_skipped: AtomicU64::new(0),
            execution_stats: Mutex::new(None),
        }
    }

    fn detect(&self, index: u32, phase: &'static str) {
        self.detection.lock().expect("observer lock").get_or_insert((
            self.start.elapsed().as_nanos() as u64,
            index,
            phase,
        ));
        if self.cancel_on_reject {
            self.cancel.store(true, Ordering::Release);
        }
    }

    pub(crate) fn snapshot(&self) -> Value {
        let detection = *self.detection.lock().expect("observer lock");
        let stats = self.execution_stats.lock().expect("stats lock");
        json!({
            "t_detect_ns": detection.map(|event| event.0),
            "canonical_index_at_detection": detection.map(|event| event.1),
            "rejection_phase": detection.map(|event| event.2),
            "last_verified_canonical_index": self.completed.load(Ordering::Relaxed),
            "prewarm_storage_requests_started": self.prefetch_started.load(Ordering::Relaxed),
            "prewarm_storage_requests_completed": self.prefetch_completed.load(Ordering::Relaxed),
            "prewarm_storage_requests_skipped": self.prefetch_skipped.load(Ordering::Relaxed),
            "phantom_lookups_started": self.phantom_started.load(Ordering::Relaxed),
            "phantom_lookups_completed": self.phantom_completed.load(Ordering::Relaxed),
            "phantom_lookups_skipped": self.phantom_skipped.load(Ordering::Relaxed),
            "prewarm_jobs_skipped": null,
            "logical_reads": stats.as_ref().map(|stats| stats.total_account_fetches() + stats.total_storage_fetches() + stats.total_code_fetches()),
            "logical_reads_scope": "execution_provider_account_storage_code_requests_only; excludes prewarm and trie proof requests",
            "execution_account_reads": stats.as_ref().map(|stats| stats.total_account_fetches()),
            "execution_storage_reads": stats.as_ref().map(|stats| stats.total_storage_fetches()),
            "execution_code_reads": stats.as_ref().map(|stats| stats.total_code_fetches()),
        })
    }
}

impl ResearchObserver for Observer {
    fn collect_accesses(&self) -> bool {
        self.collect_accesses
    }

    fn complete(
        &self,
        index: u32,
        accesses: &[Key],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.collect_accesses
            && let Some(verifier) = &mut *self.verifier.lock().expect("observer lock")
            && let Err(error) = verifier.complete(index, accesses)
        {
            self.detect(index, "first_access_verification");
            return Err(error.into());
        }
        self.completed.store(index, Ordering::Relaxed);
        Ok(())
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn prefetch(&self, address: Address, key: U256, completed: bool, skipped: bool) {
        let phantom =
            address == ORACLE && key > U256::ZERO && key <= U256::from(self.phantom_count);
        let (total, mutated) = if skipped {
            (&self.prefetch_skipped, &self.phantom_skipped)
        } else if completed {
            (&self.prefetch_completed, &self.phantom_completed)
        } else {
            (&self.prefetch_started, &self.phantom_started)
        };
        total.fetch_add(1, Ordering::Relaxed);
        if phantom {
            mutated.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn rejected(&self, phase: &'static str) {
        self.detect(self.completed.load(Ordering::Relaxed), phase);
    }

    fn execution_stats(&self, stats: Arc<StateProviderStats>) {
        *self.execution_stats.lock().expect("stats lock") = Some(stats);
    }
}

/// Producer-only reference collection, using actual transaction results and the upstream builder.
pub(crate) fn reference_trace(fixture: &Fixture) -> Result<BTreeMap<Key, u32>> {
    let config = evm_config();
    let block = fixture.sealed_block();
    let mut state = State::builder()
        .with_database(fixture.db()?)
        .with_bundle_update()
        .with_bal_builder()
        .build();
    let mut executor = config.executor_for_block(&mut state, &block)?;
    executor.apply_pre_execution_changes()?;
    let mut trace = BTreeMap::new();
    for key in builder_accesses(
        executor.evm_mut().db_mut().bal_state.bal_builder.as_ref().expect("builder"),
    ) {
        trace.entry(key).or_insert(0);
    }
    for (index, tx) in fixture.transactions()?.into_iter().enumerate() {
        executor.evm_mut().db_mut().bump_bal_index();
        let result = executor.execute_transaction_without_commit(tx)?;
        for key in transaction_accesses(&result.result().state) {
            trace.entry(key).or_insert(index as u32 + 1);
        }
        let _ = executor.commit_transaction(result);
    }
    executor.evm_mut().db_mut().bump_bal_index();
    executor.apply_post_execution_changes()?;
    for key in builder_accesses(state.bal_state.bal_builder.as_ref().expect("builder")) {
        trace.entry(key).or_insert(fixture.block.body.transactions.len() as u32 + 1);
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixture::Workload, replay};
    use alloy_consensus::{
        proofs::calculate_transaction_root, SignableTransaction, TxEip2930, TxLegacy,
    };
    use alloy_eip7928::{bal::Bal, compute_block_access_list_hash};
    use alloy_eips::eip2930::{AccessList, AccessListItem};
    use alloy_primitives::{keccak256, Bytes, TxKind, B256};
    use reth_ethereum_primitives::Transaction;
    use reth_primitives_traits::crypto::secp256k1::sign_message;

    fn semantic(code: &[u8], gas: u64, access_list: bool) -> Result<Fixture> {
        let mut fixture = Fixture::generate(42, 1, Workload::OracleReads)?;
        fixture.accounts.iter_mut().find(|account| account.address == ORACLE).unwrap().code =
            Bytes::copy_from_slice(code);
        let tx = if access_list {
            Transaction::Eip2930(TxEip2930 {
                chain_id: 1,
                nonce: 0,
                gas_price: 1,
                gas_limit: gas,
                to: TxKind::Call(ORACLE),
                value: U256::ZERO,
                input: Bytes::new(),
                access_list: AccessList(vec![AccessListItem {
                    address: ORACLE,
                    storage_keys: vec![B256::from(U256::from(1).to_be_bytes::<32>())],
                }]),
            })
        } else {
            Transaction::Legacy(TxLegacy {
                chain_id: Some(1),
                nonce: 0,
                gas_price: 1,
                gas_limit: gas,
                to: TxKind::Call(ORACLE),
                value: U256::ZERO,
                input: Bytes::new(),
            })
        };
        let secret = keccak256("bal-research-fixture-v1:42:0");
        let signature = sign_message(secret, tx.signature_hash())?;
        fixture.block.body.transactions = vec![tx.into_signed(signature).into()];
        fixture.block.header.transactions_root =
            calculate_transaction_root(&fixture.block.body.transactions);
        let output = replay::run_serial(&fixture, None)?;
        fixture.bal_rlp = alloy_rlp::encode(Bal::from(output.bal.clone())).into();
        fixture.block.header.block_access_list_hash =
            Some(compute_block_access_list_hash(&output.bal));
        fixture.block.header.gas_used = output.execution.result.gas_used;
        fixture.expected_output_digest = output.digest()?;
        Ok(fixture)
    }

    #[test]
    fn actual_access_semantics_match_independent_bal_builder() -> Result<()> {
        // Successful read, reverted read, no-op SSTORE, OOG before/after SLOAD,
        // and an untouched EIP-2930 hint. Gas prechecks determine qualifying reads.
        for (code, gas, hints, touched) in [
            (crate::fixture::ORACLE_CODE, 100_000, false, true),
            (&[0x60, 0, 0x54, 0x50, 0x60, 0, 0x60, 0, 0xfd][..], 100_000, false, true),
            (&[0x62, 0x01, 0xe2, 0x40, 0x60, 0, 0x55, 0][..], 100_000, false, true),
            // Amsterdam EIP-2780/EIP-8038: this call's intrinsic gas is 12,000 + 3,000.
            // The PUSH consumes the remaining 3; SLOAD then fails before loading the slot.
            (crate::fixture::ORACLE_CODE, 15_003, false, false),
            (&[0x60, 0, 0x54, 0x50, 0x5b, 0x60, 4, 0x56][..], 30_000, false, true),
            (crate::fixture::ORACLE_CODE, 100_000, true, true),
        ] {
            let fixture = semantic(code, gas, hints)?;
            let trace = reference_trace(&fixture)?;
            let bal = alloy_rlp::decode_exact::<Bal>(&fixture.bal_rlp)?;
            let oracle = bal.iter().find(|account| account.address == ORACLE).unwrap();
            let result = replay::run_serial(&fixture, None)?;
            assert_eq!(
                result.execution.result.receipts[0].success,
                !matches!(gas, 15_003 | 30_000) && !code.contains(&0xfd)
            );
            assert_eq!(
                trace.get(&(ORACLE, U256::ZERO)).copied(),
                touched.then_some(1),
                "code={code:?}, gas={gas}, hints={hints}"
            );
            assert_eq!(oracle.storage_reads.contains(&U256::ZERO), touched);
            assert!(!trace.contains_key(&(ORACLE, U256::from(1))));
            assert!(!oracle.storage_reads.contains(&U256::from(1)));
            let index = crate::read_index::ReadIndex::from_trace(&bal, &trace, 1);
            crate::read_index::ReadIndex::decode(&index.encode(), &bal, 1)?;
            assert!(trace.values().any(|index| *index == 0));
            assert!(trace.values().any(|index| *index == 2));
            // Exercise the collector on the actual parallel engine path as well as the
            // independent producer execution, including exceptional receipt outcomes.
            let disk = crate::validator::DiskFixture::generate(&fixture)?;
            let record = crate::validator::Harness::new(
                &disk,
                crate::validator::DiskVariant::C,
                replay::runtime(4)?,
            )?
            .attempt(&disk, crate::validator::DiskVariant::C)?;
            assert_eq!(record["verdict"], "accepted");
            assert_eq!(record["output_digest"], json!(disk.execution.expected_output_digest));
        }
        Ok(())
    }

    #[test]
    fn only_candidate_cancels_and_observer_is_per_block() -> Result<()> {
        for cancel in [false, true] {
            let index = crate::read_index::ReadIndex(vec![crate::read_index::Entry {
                address: ORACLE,
                key: U256::from(1),
                first: 1,
            }]);
            let observer =
                Observer::new(Instant::now(), Some(PrefixVerifier::new(index)), cancel, 1);
            observer.complete(0, &[]).unwrap();
            assert!(observer.complete(1, &[]).is_err());
            assert_eq!(observer.cancelled(), cancel);
            assert_eq!(observer.snapshot()["canonical_index_at_detection"], 1);
            assert!(!Observer::new(Instant::now(), None, true, 0).cancelled());
        }
        Ok(())
    }
}
