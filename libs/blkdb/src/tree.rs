use crate::traits::DbBackend;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::TryInto;
use std::fmt::Write;

use dashmap::DashMap;
use novasmt::ContentAddrStore;
use serde::{Deserialize, Serialize};
use themelio_stf::SealedState;
use themelio_structs::{Block, BlockHeight, Header, ProposerAction};
use thiserror::Error;
use tmelcrypt::HashVal;

/// A block tree, stored on a particular backend.
pub struct BlockTree<B: DbBackend, C: ContentAddrStore> {
    inner: Inner<B, C>,
    forest: novasmt::Database<C>,
}

impl<B: DbBackend, C: ContentAddrStore> BlockTree<B, C> {
    /// Create a new BlockTree.
    pub fn new(backend: B, forest: novasmt::Database<C>) -> Self {
        let inner = Inner {
            backend,

            cache: Default::default(),
        };
        let mut toret = Self { inner, forest };
        toret.initial_tip_cleanup();
        toret
    }

    /// Initial cleanup: delete tips that are ancestors of other tips.
    fn initial_tip_cleanup(&mut self) {
        let mut tips = HashMap::new();

        self.get_tips().iter().for_each(|tip| {
            let hash = tip.header().hash();
            tips.insert(hash, tip.to_state().inner_ref().history.clone());
        });

        let mut to_delete = Vec::new();

        self.get_tips().iter().for_each(|tip| {
            if tips
                .iter()
                .any(|(_, history)| history.get(&tip.header().height).0 == Some(tip.header()))
            {
                to_delete.push(tip.header().hash())
            }
        });

        to_delete.into_iter().for_each(|to_delete_single| {
            self.inner.tip_remove(to_delete_single);
        });
    }

    /// Attempts to apply a block.
    pub fn apply_block(
        &mut self,
        block: &Block,
        init_metadata: &[u8],
    ) -> Result<(), ApplyBlockErr> {
        let previous = self
            .inner
            .get_block(
                block.header.previous,
                Some(block.header.height.0.saturating_sub(1).into()),
            )
            .ok_or(ApplyBlockErr::ParentNotFound(block.header.previous))?;
        let previous = previous.to_state(&self.forest, &self.inner.cache);
        let next_state = previous
            .apply_block(block)
            .map_err(ApplyBlockErr::CannotValidate)?;

        // apply block should already have checked this
        assert_eq!(next_state.header(), block.header);
        self.inner.insert_block(next_state, init_metadata);
        Ok(())
    }

