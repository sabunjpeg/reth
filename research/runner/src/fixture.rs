//! Deterministic, self-contained execution fixtures, not complete valid payloads.

use crate::replay::{run_serial, Output};
use alloy_consensus::{proofs::calculate_transaction_root, Header, SignableTransaction, TxLegacy};
use alloy_eip7928::{bal::Bal, compute_block_access_list_hash};
use alloy_eips::{
    eip2935::{HISTORY_STORAGE_ADDRESS, HISTORY_STORAGE_CODE},
    eip4788::{BEACON_ROOTS_ADDRESS, BEACON_ROOTS_CODE},
    eip7002::{WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS, WITHDRAWAL_REQUEST_PREDEPLOY_CODE},
};
use alloy_primitives::{keccak256, Address, Bytes, TxKind, B256, U256};
use eyre::{ensure, Result};
use reth_chainspec::ChainSpecBuilder;
use reth_ethereum_primitives::{Block, BlockBody, Transaction, TransactionSigned};
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives_traits::{
    crypto::secp256k1::{public_key_to_address, sign_message},
    Recovered, SealedBlock, SignerRecoverable,
};
use reth_revm::db::InMemoryDB;
use revm::state::{AccountInfo, Bytecode};
use secp256k1::{Keypair, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

/// The shared-read contract: PUSH1 0; SLOAD; POP; STOP.
pub(crate) const ORACLE_CODE: &[u8] = &[0x60, 0x00, 0x54, 0x50, 0x00];
pub(crate) const ORACLE: Address = Address::repeat_byte(0x88);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fixture {
    pub schema_version: u32,
    pub fixture_id: String,
    pub measurement_level: String,
    pub seed: u64,
    pub workload: Workload,
    pub accounts: Vec<Account>,
    pub block: Block,
    pub bal_rlp: Bytes,
    pub expected_output_digest: B256,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Workload {
    Transfers,
    OracleReads,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Account {
    pub address: Address,
    pub balance: U256,
    pub nonce: u64,
    pub code: Bytes,
    pub storage: Vec<(U256, U256)>,
}

impl Fixture {
    pub(crate) fn generate(seed: u64, tx_count: usize, workload: Workload) -> Result<Self> {
        ensure!((1..=256).contains(&tx_count), "tx count must be between 1 and 256");
        let mut accounts = vec![
            contract(BEACON_ROOTS_ADDRESS, BEACON_ROOTS_CODE.clone()),
            contract(
                WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS,
                WITHDRAWAL_REQUEST_PREDEPLOY_CODE.clone(),
            ),
            contract(HISTORY_STORAGE_ADDRESS, HISTORY_STORAGE_CODE.clone()),
        ];
        if matches!(workload, Workload::OracleReads) {
            let mut oracle = contract(ORACLE, Bytes::copy_from_slice(ORACLE_CODE));
            oracle.storage.push((U256::ZERO, U256::from(123_456)));
            accounts.push(oracle);
        }
        let secp = Secp256k1::new();
        let mut transactions = Vec::with_capacity(tx_count);
        for i in 0..tx_count {
            // These publicly reproducible keys are synthetic fixture inputs only.
            let secret = keccak256(format!("bal-research-fixture-v1:{seed}:{i}"));
            let keypair = Keypair::from_secret_key(&secp, &SecretKey::from_byte_array(&secret.0)?);
            let sender = public_key_to_address(keypair.public_key());
            accounts.push(Account {
                address: sender,
                balance: U256::from(10_u64.pow(18)),
                nonce: 0,
                code: Bytes::new(),
                storage: vec![],
            });
            let to = match workload {
                Workload::Transfers => {
                    Address::from_slice(&keccak256(format!("recipient:{seed}:{i}"))[12..])
                }
                Workload::OracleReads => ORACLE,
            };
            // Pre-fund recipients to avoid mixing account-creation gas into this control.
            if matches!(workload, Workload::Transfers) {
                accounts.push(Account {
                    address: to,
                    balance: U256::from(1),
                    nonce: 0,
                    code: Bytes::new(),
                    storage: vec![],
                });
            }
            let tx = Transaction::Legacy(TxLegacy {
                chain_id: Some(1),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100_000,
                to: TxKind::Call(to),
                value: if matches!(workload, Workload::Transfers) {
                    U256::from(100)
                } else {
                    U256::ZERO
                },
                input: Bytes::new(),
            });
            let signature = sign_message(secret, tx.signature_hash())?;
            transactions.push(tx.into_signed(signature).into());
        }
        let block = Block {
            header: Header {
                number: 1,
                timestamp: 1,
                gas_limit: 30_000_000,
                parent_beacon_block_root: Some(B256::ZERO),
                withdrawals_root: Some(alloy_consensus::EMPTY_ROOT_HASH),
                requests_hash: Some(alloy_eips::eip7685::EMPTY_REQUESTS_HASH),
                excess_blob_gas: Some(0),
                blob_gas_used: Some(0),
                transactions_root: calculate_transaction_root(&transactions),
                ..Default::default()
            },
            body: BlockBody { transactions, ommers: vec![], withdrawals: Some(vec![].into()) },
        };
        let name = match workload {
            Workload::Transfers => "transfers",
            Workload::OracleReads => "oracle-reads",
        };
        let mut fixture = Self {
            schema_version: 1,
            fixture_id: format!("{name}-{tx_count}-seed-{seed}"),
            measurement_level: "L1".into(),
            seed,
            workload,
            accounts,
            block,
            bal_rlp: Bytes::new(),
            expected_output_digest: B256::ZERO,
        };
        let reference = run_serial(&fixture, None)?;
        fixture.bal_rlp = alloy_rlp::encode(Bal::from(reference.bal.clone())).into();
        fixture.block.header.block_access_list_hash =
            Some(compute_block_access_list_hash(&reference.bal));
        fixture.block.header.gas_used = reference.execution.result.gas_used;
        fixture.expected_output_digest = reference.digest()?;
        // Commitment stamping must not change execution under this fixture's rules.
        fixture.assert_output(&run_serial(&fixture, None)?)?;
        Ok(fixture)
    }

    pub(crate) fn db(&self) -> Result<InMemoryDB> {
        let mut db = InMemoryDB::default();
        for account in &self.accounts {
            let mut info = AccountInfo::from_bytecode(Bytecode::new_raw(account.code.clone()));
            info.balance = account.balance;
            info.nonce = account.nonce;
            db.insert_account_info(account.address, info);
            for &(key, value) in &account.storage {
                db.insert_account_storage(account.address, key, value)?;
            }
        }
        Ok(db)
    }

    pub(crate) fn sealed_block(&self) -> SealedBlock<Block> {
        SealedBlock::seal_slow(self.block.clone())
    }

    pub(crate) fn transactions(&self) -> Result<Vec<Recovered<TransactionSigned>>> {
        self.block
            .body
            .transactions
            .iter()
            .cloned()
            .map(|tx| {
                let signer = tx.recover_signer()?;
                Ok(Recovered::new_unchecked(tx, signer))
            })
            .collect()
    }

    pub(crate) fn decoded_bal(&self) -> Result<alloy_eip7928::bal::DecodedBal> {
        let bal = alloy_rlp::decode_exact::<Bal>(&self.bal_rlp)?;
        ensure!(alloy_rlp::encode(&bal) == self.bal_rlp.as_ref(), "noncanonical BAL encoding");
        ensure!(
            Some(compute_block_access_list_hash(bal.as_slice()))
                == self.block.header.block_access_list_hash,
            "fixture BAL commitment mismatch"
        );
        Ok(alloy_eip7928::bal::DecodedBal::new(bal, self.bal_rlp.clone()))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.measurement_level == "L1",
            "unsupported fixture version or level"
        );
        ensure!(
            (1..=256).contains(&self.block.body.transactions.len()),
            "invalid fixture transaction count"
        );
        ensure!(
            self.block.header.transactions_root
                == calculate_transaction_root(&self.block.body.transactions),
            "fixture transaction commitment mismatch"
        );
        self.decoded_bal()?;
        self.transactions()?;
        Ok(())
    }

    pub(crate) fn assert_output(&self, output: &Output) -> Result<()> {
        ensure!(
            output.digest()? == self.expected_output_digest,
            "execution output differs from fixture reference"
        );
        ensure!(
            compute_block_access_list_hash(&output.bal)
                == self.block.header.block_access_list_hash.unwrap(),
            "reconstructed BAL differs from fixture commitment"
        );
        Ok(())
    }
}

pub(crate) fn evm_config() -> EthEvmConfig {
    // A real client creates its chain configuration once. Initialize it during untimed
    // warmups rather than rebuilding mainnet's genesis configuration in every sample.
    static CONFIG: OnceLock<EthEvmConfig> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            EthEvmConfig::new(Arc::new(ChainSpecBuilder::mainnet().amsterdam_activated().build()))
        })
        .clone()
}

const fn contract(address: Address, code: Bytes) -> Account {
    Account { address, balance: U256::ZERO, nonce: 0, code, storage: vec![] }
}
