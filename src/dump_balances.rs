use std::{collections::HashMap, fs::File, path::PathBuf};

use melstf::CoinMapping;
use melstructs::{
    Address, CoinDataHeight, CoinValue, Denom,
};
use novasmt::ContentAddrStore;
use serde_json::to_writer_pretty;

pub fn dump_balances<C: ContentAddrStore>(
    coins: &CoinMapping<C>,
    path: &PathBuf,
) -> anyhow::Result<()> {
    let balances = tally_balances(coins);
    write_to_file(path, &balances)?;
    Ok(())
}

fn tally_balances<C: ContentAddrStore>(
    coins: &CoinMapping<C>,
) -> HashMap<Address, HashMap<Denom, CoinValue>> {
    let mut balances: HashMap<Address, HashMap<Denom, CoinValue>> = HashMap::new();

    for (_, v) in coins.iter() {
        let cdh: CoinDataHeight = match stdcode::deserialize(&v) {
            Ok(cdh) => cdh,
            Err(_) => continue,
        };

        match cdh.coin_data.denom {
            Denom::Custom(_) => (),
            _ => {
                let balance = balances.entry(cdh.coin_data.covhash).or_default();
                let denom_balance = balance.entry(cdh.coin_data.denom).or_default();
                *denom_balance += cdh.coin_data.value;
            }
        }
    }

    balances
}

fn write_to_file(
    path: &PathBuf,
    hashmap: &HashMap<Address, HashMap<Denom, CoinValue>>,
) -> anyhow::Result<()> {
    let file = File::create(path)?;
    to_writer_pretty(file, hashmap)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate rand;

    use std::path::PathBuf;

    use bytes::Bytes;
    use melstf::CoinMapping;
    use melstructs::{
        Address, BlockHeight, CoinData, CoinDataHeight, CoinID, CoinValue, Denom, TxHash,
    };
    use novasmt::{ContentAddrStore, Database, InMemoryCas};
    use rand::Rng;
    use tmelcrypt::HashVal;

    use super::dump_balances;

    #[test]
    fn dump_balances_test() -> anyhow::Result<()> {
        let path = PathBuf::from("dump.json");
        let db = Database::new(InMemoryCas::default());
        let tree = db.get_tree(HashVal::default().0).unwrap();
        let mut coins = CoinMapping::new(tree);

        for _ in 0..10 {
            add_random_coin(&mut coins);
        }

        dump_balances(&coins, &path)?;

        Ok(())
    }

    fn add_random_coin<C: ContentAddrStore>(coins: &mut CoinMapping<C>) {
        let (id, cdh) = random_coin();
        coins.insert_coin(id, cdh, true);
    }

    fn random_coin() -> (CoinID, CoinDataHeight) {
        let mut rng = rand::thread_rng();
        let coin_id = CoinID::new(TxHash(HashVal::random()), rng.gen());
        let coin_data = CoinData {
            covhash: Address(HashVal::random()),
            value: CoinValue(rng.gen()),
            denom: random_denom(),
            additional_data: Bytes::new(),
        };
        let coin_data_height = CoinDataHeight {
            coin_data,
            height: BlockHeight(rng.gen()),
        };
        (coin_id, coin_data_height)
    }

    fn random_denom() -> Denom {
        let options = [
            Denom::Mel,
            Denom::Sym,
            Denom::Erg,
            Denom::Custom(TxHash(HashVal::random())),
        ];
        options[rand::thread_rng().gen_range(0..options.len())]
    }
}
