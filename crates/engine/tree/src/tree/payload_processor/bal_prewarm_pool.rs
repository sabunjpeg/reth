//! BAL read-set prewarming pool.

use alloy_primitives::{Address, StorageKey};
use reth_execution_cache::{CachedStateProvider, ExecutionCache, TxPoolPrewarmCacheSnapshot};
use reth_provider::{EvmStateProvider, EvmStateProviderBox, ProviderResult};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::JoinHandle,
};
use tokio::sync::oneshot;
use tracing::trace;

/// Builds a fresh EVM provider over the block's parent state. Type-erased so the pool is not
/// generic over the provider factory; each worker builds its own per block.
pub type BuildProviderFn = dyn Fn() -> ProviderResult<EvmStateProviderBox> + Send + Sync;

/// A single warm request: an account and a batch of its storage slots, or a batch of storage slots
/// on their own.
enum PrewarmTarget {
    Account(Address, Box<[StorageKey]>),
    Storage(Address, Box<[StorageKey]>),
}

/// A message in a worker's queue. The per-block lifecycle is explicit and ordered (the queue is
/// FIFO): one `BeginBlock`, then the worker's share of `Warm`s, then one `EndBlock`.
enum PrewarmMsg {
    /// Installs one immutable block observer before its provider and queued warming work.
    #[cfg(feature = "bal-research")]
    ResearchBegin(Option<Arc<dyn super::bal::research::ResearchObserver>>),
    /// Open a read txn for the new block: build a provider over the parent state and hold it.
    BeginBlock {
        build: Arc<BuildProviderFn>,
        caches: ExecutionCache,
        txpool_snapshot: Option<TxPoolPrewarmCacheSnapshot>,
    },
    /// Warm one target into the held provider's cache. Ignored if no provider is held.
    Warm(PrewarmTarget),
    /// Drop the held provider (and its read txn).
    EndBlock(Arc<SendOnDrop>),
}

/// Long-lived pool of blocking threads that warm the BAL read-set into the shared execution cache.
#[derive(Debug)]
pub struct BalPrewarmPool {
    /// One queue per worker. `BeginBlock`/`EndBlock` are broadcast to all; `Warm`s round-robin.
    workers: Vec<crossbeam_channel::Sender<PrewarmMsg>>,
    /// Round-robin cursor for distributing warm requests across workers.
    next: AtomicUsize,
    _handles: Vec<JoinHandle<()>>,
}

impl BalPrewarmPool {
    /// Registers a block observer on each FIFO queue without changing initial work ordering.
    #[cfg(feature = "bal-research")]
    pub(crate) fn research_begin_block(
        &self,
        observer: Option<Arc<dyn super::bal::research::ResearchObserver>>,
    ) {
        for worker in &self.workers {
            let _ = worker.send(PrewarmMsg::ResearchBegin(observer.clone()));
        }
    }
    /// Spawns `num_threads` long-lived blocking worker threads. Owned by the
    /// [`PayloadProcessor`](super::PayloadProcessor); the threads exit when the pool is dropped.
    pub fn new(num_threads: usize) -> Arc<Self> {
        let mut workers = Vec::with_capacity(num_threads);
        let mut handles = Vec::with_capacity(num_threads);
        for i in 0..num_threads {
            let (tx, rx) = crossbeam_channel::unbounded::<PrewarmMsg>();
            workers.push(tx);
            handles.push(
                std::thread::Builder::new()
                    .name(format!("bal-prewarm-{i:03}"))
                    .spawn(move || prewarm_loop(rx))
                    .expect("spawn bal-prewarm thread"),
            );
        }
        trace!(target: "engine::tree::bal_prewarm_pool", num_threads, "BalPrewarmPool spawned");
        Arc::new(Self { workers, next: AtomicUsize::new(0), _handles: handles })
    }

    /// Begins a block: hands every worker the provider builder and shared cache so each opens its
    /// own read txn over the parent state. Pair with [`end_block`](Self::end_block).
    pub fn begin_block(
        &self,
        build: Arc<BuildProviderFn>,
        caches: ExecutionCache,
        txpool_snapshot: Option<TxPoolPrewarmCacheSnapshot>,
    ) {
        for worker in &self.workers {
            let _ = worker.send(PrewarmMsg::BeginBlock {
                build: build.clone(),
                caches: caches.clone(),
                txpool_snapshot: txpool_snapshot.clone(),
            });
        }
    }

    /// Fire-and-forget: warm an account and its storage slots.
    ///
    /// The slots are dispatched in `WARM_BATCH_SIZE` chunks that are distributed independently,
    /// so a single account with a large read-set does not serialize onto one worker;
    /// [`end_block`](Self::end_block) waits for the slowest queue.
    pub fn warm_account(&self, addr: Address, slots: impl IntoIterator<Item = StorageKey>) {
        let mut slots = slots.into_iter();
        let mut batch: Box<[StorageKey]> = slots.by_ref().take(WARM_BATCH_SIZE).collect();
        self.send_warm(PrewarmTarget::Account(addr, batch));

        loop {
            batch = slots.by_ref().take(WARM_BATCH_SIZE).collect();
            if batch.is_empty() {
                break
            }
            self.send_warm(PrewarmTarget::Storage(addr, batch));
        }
    }