    /// Get all the cursors at a given height.
    pub fn get_at_height(&self, height: BlockHeight) -> Vec<Cursor<'_, B, C>> {
        self.inner
            .all_at_height(height)
            .into_iter()
            .map(|v| {
                self.get_cursor(v)
                    .expect("did not get expected block at height")
            })
            .collect()
    }

    /// Obtains a *cursor* pointing to a particular block in the tree. The cursor has a lifetime bound that prevents the blocktree from mutating when cursors exist.
    pub fn get_cursor(&self, hash: HashVal) -> Option<Cursor<'_, B, C>> {
        let internal = self.inner.get_block(hash, None)?;
        Some(Cursor {
            tree: self,
            internal,
        })
    }

    /// Obtains a *mutable cursor* pointing to a particular block in the tree. The cursor has a lifetime bound that prevents the blocktree from mutating when cursors exist.
    pub fn get_cursor_mut(&mut self, hash: HashVal) -> Option<CursorMut<'_, B, C>> {
        let internal = self.inner.get_block(hash, None)?;
        Some(CursorMut {
            tree: self,
            internal,
        })
    }

    /// Get a vector of "tips" in the blockchain
    pub fn get_tips(&self) -> Vec<Cursor<'_, B, C>> {
        let tip_keys = self.inner.all_tips();
        tip_keys
            .into_iter()
            .filter_map(|v| self.get_cursor(v))
            .collect()
    }

    /// Sets the genesis block of the tree. This also prunes all elements that do not belong to the given genesis block.
    pub fn set_genesis(&mut self, state: SealedState<C>, init_metadata: &[u8]) {
        let state_hash = state.header().hash();
        if self.get_cursor(state.header().hash()).is_none() {
            self.inner.insert_block(state, init_metadata);
        }

        let old_genesis = self.get_tips().into_iter().next().map(|v| {
            let mut v = v;
            while let Some(parent) = v.parent() {
                v = parent;
            }
            v
        });

        // remove all non-descendants
        let mut descendants = HashSet::new();
        {
            let mut stack: Vec<Cursor<_, _>> = vec![self
                .get_cursor(state_hash)
                .expect("just-set genesis is gone?!")];
            while let Some(top) = stack.pop() {
                descendants.insert(top.header().hash());

                top.children().into_iter().for_each(|child| {
                    stack.push(child);
                });
            }
        }
        let mut to_delete = HashSet::new();
        if let Some(old_genesis) = old_genesis {
            if old_genesis.header().hash() != state_hash {
                // use this cursor to traverse
                let mut stack: Vec<Cursor<_, _>> = vec![old_genesis];
                while let Some(top) = stack.pop() {
                    if !descendants.contains(&top.header().hash()) {
                        // this is a damned one!
                        to_delete.insert(top.header());

                        top.children().into_iter().for_each(|child| {
                            stack.push(child);
                        });
                    }
                }
            }
        }
        // okay now we go through the whole to_delete sequence
        let mut to_delete = to_delete.into_iter().collect::<Vec<_>>();
        to_delete.sort_unstable_by_key(|v| v.height);

        to_delete.into_iter().for_each(|to_delete_single| {
            self.inner
                .remove_orphan(to_delete_single.hash(), Some(to_delete_single.height));
        });
    }

    /// Deletes all the tips.
    pub fn delete_tips(&mut self) {
        let tips = self
            .get_tips()
            .iter()
            .map(|v| v.header().hash())
            .collect::<Vec<_>>();

        tips.into_iter().for_each(|tip| {
            self.inner.remove_childless(tip, None);
        });
    }

    /// Creates a GraphViz string that represents all the blocks in the tree.
    pub fn debug_graphviz(&self, visitor: impl Fn(&Cursor<'_, B, C>) -> String) -> String {
        let mut stack = self.get_tips();
        let tips = self
            .get_tips()
            .iter()
            .map(|v| v.header())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut output = String::new();
        writeln!(&mut output, "digraph G {{").unwrap();
        while let Some(top) = stack.pop() {
            if seen.insert(top.header()) {
                if tips.contains(&top.header()) {
                    writeln!(
                        &mut output,
                        "\"{}\" [label={}, shape=rectangle, style=filled, fillcolor=red];",
                        top.header().hash(),
                        top.header().height
                    )
                    .unwrap();
                } else {
                    writeln!(
                        &mut output,
                        "\"{}\" [label={}, shape=rectangle, style=filled, fillcolor=\"{}\"];",
                        top.header().hash(),
                        top.header().height,
                        visitor(&top),
                    )
                    .unwrap();
                }
                if let Some(parent) = top.parent() {
                    writeln!(
                        &mut output,
                        "\"{}\" -> \"{}\";",
                        top.header().hash(),
                        top.header().previous
                    )
                    .unwrap();
                    stack.push(parent);
                }
            }
        }
        writeln!(&mut output, "}}").unwrap();
        output
    }
}

/// A cursor, pointing to something inside the block tree.
pub struct Cursor<'a, B: DbBackend, C: ContentAddrStore> {
    tree: &'a BlockTree<B, C>,
    internal: InternalValue,
}

impl<'a, B: DbBackend, C: ContentAddrStore> Clone for Cursor<'a, B, C> {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree,
            internal: self.internal.clone(),
        }
    }
}

impl<'a, B: DbBackend, C: ContentAddrStore> Cursor<'a, B, C> {
    /// Converts to a SealedState.
    pub fn to_state(&self) -> SealedState<C> {
        self.internal
            .to_state(&self.tree.forest, &self.tree.inner.cache)
    }

    /// Extracts the header.
    pub fn header(&self) -> Header {
        self.internal.header
    }

    /// Extracts the metadata.
    pub fn metadata(&self) -> &[u8] {
        &self.internal.metadata
    }

    /// Returns a vector of child cursors.
    pub fn children(&self) -> Vec<Self> {
        self.internal
            .next
            .iter()
            .map(|hash| self.tree.get_cursor(*hash).expect("dangling child pointer"))
            .collect()
    }

    /// Returns the parent of this block.
    pub fn parent(&self) -> Option<Self> {
        self.tree.get_cursor(self.internal.header.previous)
    }
}

