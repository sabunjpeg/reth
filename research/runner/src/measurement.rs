//! Consistent database-call counters and process CPU measurements.

use alloy_primitives::{Address, B256, U256};
use eyre::{ensure, Result};
use reth_revm::db::InMemoryDB;
use revm::{
    state::{AccountInfo, Bytecode},
    Database,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
pub(crate) struct Counters {
    account_reads: AtomicU64,
    storage_reads: AtomicU64,
    code_reads: AtomicU64,
    block_hash_reads: AtomicU64,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct ReadCounts {
    pub logical_reads: u64,
    pub account_reads: u64,
    pub storage_reads: u64,
    pub code_reads: u64,
    pub block_hash_reads: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CountedDb {
    inner: InMemoryDB,
    counters: Option<Arc<Counters>>,
}

impl CountedDb {
    pub(crate) const fn new(inner: InMemoryDB, counters: Option<Arc<Counters>>) -> Self {
        Self { inner, counters }
    }
}

impl Counters {
    pub(crate) fn snapshot(&self) -> ReadCounts {
        let account_reads = self.account_reads.load(Ordering::Relaxed);
        let storage_reads = self.storage_reads.load(Ordering::Relaxed);
        let code_reads = self.code_reads.load(Ordering::Relaxed);
        let block_hash_reads = self.block_hash_reads.load(Ordering::Relaxed);
        ReadCounts {
            logical_reads: account_reads + storage_reads + code_reads + block_hash_reads,
            account_reads,
            storage_reads,
            code_reads,
            block_hash_reads,
        }
    }
}

impl Database for CountedDb {
    type Error = <InMemoryDB as Database>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(counters) = &self.counters {
            counters.account_reads.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if let Some(counters) = &self.counters {
            counters.code_reads.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        if let Some(counters) = &self.counters {
            counters.storage_reads.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        if let Some(counters) = &self.counters {
            counters.block_hash_reads.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.block_hash(number)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Usage {
    pub user_ns: u64,
    pub system_ns: u64,
    pub peak_rss_bytes: u64,
}

#[cfg(unix)]
pub(crate) fn usage() -> Result<Usage> {
    let mut raw = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the entire rusage value on success. The pointer is valid
    // for the call; assume_init is reached only after checking its return value.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, raw.as_mut_ptr()) };
    ensure!(status == 0, "getrusage failed: {}", std::io::Error::last_os_error());
    let raw = unsafe { raw.assume_init() };
    let scale = if cfg!(target_os = "macos") { 1 } else { 1024 };
    Ok(Usage {
        user_ns: raw.ru_utime.tv_sec as u64 * 1_000_000_000 + raw.ru_utime.tv_usec as u64 * 1000,
        system_ns: raw.ru_stime.tv_sec as u64 * 1_000_000_000 + raw.ru_stime.tv_usec as u64 * 1000,
        peak_rss_bytes: raw.ru_maxrss as u64 * scale,
    })
}

#[cfg(not(unix))]
pub(crate) fn usage() -> Result<Usage> {
    eyre::bail!(
        "this pilot requires Unix getrusage; unsupported counters must not be reported as zero"
    )
}