    /// Ends the block: every worker drops its provider (and read txn) once it has drained the warm
    /// requests queued ahead of this message.
    ///
    /// Blocks until all workers processed the end block message.
    pub fn end_block(&self) {
        let (tx, rx) = oneshot::channel();
        let tx = Arc::new(SendOnDrop { sender: Some(tx) });

        for worker in &self.workers {
            let _ = worker.send(PrewarmMsg::EndBlock(tx.clone()));
        }

        drop(tx);
        rx.blocking_recv().expect("BAL prewarm pool dropped without signaling completion");
    }

    fn send_warm(&self, target: PrewarmTarget) {
        let i = self.next.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let _ = self.workers[i].send(PrewarmMsg::Warm(target));
    }
}

/// Number of warming threads.
///
/// The work performed on those threads boils down mostly to MDBX reads. An MDBX read consists of
/// a tree traversal and major page faults causing I/O.
///
/// In order to utilize the parallelism of `NVMe` we have to give it enough work, or equally,
/// maintain a high queue depth. Modern `NVMe` devices require in between 64-128 requests in-flight
/// to achieve its peak performance. Ideally we don't grow past that but it's OK to do so, it just
/// means that a request is going to wait in the `NVMe` queue rather than in memory.
///
/// MDBX piggy-backs on the OS page cache for its buffers. Oftentimes, the hit rate reaches 90-99%
/// hit rate. At that point, the workload can be classified as CPU-bound. In that case, having
/// a high number of threads is counterproductive due to the effects of context switching, core
/// migration, contention, etc.
///
/// However, that overhead is considered negligible compared to the benefits of fully utilizing
/// `NVMe` resources. For example, with request latency of 100µs, 100k IO requests the expected
/// time to finish is 312.5ms at QD=32 and 156.26ms at QD=64.
///
/// This should explain why this particular value is picked.
pub const DEFAULT_BAL_PREWARM_THREADS: usize = 128;

/// Number of storage slots carried by one warm message.
///
/// Batching amortizes the send over many slots and hands the worker a run of slots that live
/// close together in the storage table, while the cap keeps enough messages in flight to saturate
/// the workers on blocks whose read-set is concentrated in a few accounts.
const WARM_BATCH_SIZE: usize = 8;

fn prewarm_loop(rx: crossbeam_channel::Receiver<PrewarmMsg>) {
    // The provider (and its MDBX read txn) held for the current block, between `BeginBlock` and
    // `EndBlock`. `None` while idle, so no read txn is pinned across the inter-block gap.
    let mut provider: Option<CachedStateProvider<EvmStateProviderBox>> = None;
    #[cfg(feature = "bal-research")]
    let mut research_observer: Option<Arc<dyn super::bal::research::ResearchObserver>> = None;

    // Blocks when idle; the channel disconnects (and the loop ends) when the pool is dropped.
    while let Ok(msg) = rx.recv() {
        match msg {
            #[cfg(feature = "bal-research")]
            PrewarmMsg::ResearchBegin(observer) => research_observer = observer,
            PrewarmMsg::BeginBlock { build, caches, txpool_snapshot } => {
                provider = match (build)() {
                    Ok(inner) => Some(
                        CachedStateProvider::new_prewarm(inner, caches)
                            .with_txpool_snapshot(txpool_snapshot),
                    ),
                    Err(err) => {
                        trace!(target: "engine::tree::bal_prewarm_pool", %err, "failed to build provider");
                        None
                    }
                };
            }
            PrewarmMsg::Warm(target) => {
                let Some(provider) = provider.as_ref() else { continue };
                match target {
                    PrewarmTarget::Account(addr, slots) => {
                        #[cfg(feature = "bal-research")]
                        if research_observer.as_ref().is_some_and(|observer| observer.cancelled()) {
                            for &slot in &slots {
                                if let Some(observer) = &research_observer {
                                    observer.prefetch(addr, slot.into(), false, true);
                                }
                            }
                            continue;
                        }
                        let _ = provider.basic_account(&addr);
                        for &slot in &slots {
                            #[cfg(feature = "bal-research")]
                            if research_prefetch(provider, research_observer.as_ref(), addr, slot) {
                                continue;
                            }
                            let _ = provider.storage(addr, slot);
                        }
                    }
                    PrewarmTarget::Storage(addr, slots) => {
                        for &slot in &slots {
                            #[cfg(feature = "bal-research")]
                            if research_prefetch(provider, research_observer.as_ref(), addr, slot) {
                                continue;
                            }
                            let _ = provider.storage(addr, slot);
                        }
                    }
                }
            }
            PrewarmMsg::EndBlock(end_tx) => {
                provider = None;
                #[cfg(feature = "bal-research")]
                {
                    research_observer = None;
                }
                drop(end_tx);
            }
        }
    }
}