/// A mutable cursor, pointing to something inside the block tree.
pub struct CursorMut<'a, B: DbBackend, C: ContentAddrStore> {
    tree: &'a mut BlockTree<B, C>,
    internal: InternalValue,
}

impl<'a, B: DbBackend, C: ContentAddrStore> CursorMut<'a, B, C> {
    /// Converts to a SealedState.
    pub fn to_state(&self) -> SealedState<C> {
        self.internal
            .to_state(&self.tree.forest, &self.tree.inner.cache)
    }

    /// Extracts the header.
    pub fn header(&self) -> Header {
        self.internal.header
    }

    /// Extracts the metadata.
    pub fn metadata(&self) -> &[u8] {
        &self.internal.metadata
    }

    /// Sets the metadata.
    pub fn set_metadata(&mut self, metadata: &[u8]) {
        self.internal.metadata = metadata.to_vec();
        self.tree.inner.internal_insert(
            self.header().hash(),
            self.header().height,
            self.internal.clone(),
        );
    }

    /// Consumes and returns the parent of this block.
    pub fn parent(self) -> Option<Self> {
        self.tree.get_cursor_mut(self.internal.header.previous)
    }

    /// "Downgrades" the cursor to an immutable cursor.
    pub fn downgrade(self) -> Cursor<'a, B, C> {
        Cursor {
            tree: self.tree,
            internal: self.internal,
        }
    }
}

/// An error returned when applying a block
#[derive(Error, Debug)]
pub enum ApplyBlockErr {
    #[error("parent `{0}` not found")]
    ParentNotFound(HashVal),
    #[error("validation error: `{0}`")]
    CannotValidate(themelio_stf::StateError),
    #[error("header mismatch")]
    HeaderMismatch,
}

/// Lower-level helper struct that provides fail-safe basic operations.
struct Inner<B: DbBackend, C: ContentAddrStore> {
    backend: B,
    // cached SealedStates. this is also required so that inserted blocks in non-canonical mode are persistent.
    cache: DashMap<HashVal, SealedState<C>>,
}

impl<B: DbBackend, C: ContentAddrStore> Inner<B, C> {
    /// Gets a block from the database.
    fn get_block(&self, blkhash: HashVal, height: Option<BlockHeight>) -> Option<InternalValue> {
        let height = match height {
            Some(height) => height,
            None => self.index_get(blkhash)?,
        };
        self.internal_get(blkhash, height)
    }

    /// Removes a block with no parent.
    fn remove_orphan(&mut self, blkhash: HashVal, height: Option<BlockHeight>) {
        let current = self
            .get_block(blkhash, height)
            .expect("trying to remove nonexistent orphan");
        debug_assert!(self.get_block(current.header.previous, None).is_none());
        // remove from tips, index, then main
        self.tip_remove(blkhash);
        self.index_remove(blkhash);
        self.internal_remove(blkhash, current.header.height);
        // finally delete from cache
        self.cache.remove(&blkhash);
    }

    /// Removes a block with no children.
    fn remove_childless(&mut self, blkhash: HashVal, height: Option<BlockHeight>) {
        let current = self
            .get_block(blkhash, height)
            .expect("trying to remove nonexistent childless");
        // remove from tips, index, then main
        self.tip_remove(blkhash);
        self.tip_insert(current.header.previous, current.header.height - 1.into());
        self.index_remove(blkhash);

        let mut parent = self
            .get_block(
                current.header.previous,
                Some(current.header.height - 1.into()),
            )
            .unwrap();
        parent.next.remove(&blkhash);
        self.internal_insert(current.header.previous, parent.header.height, parent);

        self.internal_remove(blkhash, current.header.height);
        // finally delete from cache
        self.cache.remove(&blkhash);
    }

