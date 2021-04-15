use crate::wallet::manager::WalletManager;
use crate::shell::runner::ShellRunner;
use crate::shell::io::ShellOutput;
use blkstructs::{NetID, CoinID};
use nodeprot::ValClient;
use smol::Timer;
use std::time::Duration;

/// Responsible for executing a single client CLI command non-interactively.
pub struct CommandExecutor {
    pub host: smol::net::SocketAddr,
    pub database: std::path::PathBuf,
    pub version: String,
    pub network: NetID,
}

impl CommandExecutor {
    pub fn new(host: smol::net::SocketAddr, database: std::path::PathBuf, version: &str, network: NetID) -> Self {
        let version = version.to_string();
        Self {
            host,
            database,
            version,
            network
        }
    }

    /// Creates a new wallet, stores it into db and outputs the name & secret.
    pub async fn create_wallet(&self, wallet_name: &str) -> anyhow::Result<()> {
        let manager = WalletManager::new(&self.host.clone(), &self.database.clone());
        let wallet = manager.create_wallet(wallet_name).await?;
        ShellOutput::show_new_wallet(wallet).await?;
        Ok(())
    }

    /// Opens a wallet by name and secret and creates a faucet tx to fund the wallet.
    /// It then sends the transaction and waits for a confirmation of the coins on the ledger.
    pub async fn faucet(&self, wallet_name: &str, secret: &str, amount: &str, unit: &str) -> anyhow::Result<()> {
        // Load wallet from wallet manager using name and secret
        let manager = WalletManager::new(&self.host.clone(), &self.database.clone());
        let wallet = manager.load_wallet(wallet_name, secret).await?;

        // Create faucet tx.
        let fee = 2050000000;
        let tx = wallet.create_faucet_tx(amount, unit, fee).await?;

        // Send the faucet tx.
        wallet.send_tx(&tx).await?;

        // Wait for confirmation of the coin.
        let coin = CoinID {
            txhash: tx.hash_nosigs(),
            index: 0,
        };
        wallet.confirm_faucet_coins(&coin).await?;

        // print results

        Ok(())
    }

    /// Opens a wallet by name and secret and sends coins from the wallet to a destination.
    pub async fn send_coins(&self, wallet_name: &str, secret: &str, address: &str, amount: &str, unit: &str) -> anyhow::Result<()> {
        // // Load wallet from wallet manager using name and secret
        // let manager = WalletManager::new(&self.host.clone(), &self.database.clone());
        // let wallet = manager.load_wallet(wallet_name, secret).await?;
        //
        // // Create send coins tx.
        // let fee = 2050000000;
        // let tx = wallet.create_send_coins_tx(amount, unit, fee).await?;
        //
        // // Send the tx
        // wallet.send_tx(&tx).await?;
        //
        // // Wait for confirmation of the coin
        // let coin = CoinID {
        //     txhash: tx.hash_nosigs(),
        //     index: 0,
        // };
        // wallet.confirm_coins(&coin).await?;

        // print results

        Ok(())
    }

    /// Adds coins by coin id to wallet.
    pub async fn add_coins(&self, wallet_name: &str, secret: &str, coin_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shows the total known wallet balance.
    pub async fn show_balance(&self, wallet_name: &str, secret: &str, ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shows all the wallets by name that are stored in the db.
    pub async fn show_wallets(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Launch shell mode until user exits.
    pub async fn shell(&self) -> anyhow::Result<()> {
        let runner = ShellRunner::new(&self.host, &self.database, &self.version);
        runner.run().await?;
        Ok(())
    }
}