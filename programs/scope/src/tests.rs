#![cfg(test)]
use anchor_lang::{prelude::*, InstructionData, ToAccountMetas};
use chainlink_streams_report::{feed_id::ID as FeedID, report::v3::ReportDataV3};
use prost::Message as _;
use solana_program::{instruction::Instruction, program::set_return_data, pubkey::Pubkey};
use solana_program_test::*;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer, system_instruction::create_account, transaction::Transaction};

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
}

