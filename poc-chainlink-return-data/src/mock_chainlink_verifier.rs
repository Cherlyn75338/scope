use anchor_lang::prelude::*;
use solana_program::program::set_return_data;
use chainlink_streams_report::report::v3::{ReportDataV3, FeedID};
use num_bigint::BigInt;

declare_id!("ChainMock111111111111111111111111111111111");

#[program]
pub mod mock_chainlink_verifier {
    use super::*;

    /// Mock Chainlink verifier that simulates the vulnerable behavior:
    /// - It succeeds the CPI call
    /// - But it doesn't set return data (simulating a bug or edge case)
    /// This allows the attacker's previously set return data to remain as "last writer"
    pub fn verify_without_return_data(
        ctx: Context<MockVerify>,
        _serialized_report: Vec<u8>,
    ) -> Result<()> {
        msg!("📡 MOCK CHAINLINK: Verify called");
        msg!("📡 MOCK CHAINLINK: Processing report validation...");
        
        // Simulate successful verification
        msg!("📡 MOCK CHAINLINK: ✅ Report signature valid");
        
        // VULNERABILITY: Not setting return data!
        // In a real scenario, this could happen if:
        // 1. The verifier has a bug
        // 2. The verifier calls another program that overwrites return data
        // 3. The verifier only sets return data on certain paths
        
        msg!("📡 MOCK CHAINLINK: ⚠️ NOT setting return data (simulating vulnerability)");
        msg!("📡 MOCK CHAINLINK: This leaves attacker's data as last writer!");
        
        Ok(())
    }

    /// Alternative scenario: Verifier sets return data but then calls another program
    /// that overwrites it
    pub fn verify_with_overwrite(
        ctx: Context<MockVerify>,
        serialized_report: Vec<u8>,
    ) -> Result<()> {
        msg!("📡 MOCK CHAINLINK ALT: Verify with potential overwrite");
        
        // First, set legitimate return data
        let legitimate_report = create_legitimate_report();
        let encoded = legitimate_report.abi_encode();
        set_return_data(&encoded);
        msg!("📡 MOCK CHAINLINK ALT: Set legitimate return data");
        
        // Simulate calling another program that might overwrite
        // (In this PoC, we just clear it to simulate the issue)
        set_return_data(&[]);
        msg!("📡 MOCK CHAINLINK ALT: ⚠️ Return data overwritten/cleared!");
        
        Ok(())
    }

    /// Scenario where verifier succeeds but sets incomplete/malformed data
    pub fn verify_with_malformed_data(
        ctx: Context<MockVerify>,
        _serialized_report: Vec<u8>,
    ) -> Result<()> {
        msg!("📡 MOCK CHAINLINK MALFORMED: Setting malformed return data");
        
        // Set some data that's not a valid report
        // This simulates a verifier bug where it sets wrong data
        set_return_data(b"INVALID_DATA");
        msg!("📡 MOCK CHAINLINK MALFORMED: Set non-report data");
        
        Ok(())
    }
}

fn create_legitimate_report() -> ReportDataV3 {
    ReportDataV3 {
        feed_id: FeedID([0u8; 32]),
        valid_from_timestamp: 1000,
        observations_timestamp: 1000,
        native_fee: BigInt::from(0),
        link_fee: BigInt::from(0),
        expires_at: 2000,
        benchmark_price: BigInt::from(100_000_000), // $100
        bid: BigInt::from(99_900_000),
        ask: BigInt::from(100_100_000),
    }
}

#[derive(Accounts)]
pub struct MockVerify<'info> {
    pub verifier_account: Signer<'info>,
    /// CHECK: Mock account
    pub access_controller: AccountInfo<'info>,
    pub user: Signer<'info>,
    /// CHECK: Mock account  
    pub config_account: AccountInfo<'info>,
}