/// Checks cancellation between storage operations; active noninterruptible reads finish.
#[cfg(feature = "bal-research")]
fn research_prefetch(
    provider: &CachedStateProvider<EvmStateProviderBox>,
    observer: Option<&Arc<dyn super::bal::research::ResearchObserver>>,
    address: Address,
    slot: StorageKey,
) -> bool {
    let Some(observer) = observer else {
        return false;
    };
    if observer.cancelled() {
        observer.prefetch(address, slot.into(), false, true);
        return true;
    }
    observer.prefetch(address, slot.into(), false, false);
    let _ = provider.storage(address, slot);
    observer.prefetch(address, slot.into(), true, false);
    true
}

struct SendOnDrop {
    sender: Option<oneshot::Sender<()>>,
}

impl Drop for SendOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(all(test, feature = "bal-research"))]
mod research_tests {
    use super::*;
    use crate::tree::payload_processor::bal::research::{AccessKey, ResearchObserver};
    use alloy_primitives::{B256, U256};
    use reth_primitives_traits::{Account, Bytecode};
    use std::{sync::atomic::AtomicBool, time::Duration};

    #[derive(Debug, Default)]
    struct Observer {
        cancelled: AtomicBool,
        started: AtomicUsize,
        completed: AtomicUsize,
        skipped: AtomicUsize,
    }

    impl ResearchObserver for Observer {
        fn collect_accesses(&self) -> bool {
            false
        }
        fn complete(
            &self,
            _: u32,
            _: &[AccessKey],
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        fn cancelled(&self) -> bool {
            self.cancelled.load(Ordering::Acquire)
        }
        fn prefetch(&self, _: Address, _: U256, completed: bool, skipped: bool) {
            let counter = if skipped {
                &self.skipped
            } else if completed {
                &self.completed
            } else {
                &self.started
            };
            counter.fetch_add(1, Ordering::Relaxed);
        }
        fn rejected(&self, _: &'static str) {}
    }

    struct Provider {
        first: Arc<AtomicBool>,
        entered: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
        dropped: Arc<AtomicUsize>,
    }

    impl EvmStateProvider for Provider {
        fn basic_account(&self, _: &Address) -> ProviderResult<Option<Account>> {
            Ok(Some(Account::default()))
        }
        fn block_hash(&self, _: u64) -> ProviderResult<Option<B256>> {
            Ok(None)
        }
        fn bytecode_by_hash(&self, _: &B256) -> ProviderResult<Option<Bytecode>> {
            Ok(None)
        }
        fn storage(&self, _: Address, _: StorageKey) -> ProviderResult<Option<U256>> {
            if !self.first.swap(true, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            Ok(Some(U256::from(7)))
        }
    }
    impl Drop for Provider {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn active_read_finishes_queued_reads_skip_and_next_block_resets() {
        let pool = BalPrewarmPool::new(1);
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        let dropped = Arc::new(AtomicUsize::new(0));
        let first = Arc::new(AtomicBool::new(false));
        let dropped_clone = dropped.clone();
        let build: Arc<BuildProviderFn> = Arc::new(move || {
            Ok(Box::new(Provider {
                first: first.clone(),
                entered: entered_tx.clone(),
                release: release_rx.clone(),
                dropped: dropped_clone.clone(),
            }))
        });
        let observer = Arc::new(Observer::default());
        pool.research_begin_block(Some(observer.clone()));
        pool.begin_block(build.clone(), ExecutionCache::new(1024 * 1024), None);
        pool.warm_account(Address::ZERO, [B256::ZERO, B256::repeat_byte(1)]);
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        observer.cancelled.store(true, Ordering::Release);
        pool.warm_account(Address::ZERO, [B256::repeat_byte(2)]);
        release_tx.send(()).unwrap();
        pool.end_block();
        assert_eq!(observer.started.load(Ordering::Relaxed), 1);
        assert_eq!(observer.completed.load(Ordering::Relaxed), 1);
        assert_eq!(observer.skipped.load(Ordering::Relaxed), 2);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        let next = Arc::new(Observer::default());
        pool.research_begin_block(Some(next.clone()));
        pool.begin_block(build, ExecutionCache::new(1024 * 1024), None);
        pool.warm_account(Address::ZERO, [B256::repeat_byte(3)]);
        pool.end_block();
        assert_eq!(next.completed.load(Ordering::Relaxed), 1);
        assert_eq!(next.skipped.load(Ordering::Relaxed), 0);
        assert_eq!(observer.completed.load(Ordering::Relaxed), 1);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }
}
