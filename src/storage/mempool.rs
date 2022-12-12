use crate::storage::MeshaCas;

use std::collections::HashSet;

use melvm::covenant_weight_from_bytes;
use themelio_stf::{SealedState, StateError, UnsealedState};
use themelio_structs::{Transaction, TxHash};

const WEIGHT_LIMIT: u128 = 10_000_000;

/// Mempool encapsulates a "mempool" --- a provisional state that is used to form new blocks by stakers, or provisionally validate transactions by auditors.
pub struct Mempool {
    provisional_state: UnsealedState<MeshaCas>,
    last_rebase: UnsealedState<MeshaCas>,
    txx_in_state: HashSet<TxHash>,
    next_weight: u128, // seen: LruCache<TxHash, ()>,
}

impl Mempool {
    /// Create sa new mempool based on a provisional state.
    pub fn new(state: UnsealedState<MeshaCas>) -> Self {
        Self {
            provisional_state: state.clone(),
            last_rebase: state,
            txx_in_state: Default::default(),
            next_weight: 0,
            // seen: LruCache::new(10000),
        }
    }
    /// Creates a State based on the present state of the mempool.
    pub fn to_state(&self) -> UnsealedState<MeshaCas> {
        self.provisional_state.clone()
    }

    /// Tries to add a transaction to the mempool.
    pub fn apply_transaction(&mut self, tx: &Transaction) -> anyhow::Result<()> {
        if self.next_weight < WEIGHT_LIMIT {
            if !self.txx_in_state.insert(tx.hash_nosigs()) {
                return Err(StateError::DuplicateTx.into());
            }
            self.provisional_state.apply_tx(tx)?;
            self.next_weight += tx.weight(covenant_weight_from_bytes);
            // self.seen.put(tx.hash_nosigs(), ());
            Ok(())
        } else {
            anyhow::bail!("mempool is full, try again later")
        }
    }

    /// Forcibly replaces the internal state of the mempool with the given state.
    pub fn rebase(&mut self, state: SealedState<MeshaCas>) {
        let sealed_provisional_state = self.provisional_state.clone().seal(None);

        if state.header().height > sealed_provisional_state.header().height {
            log::trace!(
                "rebasing mempool {} => {}",
                sealed_provisional_state.header().height,
                state.header().height
            );
            if !sealed_provisional_state.is_empty() {
                let transactions = sealed_provisional_state.to_block().transactions;
                log::warn!("*** THROWING AWAY {} MEMPOOL TXX ***", transactions.len());
            }
            assert!(state.is_empty());
            let next_state = state.next_unsealed();
            self.provisional_state = next_state.clone();
            self.last_rebase = next_state;
            self.txx_in_state.clear();
            self.next_weight = 0;
        }
    }

    /// Lookups a recent transaction.
    pub fn lookup_recent_tx(&self, _hash: TxHash) -> Option<Transaction> {
        None
        // self.seen
        //     .peek(&hash)
        //     .cloned()
        //     .or_else(|| self.provisional_state.transactions.get(&hash).cloned())
    }
}
