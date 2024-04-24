use melnode::{args::MainArgs, node::Node, staker::Staker, storage::Storage, telemetry};

use anyhow::Context;

use clap::Parser;
use melnet2::{wire::http::HttpBackhaul, Swarm};
use melprot::{Client, CoinChange, NodeRpcClient};
use melstructs::{BlockHeight, CoinID};

fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "melnode=debug,warn");
    }

    telemetry::init_tracing();

    let opts = MainArgs::parse();

    smolscale::block_on(main_async(opts))
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the main function for a node.
#[tracing::instrument(skip(opt))]
pub async fn main_async(opt: MainArgs) -> anyhow::Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    tracing::info!(version = debug(VERSION), "initializing melnode...",);

    let genesis = opt.genesis_config().await?;
    let netid = genesis.network;
    let storage: Storage = opt.storage().await?;
    let bootstrap = opt.bootstrap().await?;

    tracing::info!(
        bootstrap_addr = debug(bootstrap.clone()),
        "bootstrapping..."
    );

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
            .await
            .unwrap();
        let client = Client::new(netid, rpc_client);

        client.dangerously_trust_latest().await.unwrap();
        let snapshot = client.latest_snapshot().await.unwrap();
        smolscale::spawn::<anyhow::Result<()>>(async move {
            loop {
                tracing::info!("*** SELF TEST STARTED! ***");
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
                    tracing::debug!(
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
                                .await
                                .unwrap()
                                .unwrap();

                            tracing::debug!("testing transaction recipient {recipient}");

                            assert!(coin_changes
                                .contains(&CoinChange::Add(CoinID::new(tx_0.hash_nosigs(), 0))));
                        }
                        if let Some(proposer_action) = blk.proposer_action {
                            let reward_dest = proposer_action.reward_dest;
                            let coin_changes = snapshot
                                .get_raw()
                                .get_coin_changes(bh, reward_dest)
                                .await
                                .unwrap()
                                .unwrap();

                            tracing::debug!("testing proposer {reward_dest}");
                            assert!(coin_changes
                                .contains(&CoinChange::Add(CoinID::proposer_reward(bh))));
                        }
                    }
                }
            }
        })
        .detach();
    }

    Ok(())
}
