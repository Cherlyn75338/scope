use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{Keypair, Signer},
    transaction::Transaction,
    system_instruction,
    instruction::Instruction,
    pubkey::Pubkey,
};

#[tokio::main]
async fn main() -> Result<()> {
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Placeholder: load or create keypair
    let payer = Keypair::new();

    // Placeholder: airdrop
    let _sig = client.request_airdrop(&payer.pubkey(), 2_000_000_000).await?;

    // Scaffold only: print pubkey and exit
    println!("payer: {}", payer.pubkey());
    Ok(())
}

