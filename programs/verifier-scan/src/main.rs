use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{UiConfirmedBlock, UiInstruction, UiMessage, UiParsedMessage, EncodedConfirmedTransactionWithStatusMeta};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// RPC URL (e.g. https://api.mainnet-beta.solana.com)
    #[arg(long)]
    rpc_url: String,
    /// Verifier program id (Gt9S41Pt...)
    #[arg(long)]
    verifier: String,
    /// Number of recent slots to scan backwards
    #[arg(long, default_value_t = 2000)]
    slots: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let verifier = args.verifier.parse::<Pubkey>()?;
    let client = RpcClient::new_with_commitment(args.rpc_url.clone(), CommitmentConfig::confirmed());

    let current_slot = client.get_slot().await?;
    let start = current_slot.saturating_sub(args.slots);

    let mut total = 0usize;
    let mut with_return_data = 0usize;
    let mut without_return_data = 0usize;

    for slot in (start..=current_slot).rev() {
        let Ok(block) = client.get_block_with_cfg(slot, solana_client::rpc_config::RpcBlockConfig {
            encoding: Some(solana_transaction_status::UiTransactionEncoding::Json),
            transaction_details: Some(solana_client::rpc_config::TransactionDetails::Full),
            rewards: Some(false),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
            rewards_config: None,
        }).await else { continue };

        if let Some(transactions) = block.transactions {
            for EncodedConfirmedTransactionWithStatusMeta { transaction: tx, meta, .. } in transactions {
                // Skip failed
                let Some(meta) = meta else { continue };
                if meta.status.is_err() { continue; }

                // Find if this tx invoked the verifier program
                let invoked = match &tx.message { 
                    solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                        match &ui_tx.message { 
                            UiMessage::Parsed(UiParsedMessage { instructions, .. }) => has_program(&verifier, instructions),
                            UiMessage::Raw(raw) => raw.instructions.iter().any(|ix| ix.program_id == verifier.to_string()),
                        }
                    }
                    _ => false,
                };
                if !invoked { continue; }

                total += 1;
                if let Some(rd) = meta.return_data { 
                    // Verify producer is the verifier
                    if rd.program_id == verifier.to_string() { with_return_data += 1; } else { without_return_data += 1; }
                } else { without_return_data += 1; }
            }
        }
    }

    println!("scanned_total_invocations={}", total);
    println!("success_with_verifier_return_data={}", with_return_data);
    println!("success_without_verifier_return_data_or_other_producer={}", without_return_data);
    Ok(())
}

fn has_program(verifier: &Pubkey, instructions: &[UiInstruction]) -> bool {
    instructions.iter().any(|ix| match ix {
        UiInstruction::Parsed(p) => p.program_id == verifier.to_string(),
        UiInstruction::Compiled(c) => c.program_id_index == 0, // cannot easily check here
        UiInstruction::Raw(r) => r.program_id == verifier.to_string(),
    })
}

