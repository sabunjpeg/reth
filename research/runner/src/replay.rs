//! Adapters around the pinned serial executor and unmodified BAL execution helper.

use crate::{
    fixture::{evm_config, Fixture},
    measurement::{CountedDb, Counters},
};
use alloy_eip7928::BlockAccessList;
use alloy_evm::{block::BlockExecutor, Evm};
use alloy_primitives::{keccak256, B256};
use eyre::{ensure, Result};
use reth_engine_tree::tree::payload_processor::bal::execute_block;
use reth_ethereum_primitives::Receipt;
use reth_evm::ConfigureEvm;
use reth_provider::BlockExecutionOutput;
use reth_revm::db::InMemoryDB;
use reth_tasks::{RayonConfig, Runtime, RuntimeBuilder, RuntimeConfig, TokioConfig};
use revm::database::{states::bundle_state::BundleRetention, State};
use revm::Database;
use serde_json::{json, Value};
use std::{convert::Infallible, fmt::Debug, sync::Arc};

#[derive(Debug)]
pub(crate) struct Output {
    pub execution: BlockExecutionOutput<Receipt>,
    pub bal: BlockAccessList,
}

impl Output {
    pub(crate) fn digest(&self) -> Result<B256> {
        // Sort JSON object keys recursively. Hash-map iteration cannot affect the digest.
        // Revert bookkeeping is compared directly in check(), not used as a final-state digest.
        let value = json!({
            "state": self.execution.state.state,
            "contracts": self.execution.state.contracts,
            "receipts": self.execution.result.receipts,
            "gas_used": self.execution.result.gas_used,
            "requests": self.execution.result.requests,
            "bal_rlp": alloy_rlp::encode(alloy_eip7928::bal::Bal::from(self.bal.clone())),
        });
        let mut bytes = Vec::new();
        canonical_json(&value, &mut bytes)?;
        Ok(keccak256(bytes))
    }
}

pub(crate) fn run_serial(fixture: &Fixture, counters: Option<Arc<Counters>>) -> Result<Output> {
    run_serial_with_db(fixture, fixture.db()?, counters)
}

pub(crate) fn run_serial_with_db(
    fixture: &Fixture,
    parent: InMemoryDB,
    counters: Option<Arc<Counters>>,
) -> Result<Output> {
    match counters {
        Some(counters) => execute_serial(fixture, CountedDb::new(parent, Some(counters))),
        None => execute_serial(fixture, parent),
    }
}

fn execute_serial<DB>(fixture: &Fixture, parent: DB) -> Result<Output>
where
    DB: Database<Error = <InMemoryDB as Database>::Error> + Debug,
{
    let config = evm_config();
    let block = fixture.sealed_block();
    let mut state =
        State::builder().with_database(parent).with_bundle_update().with_bal_builder().build();
    let result = {
        let mut executor = config.executor_for_block(&mut state, &block)?;
        executor.apply_pre_execution_changes()?;
        for tx in fixture.transactions()? {
            executor.evm_mut().db_mut().bump_bal_index();
            executor.execute_transaction(tx)?;
        }
        executor.evm_mut().db_mut().bump_bal_index();
        executor.apply_post_execution_changes()?
    };
    let bal = state.take_built_alloy_bal().expect("BAL builder enabled");
    state.merge_transitions(BundleRetention::Reverts);
    Ok(Output { execution: BlockExecutionOutput { state: state.take_bundle(), result }, bal })
}

pub(crate) fn run_bal(
    fixture: &Fixture,
    runtime: &Runtime,
    counters: Option<Arc<Counters>>,
) -> Result<Output> {
    run_bal_with_db(fixture, fixture.db()?, runtime, counters)
}

pub(crate) fn run_bal_with_db(
    fixture: &Fixture,
    parent: InMemoryDB,
    runtime: &Runtime,
    counters: Option<Arc<Counters>>,
) -> Result<Output> {
    match counters {
        Some(counters) => execute_bal(fixture, CountedDb::new(parent, Some(counters)), runtime),
        None => execute_bal(fixture, parent, runtime),
    }
}

fn execute_bal<DB>(fixture: &Fixture, parent: DB, runtime: &Runtime) -> Result<Output>
where
    DB: Database<Error = <InMemoryDB as Database>::Error> + Clone + Send + Sync + Debug,
{
    let config = evm_config();
    let block = fixture.sealed_block();
    let make_db = |_: bool| Ok(parent.clone());
    let (tx_sender, tx_receiver) = crossbeam_channel::unbounded();
    for (index, tx) in fixture.transactions()?.into_iter().enumerate() {
        tx_sender.send((index, Ok::<_, Infallible>(tx)))?;
    }
    drop(tx_sender);
    let (receipt_sender, _receipt_receiver) = crossbeam_channel::unbounded();
    let (execution, _, bal, _) = execute_block(
        runtime,
        &config,
        &make_db,
        Arc::new(fixture.decoded_bal()?),
        config.evm_env(block.header())?,
        config.context_for_block(&block)?,
        fixture.block.body.transactions.len(),
        tx_receiver,
        receipt_sender,
    )?;
    // The scoped worker pool has joined before execute_block returns.
    Ok(Output { execution, bal })
}

