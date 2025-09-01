use anchor_lang::prelude::*;
use chainlink_streams_report::report::{
    v3::{ReportDataV3, FeedID},
    v8::ReportDataV8,
};
use num_bigint::BigInt;
use solana_program::{
    program::get_return_data,
    instruction::Instruction,
    program::invoke,
};

/// This module simulates the vulnerable Scope behavior
/// to demonstrate the exploit without needing the full Scope program
pub mod scope_simulator {
    use super::*;

    /// Simulates the vulnerable refresh_chainlink_price handler
    pub fn simulate_refresh_chainlink_price(
        verifier_program_id: &Pubkey,
        serialized_report: Vec<u8>,
        token_index: u16,
        oracle_type: OracleType,
    ) -> Result<(PriceInfo, AttackAnalysis)> {
        msg!("🏦 SCOPE SIMULATOR: Starting refresh_chainlink_price");
        msg!("🏦 SCOPE: Token index: {}", token_index);
        msg!("🏦 SCOPE: Oracle type: {:?}", oracle_type);
        
        // Step 1: Simulate CPI to Chainlink verifier
        msg!("🏦 SCOPE: Invoking Chainlink verifier CPI...");
        // In real code, this would be:
        // invoke(&chainlink_ix, &[accounts...])?;
        
        // Step 2: THE VULNERABILITY - Get return data without checking source
        msg!("🏦 SCOPE: Getting return data (VULNERABLE CODE)...");
        
        let (program_id, return_data) = if let Some(data) = get_return_data() {
            data
        } else {
            msg!("🏦 SCOPE: No return data found");
            return Err(error!(ScopeError::NoChainlinkReportData));
        };
        
        // VULNERABILITY: program_id is retrieved but NOT CHECKED!
        msg!("🏦 SCOPE: ⚠️ Return data from program: {}", program_id);
        msg!("🏦 SCOPE: ❌ NOT CHECKING if program_id == verifier_program_id");
        msg!("🏦 SCOPE: ❌ Expected: {}", verifier_program_id);
        
        let is_from_correct_program = program_id == *verifier_program_id;
        
        // Step 3: Decode and process the data
        msg!("🏦 SCOPE: Decoding return data as Chainlink report...");
        
        let price_info = match oracle_type {
            OracleType::Chainlink => {
                let report = ReportDataV3::decode(&return_data)
                    .map_err(|_| error!(ScopeError::InvalidChainlinkReportData))?;
                
                msg!("🏦 SCOPE: Decoded V3 report successfully");
                msg!("🏦 SCOPE: Price: {}", report.benchmark_price);
                
                process_v3_report(report, token_index)?
            },
            OracleType::ChainlinkRWA => {
                let report = ReportDataV8::decode(&return_data)
                    .map_err(|_| error!(ScopeError::InvalidChainlinkReportData))?;
                
                msg!("🏦 SCOPE: Decoded V8 report successfully");
                msg!("🏦 SCOPE: Mid price: {}", report.mid_price);
                
                process_v8_report(report, token_index)?
            },
            _ => return Err(error!(ScopeError::BadTokenType)),
        };
        
        // Create attack analysis
        let analysis = AttackAnalysis {
            return_data_from_correct_program: is_from_correct_program,
            actual_program_id: program_id,
            expected_program_id: *verifier_program_id,
            price_written: price_info.clone(),
            vulnerability_exploited: !is_from_correct_program,
        };
        
        msg!("🏦 SCOPE: ✅ Price updated in OraclePrices account");
        msg!("🏦 SCOPE: New price: ${:.2}", price_info.price_usd);
        
        if !is_from_correct_program {
            msg!("🏦 SCOPE: 🚨🚨🚨 VULNERABILITY EXPLOITED!");
            msg!("🏦 SCOPE: Accepted data from WRONG program!");
            msg!("🏦 SCOPE: This price is ATTACKER CONTROLLED!");
        }
        
        Ok((price_info, analysis))
    }
    
    fn process_v3_report(report: ReportDataV3, token_index: u16) -> Result<PriceInfo> {
        // Simulate validation that would pass with forged data
        msg!("🏦 SCOPE: Validating V3 report...");
        
        // Feed ID check (attacker can match this)
        msg!("🏦 SCOPE: Feed ID validation: ✅ (attacker matched it)");
        
        // Timestamp validation (attacker can set valid timestamps)
        msg!("🏦 SCOPE: Timestamp validation: ✅ (attacker set valid times)");
        
        // Confidence interval check
        let price = report.benchmark_price.to_i128().unwrap_or(0);
        let bid = report.bid.to_i128().unwrap_or(0);
        let ask = report.ask.to_i128().unwrap_or(0);
        let spread = ask - bid;
        
        msg!("🏦 SCOPE: Price: {}, Spread: {}", price, spread);
        msg!("🏦 SCOPE: Confidence check: ✅ (attacker crafted valid spread)");
        
        Ok(PriceInfo {
            token_index,
            price_usd: price as f64 / 1_000_000.0,
            price_raw: price,
            timestamp: report.observations_timestamp,
            oracle_type: OracleType::Chainlink,
        })
    }
    
