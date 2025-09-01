use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::{UiTransactionEncoding, option_serializer::OptionSerializer};

const DEFAULT_MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const VERIFIER_PID: &str = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c";

fn last_program_return_is_verifier(logs: &[String], verifier: &str) -> Option<bool> {
    for line in logs.iter().rev() {
        if line.starts_with("Program return:") {
            // Line format typically: "Program return: <program_id> <data>"
            return Some(line.contains(verifier));
        }
    }
    None
}

fn main() -> Result<()> {
    // Args: [RPC_URL] [limit]
    let mut args = std::env::args().skip(1);
    let rpc_url = args.next().unwrap_or_else(|| DEFAULT_MAINNET_RPC.to_string());
    let limit: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);

    let client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());
    let verifier = VERIFIER_PID.parse::<Pubkey>().unwrap();

    println!("Scanning up to {} recent signatures for {} on {}", limit, verifier, rpc_url);

    let sigs = client
        .get_signatures_for_address_with_config(
            &verifier,
            solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config {
                limit: Some(limit),
                before: None,
                until: None,
                commitment: Some(CommitmentConfig::confirmed()),
            },
        )?;

    if sigs.is_empty() {
        return Err(anyhow!("No recent signatures found for verifier {}", verifier));
    }

    let mut total = 0usize;
    let mut with_logs = 0usize;
    let mut verifier_last = 0usize;

    for entry in sigs {
        let sig = entry.signature.parse::<Signature>()?;
        let tx = client.get_transaction(&sig, UiTransactionEncoding::Json)?;
        total += 1;
        if let Some(meta) = tx.transaction.meta.clone() {
            if meta.err.is_none() {
                match meta.log_messages {
                    OptionSerializer::Some(logs) => {
                        with_logs += 1;
                        if let Some(is_verifier) = last_program_return_is_verifier(&logs, &verifier.to_string()) {
                            if is_verifier { verifier_last += 1; }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    println!("Checked {} txs ({} with logs). Final Program return from verifier: {}", total, with_logs, verifier_last);
    println!("Note: If any successful tx shows a different last Program return producer than {}, verifier is not last-writer.", verifier);
    Ok(())
}