pub(crate) fn runtime(workers: usize) -> Result<Runtime> {
    ensure!((1..=64).contains(&workers), "workers must be between 1 and 64");
    let config = RuntimeConfig::default()
        .with_tokio(TokioConfig::with_worker_threads(1))
        .with_rayon(RayonConfig {
            cpu_threads: Some(1),
            rpc_threads: Some(1),
            storage_threads: Some(1),
            bal_streaming_threads: Some(workers),
            prewarming_threads: Some(1),
            proof_storage_worker_threads: Some(1),
            proof_account_worker_threads: Some(1),
            state_trie_overlay_worker_threads: Some(1),
            ..Default::default()
        });
    let runtime = RuntimeBuilder::new(config).build()?;
    let _ = runtime.bal_streaming_pool();
    Ok(runtime)
}

pub(crate) fn check(fixture: &Fixture, workers: usize) -> Result<()> {
    fixture.validate()?;
    let serial = run_serial(fixture, None)?;
    let bal = run_bal(fixture, &runtime(workers)?, None)?;
    ensure!(serial.execution.state == bal.execution.state, "state transitions differ");
    ensure!(serial.execution.result.receipts == bal.execution.result.receipts, "receipts differ");
    ensure!(serial.execution.result.gas_used == bal.execution.result.gas_used, "gas differs");
    ensure!(serial.execution.result.requests == bal.execution.result.requests, "requests differ");
    ensure!(serial.bal == bal.bal, "reconstructed BAL differs");
    fixture.assert_output(&serial)?;
    fixture.assert_output(&bal)?;
    Ok(())
}

fn canonical_json(value: &Value, bytes: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Object(map) => {
            bytes.push(b'{');
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            for (i, (key, value)) in entries.into_iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                serde_json::to_writer(&mut *bytes, key)?;
                bytes.push(b':');
                canonical_json(value, bytes)?;
            }
            bytes.push(b'}');
        }
        Value::Array(values) => {
            bytes.push(b'[');
            for (i, value) in values.iter().enumerate() {
                if i > 0 {
                    bytes.push(b',');
                }
                canonical_json(value, bytes)?;
            }
            bytes.push(b']');
        }
        _ => serde_json::to_writer(bytes, value)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Workload;

    #[test]
    fn deterministic_fixtures_and_parallel_equivalence() -> Result<()> {
        for workload in [Workload::Transfers, Workload::OracleReads] {
            let first = Fixture::generate(42, 16, workload)?;
            let second = Fixture::generate(42, 16, workload)?;
            ensure!(
                serde_json::to_vec(&first)? == serde_json::to_vec(&second)?,
                "nondeterministic fixture"
            );
            for workers in [1, 2, 4] {
                check(&first, workers)?;
            }
        }
        Ok(())
    }

    #[test]
    fn changed_bal_commitment_is_rejected() -> Result<()> {
        let mut fixture = Fixture::generate(42, 1, Workload::Transfers)?;
        fixture.block.header.block_access_list_hash = Some(B256::ZERO);
        assert_eq!(fixture.validate().unwrap_err().to_string(), "fixture BAL commitment mismatch");
        Ok(())
    }

    #[test]
    fn changed_expected_output_is_rejected() -> Result<()> {
        let mut fixture = Fixture::generate(42, 1, Workload::OracleReads)?;
        fixture.expected_output_digest = B256::ZERO;
        assert_eq!(
            check(&fixture, 2).unwrap_err().to_string(),
            "execution output differs from fixture reference"
        );
        Ok(())
    }

    #[test]
    fn counters_observe_real_storage_calls_without_changing_outputs() -> Result<()> {
        let fixture = Fixture::generate(42, 16, Workload::OracleReads)?;
        let reference = run_serial(&fixture, None)?;
        ensure!(
            reference
                .bal
                .iter()
                .any(|entry| entry.address == crate::fixture::ORACLE
                    && !entry.storage_reads.is_empty()),
            "oracle storage read missing from generated BAL"
        );
        for bal_mode in [false, true] {
            let counters = Arc::new(Counters::default());
            let output = if bal_mode {
                run_bal(&fixture, &runtime(4)?, Some(counters.clone()))?
            } else {
                run_serial(&fixture, Some(counters.clone()))?
            };
            ensure!(output.digest()? == reference.digest()?, "instrumentation changed output");
            ensure!(counters.snapshot().storage_reads > 0, "no storage calls recorded");
        }
        Ok(())
    }
}
