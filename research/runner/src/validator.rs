//! Complete local payload fixtures and the pinned disk-backed engine validator.

use crate::{
    candidate::{reference_trace, Observer},
    fixture::{Account, Fixture, ORACLE},
    measurement::usage,
    read_index::{PrefixVerifier, ReadIndex},
    replay,
};
use alloy_consensus::{proofs::calculate_receipt_root, TxReceipt};
use alloy_eip7928::{bal::Bal, compute_block_access_list_hash};
use alloy_genesis::{Genesis, GenesisAccount};
use alloy_primitives::{keccak256, Bloom, Bytes, B256, U256};
use alloy_rpc_types_engine::{ExecutionData, ExecutionPayload};
use eyre::{ensure, Result};
use reth_chain_state::CanonicalInMemoryState;
use reth_chainspec::{ChainSpec, ChainSpecBuilder};
use reth_engine_primitives::{NoopInvalidBlockHook, PayloadValidator, TreeConfig};
use reth_engine_tree::tree::payload_processor::bal::research::ResearchObserver;
use reth_engine_tree::tree::{
    payload_validator::{BasicEngineValidator, BlockOrPayload, TreeCtx},
    EngineApiTreeState,
};
use reth_ethereum_consensus::EthBeaconConsensus;
use reth_ethereum_engine_primitives::EthEngineTypes;
use reth_ethereum_payload_builder::EthereumExecutionPayloadValidator;
use reth_ethereum_primitives::Block;
use reth_evm_ethereum::EthEvmConfig;
use reth_payload_primitives::NewPayloadError;
use reth_primitives_traits::SealedBlock;
use reth_provider::{
    providers::BlockchainProvider,
    test_utils::{create_test_provider_factory_with_chain_spec, MockNodeTypesWithDB},
    HashedPostStateProvider, ProviderFactory, StateRootProvider,
};
use reth_storage_overlay::OverlayManager;
use reth_tasks::Runtime;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiskFixture {
    pub execution: Fixture,
    pub genesis: Genesis,
    pub parent_hash: B256,
    pub phantom_count: usize,
    pub index_rlp: Bytes,
    pub envelope: Envelope,
    #[serde(skip)]
    pub producer_trace_ns: u64,
    #[serde(skip)]
    pub producer_encode_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Envelope {
    block_hash: B256,
    bal_hash: B256,
    index_hash: B256,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
pub(crate) enum DiskVariant {
    A,
    B,
    CAttr,
    C,
    BStock,
}

/// Uses exactly the payload-conversion implementation used by `EthereumEngineValidator`.
#[derive(Debug, Clone)]
struct PayloadCheck(EthereumExecutionPayloadValidator<ChainSpec>);

impl PayloadValidator<EthEngineTypes> for PayloadCheck {
    type Block = Block;

    fn convert_payload_to_block(
        &self,
        payload: ExecutionData,
    ) -> Result<SealedBlock<Block>, NewPayloadError> {
        self.0.ensure_well_formed_payload(payload).map_err(Into::into)
    }
}

type Factory = ProviderFactory<MockNodeTypesWithDB>;

impl DiskFixture {
    pub(crate) fn generate(input: &Fixture) -> Result<Self> {
        let mut genesis = ChainSpecBuilder::mainnet().amsterdam_activated().build().genesis;
        genesis.alloc = input
            .accounts
            .iter()
            .map(|account| (account.address, genesis_account(account)))
            .collect();
        genesis.difficulty = U256::ZERO;
        genesis.nonce = 0;
        genesis.timestamp = 0;
        genesis.extra_data = Bytes::new();
        genesis.gas_limit = 30_000_000;
        genesis.base_fee_per_gas = Some(1);
        genesis.blob_gas_used = Some(0);
        genesis.excess_blob_gas = Some(0);
        genesis.slot_number = Some(0);
        let spec = chain_spec(&genesis);
        let provider = database(spec.clone())?;
        let mut execution = input.clone();
        execution.block.header.parent_hash = spec.genesis_hash();
        execution.block.header.timestamp = 12;
        execution.block.header.base_fee_per_gas = Some(1);
        execution.block.header.slot_number = Some(1);
        let output = replay::execute_serial(
            &execution,
            reth_revm::database::StateProviderDatabase::new(
                reth_provider::EvmStateProviderAdapter(provider.latest()?),
            ),
        )?;
        ensure!(
            output.bal == replay::run_serial(&execution, None)?.bal,
            "disk and memory reference access sets differ"
        );
        let receipts = output
            .execution
            .result
            .receipts
            .iter()
            .map(TxReceipt::with_bloom_ref)
            .collect::<Vec<_>>();
        execution.block.header.receipts_root = calculate_receipt_root(&receipts);
        execution.block.header.logs_bloom =
            receipts.iter().fold(Bloom::ZERO, |acc, receipt| acc | receipt.bloom_ref());
        execution.block.header.gas_used = output.execution.result.gas_used;
        execution.block.header.requests_hash =
            Some(output.execution.result.requests.requests_hash());
        let parent = provider.latest()?;
        execution.block.header.state_root =
            parent.state_root(parent.hashed_post_state(&output.execution.state)?)?;
        execution.bal_rlp = alloy_rlp::encode(Bal::from(output.bal.clone())).into();
        execution.block.header.block_access_list_hash =
            Some(compute_block_access_list_hash(&output.bal));
        execution.expected_output_digest = output.digest()?;
        execution.assert_output(&replay::execute_serial(
            &execution,
            reth_revm::database::StateProviderDatabase::new(
                reth_provider::EvmStateProviderAdapter(provider.latest()?),
            ),
        )?)?;
        let start = Instant::now();
        let trace = reference_trace(&execution)?;
        let producer_trace_ns = start.elapsed().as_nanos() as u64;
        let start = Instant::now();
        let index_rlp = ReadIndex::from_trace(&Bal::from(output.bal), &trace, 1).encode();
        let producer_encode_ns = start.elapsed().as_nanos() as u64;
        let envelope = Envelope {
            block_hash: execution.sealed_block().hash(),
            bal_hash: keccak256(&execution.bal_rlp),
            index_hash: keccak256(&index_rlp),
        };
        Ok(Self {
            execution,
            genesis,
            parent_hash: spec.genesis_hash(),
            phantom_count: 0,
            index_rlp,
            envelope,
            producer_trace_ns,
            producer_encode_ns,
        })
    }

    pub(crate) fn phantom(&self, count: usize) -> Result<Self> {
        self.phantom_at(count, 1)
    }

    pub(crate) fn phantom_at(&self, count: usize, claim: u32) -> Result<Self> {
        let mut result = self.clone();
        let mut bal = alloy_rlp::decode_exact::<Bal>(&result.execution.bal_rlp)?.into_inner();
        let oracle = bal
            .iter_mut()
            .find(|account| account.address == ORACLE)
            .ok_or_else(|| eyre::eyre!("phantom fixture requires oracle workload"))?;
        for i in 1..=count {
            let slot = U256::from(i);
            ensure!(
                !oracle.storage_reads.contains(&slot)
                    && !oracle.storage_changes.iter().any(|change| change.slot == slot),
                "mutation overlaps genuine access"
            );
            oracle.storage_reads.push(slot);
        }
        oracle.storage_reads.sort_unstable();
        let bal = Bal::from(bal);
        bal.validate_gas_limit(result.execution.block.header.gas_limit)?;
        result.execution.bal_rlp = alloy_rlp::encode(&bal).into();
        result.execution.block.header.block_access_list_hash =
            Some(compute_block_access_list_hash(bal.as_slice()));
        result.execution.fixture_id =
            format!("{}-phantoms-{count}-claim-{claim}", self.execution.fixture_id);
        result.phantom_count = count;
        let genuine = ReadIndex::decode(
            &self.index_rlp,
            &alloy_rlp::decode_exact::<Bal>(&self.execution.bal_rlp)?,
            self.execution.block.body.transactions.len(),
        )?;
        let trace =
            genuine.0.into_iter().map(|entry| ((entry.address, entry.key), entry.first)).collect();
        result.index_rlp = ReadIndex::from_trace(&bal, &trace, claim).encode();
        result.envelope = Envelope {
            block_hash: result.execution.sealed_block().hash(),
            bal_hash: keccak256(&result.execution.bal_rlp),
            index_hash: keccak256(&result.index_rlp),
        };
        Ok(result)
    }

    pub(crate) fn payload(&self) -> ExecutionData {
        let (payload, sidecar) = ExecutionPayload::from_block_unchecked_with_bal(
            self.execution.sealed_block().hash(),
            &self.execution.block,
            self.execution.bal_rlp.clone(),
        );
        ExecutionData { payload, sidecar }
    }
}

fn genesis_account(account: &Account) -> GenesisAccount {
    GenesisAccount {
        nonce: Some(account.nonce),
        balance: account.balance,
        code: Some(account.code.clone()),
        storage: Some(
            account
                .storage
                .iter()
                .map(|(key, value)| {
                    (B256::from(key.to_be_bytes::<32>()), B256::from(value.to_be_bytes::<32>()))
                })
                .collect::<BTreeMap<_, _>>(),
        ),
        ..Default::default()
    }
}

pub(crate) fn chain_spec(genesis: &Genesis) -> Arc<ChainSpec> {
    Arc::new(ChainSpecBuilder::mainnet().amsterdam_activated().genesis(genesis.clone()).build())
}

fn database(spec: Arc<ChainSpec>) -> Result<Factory> {
    let provider = create_test_provider_factory_with_chain_spec(spec);
    reth_db_common::init::init_genesis(&provider)?;
    Ok(provider)
}

type Validator =
    BasicEngineValidator<BlockchainProvider<MockNodeTypesWithDB>, EthEvmConfig, PayloadCheck>;

pub(crate) struct Harness {
    validator: Validator,
    spec: Arc<ChainSpec>,
    runtime: Runtime,
}

impl Harness {
    pub(crate) fn new(
        fixture: &DiskFixture,
        variant: DiskVariant,
        runtime: Runtime,
    ) -> Result<Self> {
        let spec = chain_spec(&fixture.genesis);
        let provider = database(spec.clone())?;
        let overlay = OverlayManager::new(runtime.state_trie_overlay_worker_pool());
        let serial = variant == DiskVariant::A;
        let config = TreeConfig::default()
            .with_has_enough_parallelism(true)
            .with_state_provider_metrics(variant != DiskVariant::BStock)
            .without_bal_parallel_execution(serial)
            .without_bal_parallel_state_root(serial)
            .without_bal_batch_io(serial);
        let validator = BasicEngineValidator::new(
            BlockchainProvider::new(provider)?,
            Arc::new(EthBeaconConsensus::new(spec.clone())),
            EthEvmConfig::new(spec.clone()),
            PayloadCheck(EthereumExecutionPayloadValidator::new(spec.clone())),
            config,
            Box::new(NoopInvalidBlockHook::default()),
            overlay,
            runtime.clone(),
        );
        // Initialize shared client pools before warmups and timed per-block work.
        let _ = runtime.cpu_pool();
        let _ = runtime.storage_pool();
        let _ = runtime.proof_storage_worker_pool();
        let _ = runtime.proof_account_worker_pool();
        let _ = runtime.prewarming_pool();
        let _ = runtime.bal_streaming_pool();
        drain(&runtime);
        Ok(Self { validator, spec, runtime })
    }

    pub(crate) fn attempt(
        &mut self,
        fixture: &DiskFixture,
        variant: DiskVariant,
    ) -> Result<serde_json::Value> {
        let Self { validator, spec, runtime } = self;
        let overlay = OverlayManager::new(runtime.state_trie_overlay_worker_pool());
        let parent = spec.sealed_genesis_header();
        let canonical = CanonicalInMemoryState::with_head(parent.clone(), None, None);
        let mut tree = EngineApiTreeState::research_empty(parent.num_hash(), overlay.clone());
        validator.research_prepare(overlay, None);
        let payload = fixture.payload();
        let before = usage()?;
        let start = Instant::now();
        let candidate = matches!(variant, DiskVariant::C | DiskVariant::CAttr);
        let decode_start = Instant::now();
        let verifier = if candidate {
            ensure!(
                fixture.envelope.block_hash == fixture.execution.sealed_block().hash()
                    && fixture.envelope.bal_hash == keccak256(&fixture.execution.bal_rlp)
                    && fixture.envelope.index_hash == keccak256(&fixture.index_rlp),
                "research envelope commitment mismatch"
            );
            let bal = alloy_rlp::decode_exact::<Bal>(&fixture.execution.bal_rlp)?;
            Some(PrefixVerifier::new(ReadIndex::decode(
                &fixture.index_rlp,
                &bal,
                fixture.execution.block.body.transactions.len(),
            )?))
        } else {
            None
        };
        let index_decode_verify_ns = candidate.then(|| decode_start.elapsed().as_nanos() as u64);
        let observer = Arc::new(Observer::new(
            start,
            verifier,
            variant == DiskVariant::C,
            fixture.phantom_count,
        ));
        if variant != DiskVariant::BStock {
            validator.research_observer(Some(observer.clone()));
        }
        let result = validator.validate_block_with_state::<EthEngineTypes>(
            BlockOrPayload::Payload(payload),
            TreeCtx::new(&mut tree, &canonical),
        );
        let validation_return_ns = start.elapsed().as_nanos() as u64;
        if result.is_err() && observer.snapshot()["t_detect_ns"].is_null() {
            observer.rejected("validator_return");
        }
        // Every named queue is FIFO. Upstream cancellation tests use these same root barriers.
        // Prewarm joins its nested BAL prefetch/streaming work; root barriers precede proof/drop
        // barriers so producers cannot enqueue more work after the latter barriers complete.
        drain(runtime);
        let t_stop_ns = start.elapsed().as_nanos() as u64;
        let after = usage()?;
        let (verdict, reason, output_digest) = match &result {
            Ok(output) => {
                ensure!(
                    output.executed_block.recovered_block().header().state_root
                        == fixture.execution.block.header.state_root,
                    "accepted root mismatch"
                );
                let execution = output.executed_block.execution_outcome().clone();
                let bal = alloy_rlp::decode_exact::<Bal>(&fixture.execution.bal_rlp)?.into_inner();
                let observed = replay::Output { execution, bal };
                fixture.execution.assert_output(&observed)?;
                ("accepted", None, Some(observed.digest()?))
            }
            Err(error) => ("rejected", Some(format!("{error:?}")), None),
        };
        let mut record = observer.snapshot();
        let rejection_category = match &result {
            Err(reth_engine_tree::tree::error::InsertPayloadError::Block(error))
                if matches!(
                    error.kind(),
                    reth_engine_primitives::InsertBlockErrorKind::Consensus(
                        reth_consensus::ConsensusError::BlockAccessListHashMismatch(_)
                    )
                ) =>
            {
                Some("bal_hash_mismatch")
            }
            Err(_) if record["rejection_phase"] == "first_access_verification" => {
                Some("first_access_mismatch")
            }
            Err(_) => Some("other"),
            Ok(_) => None,
        };
        let fields = json!({
            "schema_version": 1, "measurement_level":"L2", "variant":variant, "fixture_id":fixture.execution.fixture_id,
            "seed": fixture.execution.seed, "phantom_count":fixture.phantom_count, "verdict":verdict,
            "rejection_reason":reason, "rejection_category":rejection_category, "validation_return_ns":validation_return_ns,
            "t_start_ns":0, "t_stop_ns":t_stop_ns, "wall_ns":t_stop_ns,
            "cpu_user_ns":after.user_ns - before.user_ns, "cpu_system_ns":after.system_ns - before.system_ns,
            "peak_rss_bytes":after.peak_rss_bytes, "peak_rss_scope":"process_lifetime_including_setup_and_warmups",
            "output_digest":output_digest, "gas_used":(verdict == "accepted").then_some(fixture.execution.block.header.gas_used),
            "fixture_expected_gas_used":fixture.execution.block.header.gas_used,
            "tx_count":fixture.execution.block.body.transactions.len(), "bal_bytes":fixture.execution.bal_rlp.len(),
            "index_bytes":if candidate {fixture.index_rlp.len()} else {0}, "fixture_index_bytes":fixture.index_rlp.len(), "index_decode_verify_ns":index_decode_verify_ns,
            "producer_trace_ns":null, "producer_encode_ns":null,
            "cache_hits":null, "cache_misses":null, "physical_read_bytes":null,
            "prewarm_workers":if variant == DiskVariant::A {usize::from(fixture.execution.block.body.transactions.len() >= reth_engine_tree::tree::payload_processor::SMALL_BLOCK_TX_THRESHOLD)} else {128},
            "state_root_workers":1, "state_root_workers_scope":"named root coordinator; proof and hashing workers recorded separately",
            "state_root_policy":if variant == DiskVariant::A {"serial"} else {"BAL_parallel_sparse_trie"},
            "proof_storage_workers":1, "proof_account_workers":1, "state_overlay_workers":1,
            "database_backend":"local_MDBX_Reth_provider_with_RocksDB_and_static_files",
            "cache_condition":"fresh_execution_cache_and_overlay_each_replay; OS_cache_uncontrolled",
            "block_hash":fixture.execution.sealed_block().hash(),
            "bal_hash":keccak256(&fixture.execution.bal_rlp), "index_hash":keccak256(&fixture.index_rlp),
            "scope":"complete Ethereum engine payload validation through named-task drain; ordinary transaction pilot",
        });
        record.as_object_mut().expect("record").extend(fields.as_object().expect("fields").clone());
        if variant == DiskVariant::BStock {
            for name in [
                "prewarm_storage_requests_started",
                "prewarm_storage_requests_completed",
                "prewarm_storage_requests_skipped",
                "phantom_lookups_started",
                "phantom_lookups_completed",
                "phantom_lookups_skipped",
            ] {
                record[name] = serde_json::Value::Null;
            }
        }
        Ok(record)
    }
}

fn drain(runtime: &Runtime) {
    // Join producers before their downstream queues. Account proof work can enqueue storage
    // requests; root and deferred-overlay work can enqueue hashing/proof work.
    for name in [
        "payload-convert",
        "tx-iterator",
        "prewarm",
        "prewarm-txs",
        "receipt-root",
        "hash-post-state",
        "serial-root",
        "sparse-trie",
        "deferred-trie",
        "trie-hashing",
        "account-workers",
        "storage-workers",
    ] {
        runtime.spawn_blocking_named(name, || {}).get();
    }
    runtime.bal_streaming_pool().clear();
    runtime.proof_storage_worker_pool().clear();
    runtime.proof_account_worker_pool().clear();
    runtime.prewarming_pool().clear();
    runtime.state_trie_overlay_worker_pool().clear();
    runtime.spawn_blocking_named("drop", || {}).get();
}

pub(crate) fn reproduce(
    input: &Fixture,
    workers: usize,
    phantoms: usize,
    output: &Path,
) -> Result<()> {
    ensure!(!output.exists(), "choose a new output directory");
    reth_fs_util::create_dir_all(output)?;
    let valid = DiskFixture::generate(input)?;
    let invalid = valid.phantom(phantoms)?;
    reth_fs_util::write_json_file(&output.join("valid.json"), &valid)?;
    reth_fs_util::write_json_file(&output.join("phantom.json"), &invalid)?;
    let runtime = replay::runtime(workers)?;
    let mut records = vec![];
    for (fixture, variant) in [
        (&valid, DiskVariant::A),
        (&valid, DiskVariant::BStock),
        (&valid, DiskVariant::B),
        (&valid, DiskVariant::C),
        (&invalid, DiskVariant::B),
        (&invalid, DiskVariant::CAttr),
        (&invalid, DiskVariant::C),
    ] {
        let record = Harness::new(fixture, variant, runtime.clone())?.attempt(fixture, variant)?;
        println!("{}", serde_json::to_string(&record)?);
        records.push(record);
    }
    reth_fs_util::write_json_file(&output.join("reproduction.json"), &records)?;
    ensure!(
        records[..4].iter().all(|record| record["verdict"] == "accepted"),
        "valid disk fixture rejected"
    );
    ensure!(
        records[4..].iter().all(|record| record["verdict"] == "rejected"),
        "phantom fixture unexpectedly accepted"
    );
    ensure!(
        records[5]["canonical_index_at_detection"] == 1
            && records[6]["canonical_index_at_detection"] == 1,
        "C did not reject at the claimed index"
    );
    Ok(())
}

pub(crate) fn generate_cases(input: &Fixture, directory: &Path) -> Result<()> {
    ensure!(!directory.exists(), "choose a new fixture directory");
    reth_fs_util::create_dir_all(directory)?;
    let valid = DiskFixture::generate(input)?;
    reth_fs_util::write_json_file(&directory.join("valid.json"), &valid)?;
    let mut cases = vec![json!({"file":"valid.json", "phantom_count":0, "claim":null})];
    if matches!(input.workload, crate::fixture::Workload::OracleReads) {
        let bal = alloy_rlp::decode_exact::<Bal>(&valid.execution.bal_rlp)?;
        let remaining = valid.execution.block.header.gas_limit
            / alloy_eip7928::constants::ITEM_COST as u64
            - bal.total_bal_items();
        for percent in [10_u64, 50] {
            let count = (remaining * percent / 100) as usize;
            for claim in [
                1,
                input.block.body.transactions.len() as u32 / 2,
                input.block.body.transactions.len() as u32 + 1,
            ] {
                let file = format!("phantom-{percent}-claim-{claim}.json");
                let fixture = valid.phantom_at(count, claim)?;
                reth_fs_util::write_json_file(&directory.join(&file), &fixture)?;
                cases.push(json!({"file":file,"phantom_count":count,"claim":claim,"fraction_remaining_item_budget":percent as f64 / 100.0}));
            }
        }
    }
    reth_fs_util::write_json_file(
        &directory.join("generation.json"),
        &json!({"cases":cases,"producer_trace_ns":valid.producer_trace_ns,"producer_encode_ns":valid.producer_encode_ns,"producer_scope":"independent full serial execution to collect trace, plus index encoding; outside validation timer"}),
    )?;
    Ok(())
}

pub(crate) fn sample(
    path: &Path,
    variant: DiskVariant,
    workers: usize,
    warmups: usize,
) -> Result<serde_json::Value> {
    ensure!(!cfg!(debug_assertions), "disk measurements require a release build");
    ensure!((1..=100).contains(&warmups), "warmups must be between 1 and 100");
    let fixture: DiskFixture = reth_fs_util::read_json_file(path)?;
    let mut harness = Harness::new(&fixture, variant, replay::runtime(workers)?)?;
    for _ in 0..warmups {
        check_verdict(&fixture, variant, &harness.attempt(&fixture, variant)?)?;
    }
    let mut record = harness.attempt(&fixture, variant)?;
    check_verdict(&fixture, variant, &record)?;
    record["execution_workers"] = json!(if variant == DiskVariant::A {
        1
    } else {
        workers.min(fixture.execution.block.body.transactions.len())
    });
    record["configured_bal_workers"] = json!(workers);
    Ok(record)
}

fn check_verdict(
    fixture: &DiskFixture,
    variant: DiskVariant,
    record: &serde_json::Value,
) -> Result<()> {
    if fixture.phantom_count == 0 {
        ensure!(record["verdict"] == "accepted", "truthful fixture did not pass: {record}");
    } else {
        ensure!(record["verdict"] == "rejected", "phantom fixture did not reject: {record}");
        if matches!(variant, DiskVariant::C | DiskVariant::CAttr) {
            ensure!(
                record["rejection_phase"] == "first_access_verification",
                "candidate failed outside semantic verifier: {record}"
            );
        } else {
            ensure!(
                record["rejection_category"] == "bal_hash_mismatch",
                "stock rejection was not the intended semantic BAL mismatch: {record}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Workload;

    #[test]
    fn full_validator_correctness_canonical_rejection_and_next_block() -> Result<()> {
        let valid = DiskFixture::generate(&Fixture::generate(42, 16, Workload::OracleReads)?)?;
        let second = DiskFixture::generate(&Fixture::generate(42, 16, Workload::OracleReads)?)?;
        assert_eq!(serde_json::to_vec(&valid)?, serde_json::to_vec(&second)?);
        let runtime = replay::runtime(4)?;
        for variant in [DiskVariant::A, DiskVariant::BStock, DiskVariant::B, DiskVariant::C] {
            let mut harness = Harness::new(&valid, variant, runtime.clone())?;
            let record = harness.attempt(&valid, variant)?;
            check_verdict(&valid, variant, &record)?;
            assert_eq!(record["output_digest"], json!(valid.execution.expected_output_digest));
            if variant == DiskVariant::C {
                for claim in [1, 8, 17] {
                    let invalid = valid.phantom_at(100, claim)?;
                    let record = harness.attempt(&invalid, variant)?;
                    check_verdict(&invalid, variant, &record)?;
                    assert_eq!(record["canonical_index_at_detection"], claim);
                    assert!(
                        record["t_stop_ns"].as_u64().unwrap()
                            >= record["t_detect_ns"].as_u64().unwrap()
                    );
                    assert_eq!(
                        record["phantom_lookups_completed"].as_u64().unwrap()
                            + record["phantom_lookups_skipped"].as_u64().unwrap(),
                        100
                    );
                    check_verdict(&valid, variant, &harness.attempt(&valid, variant)?)?;
                }
                let mut malformed = valid.clone();
                malformed.envelope.index_hash = B256::ZERO;
                assert_eq!(
                    harness.attempt(&malformed, variant).unwrap_err().to_string(),
                    "research envelope commitment mismatch"
                );
                check_verdict(&valid, variant, &harness.attempt(&valid, variant)?)?;
            } else if matches!(variant, DiskVariant::B | DiskVariant::BStock) {
                let invalid = valid.phantom(100)?;
                let record = harness.attempt(&invalid, variant)?;
                check_verdict(&invalid, variant, &record)?;
                if variant == DiskVariant::B {
                    assert_eq!(record["canonical_index_at_detection"], 17);
                    assert_eq!(record["phantom_lookups_completed"], 100);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn false_sidecar_on_valid_standard_block_is_candidate_only_rejection() -> Result<()> {
        let valid = DiskFixture::generate(&Fixture::generate(42, 16, Workload::OracleReads)?)?;
        let bal = alloy_rlp::decode_exact::<Bal>(&valid.execution.bal_rlp)?;
        let runtime = replay::runtime(4)?;
        let mut baseline = Harness::new(&valid, DiskVariant::B, runtime.clone())?;
        let mut candidate = Harness::new(&valid, DiskVariant::C, runtime)?;
        for (false_first, detection) in [(0, 0), (2, 1)] {
            let mut modified = valid.clone();
            let mut index = ReadIndex::decode(&modified.index_rlp, &bal, 16)?;
            index
                .0
                .iter_mut()
                .find(|entry| entry.address == ORACLE && entry.key == U256::ZERO)
                .unwrap()
                .first = false_first;
            modified.index_rlp = index.encode();
            modified.envelope.index_hash = keccak256(&modified.index_rlp);
            assert_eq!(baseline.attempt(&modified, DiskVariant::B)?["verdict"], "accepted");
            let record = candidate.attempt(&modified, DiskVariant::C)?;
            assert_eq!(record["verdict"], "rejected");
            assert_eq!(record["rejection_category"], "first_access_mismatch");
            assert_eq!(record["canonical_index_at_detection"], detection);
            assert_eq!(candidate.attempt(&valid, DiskVariant::C)?["verdict"], "accepted");
        }
        Ok(())
    }

    #[test]
    fn stock_retains_hash_decode_and_item_limit_guards() -> Result<()> {
        let valid = DiskFixture::generate(&Fixture::generate(42, 16, Workload::OracleReads)?)?;
        let mut harness = Harness::new(&valid, DiskVariant::B, replay::runtime(4)?)?;
        let mut bad_hash = valid.clone();
        bad_hash.execution.block.header.block_access_list_hash = Some(B256::ZERO);
        let mut malformed = valid.clone();
        malformed.execution.bal_rlp = Bytes::from(vec![0xff]);
        let mut oversized = valid.clone();
        let mut bal = alloy_rlp::decode_exact::<Bal>(&valid.execution.bal_rlp)?.into_inner();
        let oracle = bal.iter_mut().find(|entry| entry.address == ORACLE).unwrap();
        oracle.storage_reads.extend((1..=15_000).map(U256::from));
        let bal = Bal::from(bal);
        assert!(bal.validate_gas_limit(valid.execution.block.header.gas_limit).is_err());
        oversized.execution.bal_rlp = alloy_rlp::encode(&bal).into();
        oversized.execution.block.header.block_access_list_hash =
            Some(keccak256(&oversized.execution.bal_rlp));
        // Payload conversion is concurrent with execution, so a wrong block hash need not
        // stop speculative execution before its first canonical boundary.
        for (invalid, prefix) in [(bad_hash, None), (malformed, Some(0)), (oversized, Some(0))] {
            let record = harness.attempt(&invalid, DiskVariant::B)?;
            assert_eq!(record["verdict"], "rejected");
            assert_eq!(record["rejection_category"], "other");
            if let Some(prefix) = prefix {
                assert_eq!(record["last_verified_canonical_index"], prefix);
            }
            assert_eq!(record["phantom_lookups_started"], 0);
            assert_eq!(harness.attempt(&valid, DiskVariant::B)?["verdict"], "accepted");
        }
        Ok(())
    }
}
