use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::PathBuf,
};

use bytes::Bytes;
use melstructs::{
    Address, CoinData, CoinValue, Denom, Transaction, TxKind,
};

use crate::dump_balances::DUMP_PATH;

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

pub fn faucet_txs() -> anyhow::Result<Vec<Transaction>> {
    let path = PathBuf::from(DUMP_PATH);
    let balances = read_from_file(&path)?;
    Ok(to_faucet_transactions(balances).into_iter().collect())
}
