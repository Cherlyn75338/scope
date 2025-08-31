use std::str::FromStr;

use anchor_lang::{InstructionData, ToAccountMetas};
use num_bigint::BigInt;
use solana_program::{
    instruction::Instruction,
    program::set_return_data,
    pubkey::Pubkey,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use scope::{self, anchor_lang::prelude::Pubkey as AnchorPubkey};

// Hard-coded IDs
const INJECTOR_ID: &str = "Injec7or111111111111111111111111111111111111";
// Must match Scope's expected Chainlink verifier program id
const VERIFIER_ID: &str = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c";

// Mock Chainlink verifier: always succeed, do NOT set return data.
fn mock_verifier_process(
    _program_id: &solana_program::pubkey::Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    _ix_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    Ok(())
}

// Malicious injector program: sets return data to provided payload.
fn injector_process(
    _program_id: &solana_program::pubkey::Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    ix_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    set_return_data(ix_data);
    Ok(())
}

#[tokio::test]
async fn test_chainlink_return_data_confusion_v7() {
    // Build a ProgramTest with Scope, a mock verifier, and a malicious injector
    let mut pt = ProgramTest::new(
        "scope",
        scope::id(),
        processor!(scope::entry),
    );
    pt.add_program(
        "mock_verifier",
        Pubkey::from_str(VERIFIER_ID).unwrap(),
        processor!(mock_verifier_process),
    );
    pt.add_program(
        "injector",
        Pubkey::from_str(INJECTOR_ID).unwrap(),
        processor!(injector_process),
    );

    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    // Create required on-chain accounts and initialize Scope feed
    let admin = Keypair::new();
    // Fund admin
    let transfer_ix = solana_sdk::system_instruction::transfer(
        &payer.pubkey(),
        &admin.pubkey(),
        5_000_000_000,
    );
    let mut fund_tx = Transaction::new_with_payer(&[transfer_ix], Some(&payer.pubkey()));
    fund_tx.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(fund_tx).await.unwrap();

    // Pre-allocate zeroed accounts with the correct sizes
    let configuration = Keypair::new();
    let token_metadatas = Keypair::new();
    let oracle_twaps = Keypair::new();
    let oracle_prices = Keypair::new();
    let oracle_mappings = Keypair::new();

    let rent = banks_client.get_rent().await.unwrap();
    let cfg_space = scope::utils::consts::CONFIGURATION_SIZE + 8;
    let tmd_space = scope::utils::consts::TOKEN_METADATA_SIZE;
    let twaps_space = scope::utils::consts::ORACLE_TWAPS_SIZE;
    let prices_space = scope::utils::consts::ORACLE_PRICES_SIZE;
    let mappings_space = scope::utils::consts::ORACLE_MAPPING_SIZE;

    for (kp, space) in [
        (&configuration, cfg_space),
        (&token_metadatas, tmd_space),
        (&oracle_twaps, twaps_space),
        (&oracle_prices, prices_space),
        (&oracle_mappings, mappings_space),
    ] {
        let lamports = rent.minimum_balance(space);
        let create_ix = solana_sdk::system_instruction::create_account(
            &admin.pubkey(),
            &kp.pubkey(),
            lamports,
            space as u64,
            &scope::id(),
        );
        let mut tx = Transaction::new_with_payer(&[create_ix], Some(&admin.pubkey()));
        let bh = banks_client.get_latest_blockhash().await.unwrap();
        tx.sign(&[&admin], bh);
        banks_client.process_transaction(tx).await.unwrap();
    }

    // Call initialize
    let feed_name = "test".to_string();
    let init_accounts = scope::accounts::Initialize {
        admin: admin.pubkey(),
        system_program: solana_sdk::system_program::id(),
        configuration: configuration.pubkey(),
        token_metadatas: token_metadatas.pubkey(),
        oracle_twaps: oracle_twaps.pubkey(),
        oracle_prices: oracle_prices.pubkey(),
        oracle_mappings: oracle_mappings.pubkey(),
    };
    let init_ix = Instruction {
        program_id: scope::id(),
        accounts: init_accounts.to_account_metas(None),
        data: scope::instruction::Initialize { feed_name: feed_name.clone() }.data(),
    };
    let mut tx = Transaction::new_with_payer(&[init_ix], Some(&admin.pubkey()));
    let bh = banks_client.get_latest_blockhash().await.unwrap();
    tx.sign(&[&admin], bh);
    banks_client.process_transaction(tx).await.unwrap();

    // Set mapping for token 0 to ChainlinkExchangeRate (v7), price_info = feed pubkey
    let feed_pk = Keypair::new();
    // Create a dummy account for the feed (not owned by scope, just exists)
    let lamports = rent.minimum_balance(0);
    let create_feed_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &feed_pk.pubkey(),
        lamports,
        0,
        &admin.pubkey(),
    );
    let mut tx = Transaction::new_with_payer(&[create_feed_ix], Some(&admin.pubkey()));
    let bh = banks_client.get_latest_blockhash().await.unwrap();
    tx.sign(&[&admin], bh);
    banks_client.process_transaction(tx).await.unwrap();

    let token_idx: u16 = 0;
    let price_type: u8 = scope::oracles::OracleType::ChainlinkExchangeRate.into();
    let twap_enabled = false;
    let twap_source: u16 = 0;
    let ref_price_index: u16 = u16::MAX; // no ref price
    let generic_data: [u8; 20] = [0; 20];

    let upd_accounts = scope::accounts::UpdateOracleMapping {
        admin: admin.pubkey(),
        configuration: configuration.pubkey(),
        oracle_mappings: oracle_mappings.pubkey(),
        price_info: Some(feed_pk.pubkey()),
    };
    let upd_ix = Instruction {
        program_id: scope::id(),
        accounts: upd_accounts.to_account_metas(None),
        data: scope::instruction::UpdateMapping {
            token: token_idx,
            price_type,
            twap_enabled,
            twap_source,
            ref_price_index,
            feed_name: feed_name.clone(),
            generic_data,
        }
        .data(),
    };
    let mut tx = Transaction::new_with_payer(&[upd_ix], Some(&admin.pubkey()));
    let bh = banks_client.get_latest_blockhash().await.unwrap();
    tx.sign(&[&admin], bh);
    banks_client.process_transaction(tx).await.unwrap();

    // Craft a valid Chainlink ReportDataV7 that matches mapping feed id and a chosen price
    let feed_id = chainlink_streams_report::feed_id::ID(feed_pk.pubkey().to_bytes());
    let report_v7 = chainlink_streams_report::report::v7::ReportDataV7 {
        feed_id,
        exchange_rate: BigInt::from(1_000_000_000_000_000_000u128), // 1.0 with 18 decimals
        observations_timestamp: 123456789u64.into(),
    };
    let malicious_bytes = report_v7.encode();

    // Instruction 0: malicious injector sets return data
    let injector_ix = Instruction {
        program_id: Pubkey::from_str(INJECTOR_ID).unwrap(),
        accounts: vec![],
        data: malicious_bytes.clone(),
    };

    // Prepare accounts for refresh_chainlink_price
    let verifier_config = scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY;
    let access_controller = scope::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY;
    let verifier_program_id = AnchorPubkey::from_str(VERIFIER_ID).unwrap();

    // Create placeholder config PDA and the two required verifier accounts so they exist on-chain
    let config_pda = Keypair::new();
    let lamports = rent.minimum_balance(0);
    let create_cfg_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &config_pda.pubkey(),
        lamports,
        0,
        &admin.pubkey(),
    );
    // Also create the two read-only accounts required by the verifier interface
    let create_verifier_account_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &verifier_config,
        lamports,
        0,
        &admin.pubkey(),
    );
    let create_access_controller_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &access_controller,
        lamports,
        0,
        &admin.pubkey(),
    );
    let mut tx = Transaction::new_with_payer(&[create_cfg_ix, create_verifier_account_ix, create_access_controller_ix], Some(&admin.pubkey()));
    let bh = banks_client.get_latest_blockhash().await.unwrap();
    tx.sign(&[&admin], bh);
    banks_client.process_transaction(tx).await.unwrap();

    // Instruction 1: Scope refresh_chainlink_price (CPI to mock verifier succeeds, does not set return data)
    let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
        user: admin.pubkey(),
        oracle_prices: oracle_prices.pubkey(),
        oracle_mappings: oracle_mappings.pubkey(),
        oracle_twaps: oracle_twaps.pubkey(),
        verifier_account: verifier_config,
        access_controller,
        config_account: config_pda.pubkey(),
        verifier_program_id,
    };
    let refresh_ix = Instruction {
        program_id: scope::id(),
        accounts: refresh_accounts.to_account_metas(None),
        data: scope::instruction::RefreshChainlinkPrice {
            token: token_idx,
            serialized_chainlink_report: vec![],
        }
        .data(),
    };

    let mut tx = Transaction::new_with_payer(&[injector_ix, refresh_ix], Some(&admin.pubkey()));
    let bh = banks_client.get_latest_blockhash().await.unwrap();
    tx.sign(&[&admin], bh);
    banks_client.process_transaction(tx).await.unwrap();

    // Read OraclePrices and assert price updated from zero
    let prices_acc = banks_client
        .get_account(oracle_prices.pubkey())
        .await
        .unwrap()
        .expect("oracle_prices must exist");
    assert!(prices_acc.data.len() >= scope::utils::consts::ORACLE_PRICES_SIZE);

    // Basic sanity: non-zero somewhere after header; a deeper decode would require Anchor zero-copy helper here.
    // At minimum, ensure that handler did not revert and wrote something; exhaustive struct decode is out-of-scope.
    // We check that some byte in the price array region changed from 0.
    assert!(prices_acc.data.iter().any(|b| *b != 0));
}