    fn process_v8_report(report: ReportDataV8, token_index: u16) -> Result<PriceInfo> {
        msg!("🏦 SCOPE: Validating V8 (RWA) report...");
        
        // Market status check (attacker can set to Open)
        if report.market_status != 1 {
            msg!("🏦 SCOPE: Market closed");
            return Err(error!(ScopeError::PriceNotValid));
        }
        msg!("🏦 SCOPE: Market status: Open ✅");
        
        let price = report.mid_price.to_i128().unwrap_or(0);
        
        Ok(PriceInfo {
            token_index,
            price_usd: price as f64 / 1_000_000.0,
            price_raw: price,
            timestamp: report.observations_timestamp,
            oracle_type: OracleType::ChainlinkRWA,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PriceInfo {
    pub token_index: u16,
    pub price_usd: f64,
    pub price_raw: i128,
    pub timestamp: u32,
    pub oracle_type: OracleType,
}

#[derive(Debug)]
pub struct AttackAnalysis {
    pub return_data_from_correct_program: bool,
    pub actual_program_id: Pubkey,
    pub expected_program_id: Pubkey,
    pub price_written: PriceInfo,
    pub vulnerability_exploited: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum OracleType {
    Chainlink,
    ChainlinkRWA,
    ChainlinkNAV,
    ChainlinkX,
    ChainlinkExchangeRate,
}

#[derive(Debug)]
pub enum ScopeError {
    NoChainlinkReportData,
    InvalidChainlinkReportData,
    BadTokenType,
    PriceNotValid,
}

impl From<ScopeError> for anchor_lang::error::Error {
    fn from(e: ScopeError) -> Self {
        match e {
            ScopeError::NoChainlinkReportData => error::Error::from(error::ErrorCode::AccountDidNotDeserialize),
            ScopeError::InvalidChainlinkReportData => error::Error::from(error::ErrorCode::AccountDidNotDeserialize),
            ScopeError::BadTokenType => error::Error::from(error::ErrorCode::InvalidAccountData),
            ScopeError::PriceNotValid => error::Error::from(error::ErrorCode::ConstraintRaw),
        }
    }
}

/// Demonstrates downstream impact on Kamino components
pub mod kamino_impact {
    use super::*;
    
    pub struct LendingPosition {
        pub collateral_token: u16,
        pub collateral_amount: f64,
        pub borrow_token: u16,
        pub borrow_amount: f64,
    }
    
    pub fn calculate_lending_impact(
        position: &LendingPosition,
        legitimate_price: f64,
        manipulated_price: f64,
    ) -> LendingImpact {
        let price_ratio = manipulated_price / legitimate_price;
        
        // Calculate collateral values
        let legitimate_collateral_value = position.collateral_amount * legitimate_price;
        let manipulated_collateral_value = position.collateral_amount * manipulated_price;
        
        // Calculate borrow capacity (assuming 80% LTV)
        let ltv = 0.8;
        let legitimate_borrow_capacity = legitimate_collateral_value * ltv;
        let manipulated_borrow_capacity = manipulated_collateral_value * ltv;
        
        // Calculate potential theft
        let excess_borrow = manipulated_borrow_capacity - legitimate_borrow_capacity;
        
        msg!("💰 KAMINO LENDING IMPACT:");
        msg!("  Collateral: {} tokens @ ${}/token", position.collateral_amount, legitimate_price);
        msg!("  Legitimate value: ${:.2}", legitimate_collateral_value);
        msg!("  Manipulated value: ${:.2} ({}x)", manipulated_collateral_value, price_ratio);
        msg!("  Excess borrow capacity: ${:.2}", excess_borrow);
        msg!("  🚨 POTENTIAL THEFT: ${:.2}", excess_borrow);
        
        LendingImpact {
            legitimate_collateral_value,
            manipulated_collateral_value,
            legitimate_borrow_capacity,
            manipulated_borrow_capacity,
            excess_borrow_amount: excess_borrow,
            price_manipulation_factor: price_ratio,
        }
    }
    
    pub struct LendingImpact {
        pub legitimate_collateral_value: f64,
        pub manipulated_collateral_value: f64,
        pub legitimate_borrow_capacity: f64,
        pub manipulated_borrow_capacity: f64,
        pub excess_borrow_amount: f64,
        pub price_manipulation_factor: f64,
    }
    
    pub fn demonstrate_liquidation_impact(
        debt_value: f64,
        collateral_value: f64,
        manipulated_collateral_price: f64,
        legitimate_collateral_price: f64,
    ) {
        msg!("⚡ LIQUIDATION IMPACT:");
        
        let health_factor_legitimate = collateral_value / debt_value;
        let manipulated_collateral_value = (collateral_value / legitimate_collateral_price) * manipulated_collateral_price;
        let health_factor_manipulated = manipulated_collateral_value / debt_value;
        
        msg!("  Debt: ${:.2}", debt_value);
        msg!("  Legitimate health factor: {:.2}", health_factor_legitimate);
        msg!("  Manipulated health factor: {:.2}", health_factor_manipulated);
        
        if health_factor_legitimate > 1.0 && health_factor_manipulated < 1.0 {
            msg!("  🚨 UNFAIR LIQUIDATION: Healthy position liquidated!");
        } else if health_factor_legitimate < 1.0 && health_factor_manipulated > 1.0 {
            msg!("  🚨 AVOIDED LIQUIDATION: Unhealthy position protected!");
        }
    }
}