    /// Inserts a block into the database
    fn insert_block(
        &mut self,
        state: SealedState<C>,
        init_metadata: &[u8],
    ) -> Option<InternalValue> {
        // if let Some(val) = self.get_block(state.header().hash(), Some(state.inner_ref().height)) {
        //     return Some(val);
        // }
        let action = state.proposer_action().cloned();
        // we carefully insert the block to avoid inconsistency:
        // - first we insert the block itself
        // - then we point the parent to it
        // - then we insert into the blkhash index
        // - then we update the tips list
        let header = state.header();
        let blkhash = header.hash();
        // insert the block
        self.internal_insert(
            blkhash,
            header.height,
            InternalValue::from_state(&state, action, init_metadata.to_vec()),
        );
        // insert into parent
        if let Some(mut parent) = self.get_block(
            header.previous,
            Some(header.height.0.saturating_sub(1).into()),
        ) {
            parent.next.insert(blkhash);
            self.internal_insert(header.previous, parent.header.height, parent);
        }
        // insert into blkhash index
        self.index_insert(blkhash, header.height);
        // update tips list
        self.tip_insert(blkhash, header.height);
        self.tip_remove(header.previous);
        // cache
        self.cache.insert(blkhash, state);
        None
    }

    fn internal_insert(&mut self, blkhash: HashVal, height: BlockHeight, value: InternalValue) {
        self.backend.insert(
            &main_key(blkhash, height),
            &stdcode::serialize(&value).unwrap(),
        );
    }

    fn index_insert(&mut self, blkhash: HashVal, height: BlockHeight) {
        self.backend
            .insert(&index_key(blkhash), &stdcode::serialize(&height).unwrap());
    }

    fn tip_insert(&mut self, blkhash: HashVal, height: BlockHeight) {
        self.backend
            .insert(&tip_key(blkhash), &stdcode::serialize(&height).unwrap());
    }

    fn internal_get(&self, blkhash: HashVal, height: BlockHeight) -> Option<InternalValue> {
        Some(
            stdcode::deserialize(&self.backend.get(&main_key(blkhash, height))?)
                .expect("cannot deserialize internal value"),
        )
    }

    fn internal_remove(&mut self, blkhash: HashVal, height: BlockHeight) {
        self.backend.remove(&main_key(blkhash, height));
    }

    fn index_get(&self, blkhash: HashVal) -> Option<BlockHeight> {
        Some(
            stdcode::deserialize(&self.backend.get(&index_key(blkhash))?)
                .expect("cannot deserialize index value"),
        )
    }

    fn index_remove(&mut self, blkhash: HashVal) {
        self.backend.remove(&index_key(blkhash));
    }

    fn tip_remove(&mut self, blkhash: HashVal) {
        self.backend.remove(&tip_key(blkhash));
    }

    fn all_tips(&self) -> Vec<HashVal> {
        let raw = self
            .backend
            .key_range(&tip_key(HashVal([0x00; 32])), &tip_key(HashVal([0xff; 32])));
        raw.into_iter()
            .map(|v| HashVal((&v[8..]).try_into().expect("corrupt tip key")))
            .collect()
    }

    fn all_at_height(&self, height: BlockHeight) -> Vec<HashVal> {
        let raw = self.backend.key_range(
            &main_key(HashVal([0x00; 32]), height),
            &main_key(HashVal([0xff; 32]), height),
        );
        raw.into_iter()
            .map(|v| HashVal((&v[8..]).try_into().expect("corrupt tip key")))
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InternalValue {
    header: Header,
    partial_state: Vec<u8>,
    action: Option<ProposerAction>,
    next: BTreeSet<HashVal>,
    metadata: Vec<u8>,
}

impl InternalValue {
    fn from_state<C: ContentAddrStore>(
        state: &SealedState<C>,
        action: Option<ProposerAction>,
        metadata: Vec<u8>,
    ) -> Self {
        Self {
            header: state.header(),
            partial_state: state.partial_encoding(),
            action,
            next: Default::default(),
            metadata,
        }
    }

    fn to_state<C: ContentAddrStore>(
        &self,
        forest: &novasmt::Database<C>,
        cache: &DashMap<HashVal, SealedState<C>>,
    ) -> SealedState<C> {
        cache
            .get(&self.header.hash())
            .map(|f| f.clone())
            .unwrap_or_else(|| {
                SealedState::from_partial_encoding_infallible(&self.partial_state, forest)
            })
    }
}

fn main_key(blkhash: HashVal, height: BlockHeight) -> [u8; 40] {
    let mut toret = [0u8; 40];
    toret[..8].copy_from_slice(&height.0.to_be_bytes());
    toret[8..].copy_from_slice(&blkhash);
    toret
}

fn tip_key(blkhash: HashVal) -> [u8; 40] {
    main_key(blkhash, (u64::MAX - 1).into())
}

fn index_key(blkhash: HashVal) -> [u8; 40] {
    main_key(blkhash, (u64::MAX).into())
}
