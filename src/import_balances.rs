use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::PathBuf,
};

use bytes::Bytes;
use melstf::CoinMapping;
use melstructs::{
    Address, Block, BlockHeight, CoinData, CoinDataHeight, CoinID, CoinValue, Denom, Header,
    Transaction, TxKind,
};
use novasmt::{dense::DenseMerkleTree, Database, Hashed, InMemoryCas};
use stdcode::StdcodeSerializeExt;
use tap::Pipe;
use tmelcrypt::{HashVal, Hashable};

fn read_from_file(path: &PathBuf) -> anyhow::Result<HashMap<Address, HashMap<Denom, CoinValue>>> {
    let file = File::open(path)?;
    let balances = serde_json::from_reader(file)?;
    Ok(balances)
}

fn to_faucet_transactions(
    balances: HashMap<Address, HashMap<Denom, CoinValue>>,
) -> HashSet<Transaction> {
    let mut faucet_txs = HashSet::new();

    for (covhash, denom_to_value) in balances {
        let chunks: Vec<_> = denom_to_value.into_iter().collect();

        for chunk in chunks.chunks(255) {
            let mut tx = Transaction::new(TxKind::Faucet);
            for (denom, value) in chunk {
                tx.outputs.push(CoinData {
                    covhash,
                    value: *value,
                    denom: *denom,
                    additional_data: Bytes::new(),
                });
            }
            faucet_txs.insert(tx);
        }
    }

    faucet_txs
}

fn to_coins_hash(transactions: &HashSet<Transaction>) -> HashVal {
    let db = Database::new(InMemoryCas::default());
    let tree = db.get_tree(Hashed::default()).unwrap();
    let mut coins = CoinMapping::new(tree);

    for tx in transactions {
        for (idx, output) in tx.clone().outputs.into_iter().enumerate() {
            coins.insert_coin(
                CoinID {
                    txhash: tx.hash_nosigs(),
                    index: idx as u8,
                },
                CoinDataHeight {
                    coin_data: output,
                    height: BlockHeight(1),
                },
                true,
            );
        }
    }

    coins.root_hash()
}

fn to_transactions_hash(transactions: &HashSet<Transaction>) -> HashVal {
    let mut vv = Vec::new();
    for tx in transactions.iter() {
        let complex = tx.hash_nosigs().pipe(|nosigs_hash| {
            let mut v = nosigs_hash.0.to_vec();
            v.extend_from_slice(&tx.stdcode().hash().0);
            v
        });
        vv.push(complex);
    }
    vv.sort_unstable();
    let tree = DenseMerkleTree::new(&vv);

    HashVal(tree.root_hash())
}

fn to_block(transactions: HashSet<Transaction>) -> Block {
    let transactions_hash = to_transactions_hash(&transactions);
    let coins_hash = to_coins_hash(&transactions);
    let header = Header {
        network: melstructs::NetID::Custom07,
        previous: HashVal::default(),
        height: BlockHeight(1),
        history_hash: todo!(),
        coins_hash,
        transactions_hash,
        fee_pool: todo!(),
        fee_multiplier: todo!(),
        dosc_speed: todo!(),
        pools_hash: todo!(),
        stakes_hash: todo!(),
    };
    Block {
        header,
        transactions,
        proposer_action: todo!(),
    }
}

pub fn block_1(path: PathBuf) -> anyhow::Result<Block> {
    let init_balances = read_from_file(&path)?;
    let faucet_txs = to_faucet_transactions(init_balances);
    let block_1 = to_block(faucet_txs);
    Ok(block_1)
}
