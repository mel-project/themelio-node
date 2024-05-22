use melnode::{args::MainArgs, dump_balances, node::Node, staker::Staker};

use anyhow::Context;

use clap::Parser;
use melnet2::{wire::http::HttpBackhaul, Swarm};
use melprot::{Client, CoinChange, NodeRpcClient};
use melstf::CoinMapping;
use melstructs::{BlockHeight, CoinID};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "melnode=debug,warn");
    }

    let mut builder = env_logger::Builder::from_env("RUST_LOG");

    builder.init();
    let opts = MainArgs::parse();

    smolscale::block_on(main_async(opts))
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the main function for a node.
pub async fn main_async(opt: MainArgs) -> anyhow::Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    log::info!("melnode v{} initializing...", VERSION);

    let genesis = opt.genesis_config().await?;
    let netid = genesis.network;
    let storage = opt.storage().await?;
    let bootstrap = opt.bootstrap().await?;

    log::info!("bootstrapping with {:?}", bootstrap);

    let swarm: Swarm<HttpBackhaul, NodeRpcClient> =
        Swarm::new(HttpBackhaul::new(), NodeRpcClient, "melnode");

    // we add the bootstrap routes as "sticky" routes that never expire
    for addr in bootstrap.iter() {
        swarm.add_route(addr.to_string().into(), true).await;
    }

    let _node_prot = Node::start(
        netid,
        opt.listen_addr(),
        opt.advertise_addr(),
        storage.clone(),
        opt.index_coins,
        swarm.clone(),
    )
    .await?;

    let _staker_prot = opt
        .staker_cfg()
        .await?
        .map(|cfg| Staker::new(storage.clone(), cfg));

    if opt.self_test {
        let storage = storage.clone();

        let rpc_client = swarm
            .connect(opt.listen_addr().to_string().into())
            .await?;
        let client = Client::new(netid, rpc_client);

        client.dangerously_trust_latest().await?;
        let snapshot = client.latest_snapshot().await?;
        smolscale::spawn::<anyhow::Result<()>>(async move {
            loop {
                log::info!("*** SELF TEST STARTED! ***");
                let mut state = storage
                    .get_state(BlockHeight(9))
                    .await
                    .context("no block 1")?;
                let last_height = storage.highest_height().await.0;
                for bh in 10..=last_height {
                    let bh = BlockHeight(bh);
                    // let blk = storage.get_state(bh).await.context("no block")?.to_block();
                    let blk = storage.get_block(bh).await.context("no block")?;
                    state = state.apply_block(&blk).expect("block application failed");
                    smol::future::yield_now().await;
                    log::debug!(
                        "{}/{} replayed correctly ({:.2}%)",
                        bh,
                        last_height,
                        bh.0 as f64 / last_height as f64 * 100.0
                    );

                    // indexer test
                    if opt.index_coins {
                        if let Some(tx_0) = blk.transactions.iter().next() {
                            let recipient = tx_0.outputs[0].covhash;
                            let coin_changes = snapshot
                                .get_raw()
                                .get_coin_changes(bh, recipient)
                                .await?
                                .unwrap();

                            log::debug!("testing transaction recipient {recipient}");

                            assert!(coin_changes
                                .contains(&CoinChange::Add(CoinID::new(tx_0.hash_nosigs(), 0))));
                        }
                        if let Some(proposer_action) = blk.proposer_action {
                            let reward_dest = proposer_action.reward_dest;
                            let coin_changes = snapshot
                                .get_raw()
                                .get_coin_changes(bh, reward_dest)
                                .await?
                                .unwrap();

                            log::debug!("testing proposer {reward_dest}");
                            assert!(coin_changes
                                .contains(&CoinChange::Add(CoinID::proposer_reward(bh))));
                        }
                    }
                }
            }
        })
        .detach();
    }

    if let Some(path) = &opt.dump_balances {
        let storage = storage.clone();
        let rpc_client = swarm
            .connect(opt.listen_addr().to_string().into())
            .await?;
        let client = Client::new(netid, rpc_client);
        client.dangerously_trust_latest().await?;
        let snapshot = client.latest_snapshot().await?;
        let header = snapshot.current_header();
        let height = header.height;
        let coins_hash = header.coins_hash;
        let state = storage.get_state(height).await.context("error retrieving state")?;
        let raw_coins_smt = state.raw_coins_smt();
        let coins_smt = raw_coins_smt.database().get_tree(coins_hash.0).unwrap();
        let coins = CoinMapping::new(coins_smt);

        dump_balances::dump_balances(&coins, path)?;
    }

    // #[cfg(feature = "dhat-heap")]
    // for i in 0..300 {
    //     smol::Timer::after(Duration::from_secs(1)).await;
    //     dbg!(i);
    // }

    #[cfg(not(feature = "dhat-heap"))]
    let _: u64 = smol::future::pending().await;

    Ok(())
}
