#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${RED}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${RED}║          CHAINLINK RETURN DATA CONFUSION VULNERABILITY POC                  ║${NC}"
echo -e "${RED}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
echo ""

echo -e "${YELLOW}[*] Checking environment...${NC}"

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}[!] Cargo not found. Please install Rust first.${NC}"
    echo "    Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo -e "${GREEN}[✓] Rust/Cargo found${NC}"

# Build the project
echo -e "${YELLOW}[*] Building PoC programs...${NC}"
cargo build --release 2>/dev/null

if [ $? -eq 0 ]; then
    echo -e "${GREEN}[✓] Build successful${NC}"
else
    echo -e "${RED}[!] Build failed. Running with verbose output:${NC}"
    cargo build
    exit 1
fi

echo ""
echo -e "${BLUE}════════════════════════════════════════════════════════════════════════════════${NC}"
echo -e "${YELLOW}Select which test to run:${NC}"
echo -e "${BLUE}════════════════════════════════════════════════════════════════════════════════${NC}"
echo ""
echo "  1) Basic Exploit Demo - Shows the core vulnerability"
echo "  2) Complete Attack Flow - Full attack simulation with all steps"
echo "  3) DeFi Protocol Impact - Shows real-world financial impact"
echo "  4) Attack Speed Analysis - Demonstrates execution speed"
echo "  5) All Tests - Run all PoC tests"
echo "  6) Quick Validation Test - Verify the forged data passes validation"
echo ""
read -p "Enter choice [1-6]: " choice

echo ""

case $choice in
    1)
        echo -e "${YELLOW}[*] Running Basic Exploit Demo...${NC}"
        echo ""
        RUST_LOG=info cargo test test_chainlink_return_data_exploit -- --nocapture
        ;;
    2)
        echo -e "${YELLOW}[*] Running Complete Attack Flow...${NC}"
        echo ""
        RUST_LOG=info cargo test test_complete_attack_flow -- --nocapture
        ;;
    3)
        echo -e "${YELLOW}[*] Running DeFi Protocol Impact Analysis...${NC}"
        echo ""
        RUST_LOG=info cargo test test_defi_protocol_impact -- --nocapture
        ;;
    4)
        echo -e "${YELLOW}[*] Running Attack Speed Analysis...${NC}"
        echo ""
        RUST_LOG=info cargo test test_attack_speed -- --nocapture
        ;;
    5)
        echo -e "${YELLOW}[*] Running All Tests...${NC}"
        echo ""
        RUST_LOG=info cargo test -- --nocapture
        ;;
    6)
        echo -e "${YELLOW}[*] Running Quick Validation Test...${NC}"
        echo ""
        cargo test test_forged_report_encoding -- --nocapture
        ;;
    *)
        echo -e "${RED}[!] Invalid choice${NC}"
        exit 1
        ;;
esac

echo ""
echo -e "${BLUE}════════════════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}[✓] PoC execution complete${NC}"
echo -e "${BLUE}════════════════════════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${YELLOW}Key Findings:${NC}"
echo "  • Any wallet can manipulate oracle prices"
echo "  • No admin privileges required"
echo "  • Attack executes in < 0.5 seconds"
echo "  • Financial impact: UNBOUNDED"
echo ""
echo -e "${RED}Critical Fix Required:${NC}"
echo "  Add this check in refresh_chainlink_price:"
echo "  ${GREEN}if program_id != VERIFIER_PROGRAM_ID { return Err(...) }${NC}"
echo ""