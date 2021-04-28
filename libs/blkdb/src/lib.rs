pub mod backends;
pub mod traits;
pub mod tree;
pub use tree::*;

#[cfg(test)]
mod tests {
    use blkstructs::{GenesisConfig, State};

    use crate::{backends::InMemoryBackend, BlockTree};

    #[test]
    fn simple_test() {
        let backend = InMemoryBackend::default();
        let forest = autosmt::Forest::load(autosmt::MemDB::default());
        let mut tree = BlockTree::new(backend, forest.clone());
        assert!(tree.get_tips().is_empty());
        let genesis = State::genesis(&forest, GenesisConfig::std_testnet()).seal(None);
        tree.set_genesis(genesis.clone(), None);
        assert!(tree.get_tips()[0].header() == genesis.header());
        eprintln!("{}", tree.debug_graphviz());

        let mut next_state = genesis;
        for _ in 0..10 {
            next_state = next_state.next_state().seal(None);
            tree.apply_block(&next_state.to_block()).unwrap();
        }
        eprintln!("{}", tree.debug_graphviz());
    }
}
