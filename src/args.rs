use crate::storage::Storage;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Context;
use clap::Parser;
use serde::{Deserialize, Serialize};

use melstf::GenesisConfig;
use melstructs::Address;
use tap::Tap;
use tmelcrypt::Ed25519SK;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
/// Command-line arguments.
pub struct MainArgs {
    /// Listen address
    #[arg(long, default_value = "0.0.0.0:41814")]
    listen: SocketAddr,

    /// Optional listen address for nodes using the legacy melnet protocol.
    #[arg(long)]
    legacy_listen: Option<SocketAddr>,

    /// Advertise address. Put your public IP address here.
    #[arg(long)]
    advertise: Option<String>,

    /// Override bootstrap addresses. May be given as a DNS name.
    #[arg(long, default_value = "auto")]
    bootstrap: Vec<String>,

    /// Database path
    #[arg(long)]
    database: Option<PathBuf>,

    /// Path to a YAML staker configuration
    #[arg(long)]
    staker_cfg: Option<PathBuf>,

    /// If given, uses this JSON file to configure the network genesis rather than following the known testnet/mainnet genesis.
    #[arg(long)]
    override_genesis: Option<PathBuf>,

    /// If set to true, default to the testnet. Otherwise, mainnet validation rules are used.
    #[arg(long)]
    testnet: bool,

    /// If set to true, runs a self-test by replaying the history from genesis, ensuring that everything is correct
    #[arg(long)]
    pub self_test: bool,

    /// Create an in-memory coin index. **RPC endpoints that rely on this will be disabled if this is not set!**
    #[arg(long)]
    pub index_coins: bool,

    /// Aggregate total MEL, SYM, and ERG balances for all addresses and dump to a file
    #[arg(long)]
    pub dump_balances: Option<PathBuf>,

    #[arg(long)]
    pub proxy: Option<SocketAddr>,
}

/// Staker configuration, YAML-deserializable.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StakerConfig {
    /// ed25519 secret key of the staker
    #[serde(with = "serde_with::rust::display_fromstr")]
    pub signing_secret: Ed25519SK,
    /// Listen address for the staker.
    #[serde(with = "serde_with::rust::display_fromstr")]
    pub listen: SocketAddr,
    /// Bootstrap address into the staker network.
    #[serde(with = "serde_with::rust::display_fromstr")]
    pub bootstrap: SocketAddr,
    /// Payout address
    #[serde(with = "serde_with::rust::display_fromstr")]
    pub payout_addr: Address,
    /// Target fee multiplier
    pub target_fee_multiplier: u128,
}

impl MainArgs {
    /// Gets the advertised IP.
    pub fn advertise_addr(&self) -> Option<String> {
        self.advertise.clone()
    }

    /// Derives the genesis configuration from the arguments
    pub async fn genesis_config(&self) -> anyhow::Result<GenesisConfig> {
        if let Some(path) = &self.override_genesis {
            let genesis_yaml: Vec<u8> = smol::fs::read(&path)
                .await
                .context("cannot read genesis config")?;
            Ok(serde_yaml::from_slice(&genesis_yaml)
                .context("error while parsing genesis config")?)
        } else if self.testnet {
            Ok(GenesisConfig::std_testnet())
        } else {
            Ok(GenesisConfig::std_mainnet())
        }
    }

    pub async fn storage(&self) -> anyhow::Result<Storage> {
        let genesis = self.genesis_config().await?;

        let database_default_path = dirs::home_dir().expect("no home dir?!").tap_mut(|p| {
            p.push(".melnode/");
        });
        let database_base_path = self.database.clone().unwrap_or(database_default_path);
        let _history_path = database_base_path
            .clone()
            .tap_mut(|path| path.push("history"));
        let _smt_path = database_base_path
            .clone()
            .tap_mut(|path| path.push("smt.db"));

        std::fs::create_dir_all(&database_base_path)?;
        let storage = Storage::open(database_base_path, genesis)
            .await
            .context("cannot make storage")?;

        log::debug!("node storage opened");

        Ok(storage)
    }

    /// Derives a list of bootstrap addresses
    pub async fn bootstrap(&self) -> anyhow::Result<Vec<String>> {
        if !self.bootstrap.is_empty() {
            let mut bootstrap = vec![];
            for name in self.bootstrap.iter() {
                let addrs = if name == "auto" {
                    // Convert Vec<SocketAddr> to Vec<String>
                    melbootstrap::bootstrap_routes(self.genesis_config().await?.network)
                        .into_iter()
                        .map(|addr| addr.to_string())
                        .collect::<Vec<String>>()
                } else {
                    // TODO: find a smarter way to do this
                    if name.contains(".haven") {
                        log::warn!("idk how to handle this haven bootstrap!");
                        vec![name.to_string()]
                    } else {
                        let socket_addrs = smol::net::resolve(&name)
                            .await
                            .context("cannot resolve DNS bootstrap")?;
                        socket_addrs
                            .iter()
                            .cloned()
                            .map(|socket_addr| socket_addr.to_string())
                            .collect::<Vec<String>>()
                    }
                };
                bootstrap.extend(addrs);
            }
            Ok(bootstrap)
        } else {
            // Convert Vec<SocketAddr> to Vec<String>
            Ok(
                melbootstrap::bootstrap_routes(self.genesis_config().await?.network)
                    .into_iter()
                    .map(|addr| addr.to_string())
                    .collect::<Vec<String>>(),
            )
        }
    }

    /// Listening address
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen
    }

    /// Legacy listening address
    pub fn legacy_listen_addr(&self) -> Option<SocketAddr> {
        self.legacy_listen
    }

    /// Staker secret key
    pub async fn staker_cfg(&self) -> anyhow::Result<Option<StakerConfig>> {
        if let Some(path) = self.staker_cfg.as_ref() {
            let s = std::fs::read_to_string(path)?;
            let lele: StakerConfig = serde_yaml::from_str(&s)?;
            Ok(Some(lele))
        } else {
            Ok(None)
        }
    }
}
