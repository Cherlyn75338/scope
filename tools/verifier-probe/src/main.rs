use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_transaction_status::{UiInstruction, EncodedConfirmedTransactionWithStatusMeta};

const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const VERIFIER_PID: &str = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c";

fn extract_last_program_return_line(tx: &EncodedConfirmedTransactionWithStatusMeta) -> Option<(String, usize)> {
    let meta = tx.transaction.meta.as_ref()?;
    let log_messages = meta.log_messages.as_ref().and_then(|v| Some(v.clone()))?;
    // Heuristic: look for "Program return: <pid>" pattern nearest the end to infer last writer
    for (idx, line) in log_messages.iter().enumerate().rev() {
        if line.starts_with("Program return:") {
            return Some((line.clone(), idx));
        }
    }
    None
}

fn includes_verifier(ixs: &[UiInstruction]) -> bool {
    let verifier = VERIFIER_PID.parse::<Pubkey>().unwrap();
    ixs.iter().any(|ix| match ix {
        UiInstruction::Compiled(c) => {
            // Without loaded message, we cannot map index -> pubkey here; leave as false
            let _ = c; false
        }
        UiInstruction::Parsed(p) => p.program_id().map(|s| s == verifier.to_string()).unwrap_or(false),
        _ => false,
    })
}

fn main() -> Result<()> {
    let _client = RpcClient::new_with_commitment(MAINNET_RPC.to_string(), CommitmentConfig::confirmed());
    let verifier = VERIFIER_PID.parse::<Pubkey>().unwrap();
    // Note: a full crawl would be large; here we instruct users how to run targeted queries:
    println!("Use: solana logs or an indexer to collect recent txs involving {} and check last Program return lines.", verifier);
    println!("This tool is a scaffold. Integrate with a logs/indexing backend (e.g., BigTable or Helius) to pull txs and confirm if any successful verify lacked a final return-data write by {}.", verifier);
    Ok(())
}

