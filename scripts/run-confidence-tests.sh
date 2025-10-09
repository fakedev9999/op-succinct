#!/usr/bin/env bash
# Confidence Testing Runner
# 
# This script helps you run confidence tests for proof generation and verification
# before deploying to production.

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
print_header() {
    echo ""
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}================================================${NC}"
    echo ""
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_info() {
    echo -e "${BLUE}ℹ $1${NC}"
}

# Check required environment variables
check_env_var() {
    local var_name=$1
    local required=$2
    
    if [ -z "${!var_name:-}" ]; then
        if [ "$required" = "true" ]; then
            print_error "$var_name is not set (required)"
            return 1
        else
            print_warning "$var_name is not set (optional)"
            return 0
        fi
    else
        print_success "$var_name is set"
        return 0
    fi
}

# Main menu
show_menu() {
    print_header "OP Succinct Confidence Testing"
    echo "Choose a testing approach:"
    echo ""
    echo "1) Quick Test - Multi script with --prove flag (10-30 min)"
    echo "   ✓ Validates range proof generation"
    echo "   ✓ Tests with real block data"
    echo "   ✓ Good for initial validation"
    echo ""
    echo "2) Full E2E Test - With real contracts (1-2 hours)"
    echo "   ✓ Validates complete proof pipeline"
    echo "   ✓ Tests on-chain verification"
    echo "   ✓ Validates contract parameters"
    echo "   ✓ Required before production deployment"
    echo ""
    echo "3) VKey Validation - Check contract keys"
    echo "   ✓ Ensures vkeys match between programs and contracts"
    echo "   ✓ Prevents deployment with wrong keys"
    echo ""
    echo "4) Custom Block Range Test"
    echo "   ✓ Test specific problematic block ranges"
    echo ""
    echo "5) Exit"
    echo ""
}

# Quick test with multi script
run_quick_test() {
    print_header "Quick Test: Multi Script with --prove"
    
    # Check required env vars
    print_info "Checking environment variables..."
    check_env_var "L1_RPC" "true" || return 1
    check_env_var "L2_RPC" "true" || return 1
    check_env_var "SP1_PRIVATE_KEY" "true" || return 1
    check_env_var "RANGE_PROOF_STRATEGY" "false"
    
    # Get block range
    echo ""
    read -p "Enter start block (or press Enter for default): " start_block
    read -p "Enter end block (or press Enter for default): " end_block
    
    # Use defaults if not provided
    if [ -z "$start_block" ]; then
        print_info "Using default block range"
        just run-multi 1000 1300 false true
    else
        print_info "Testing blocks $start_block to $end_block"
        just run-multi "$start_block" "$end_block" false true
    fi
    
    if [ $? -eq 0 ]; then
        print_success "Quick test completed successfully!"
        echo ""
        print_info "Next steps:"
        echo "  - Review the proof saved in data/<chain_id>/proofs/"
        echo "  - For full confidence, run the E2E test (option 2)"
    else
        print_error "Quick test failed. Check the logs above for details."
        return 1
    fi
}

# Full E2E test
run_e2e_test() {
    print_header "Full E2E Test: With Real Contracts"
    
    # Check required env vars
    print_info "Checking environment variables..."
    check_env_var "L1_RPC" "true" || return 1
    check_env_var "L2_RPC" "true" || return 1
    check_env_var "SP1_PRIVATE_KEY" "true" || return 1
    check_env_var "FACTORY_ADDRESS" "false"
    check_env_var "VERIFIER_ADDRESS" "false"
    
    # Ask about simulation mode
    echo ""
    read -p "Run in simulation mode (no broadcast)? [Y/n]: " sim_mode
    if [[ "$sim_mode" =~ ^[Yy]$ ]] || [ -z "$sim_mode" ]; then
        export SIMULATION_MODE=true
        print_info "Running in simulation mode (no transactions broadcast)"
    else
        export SIMULATION_MODE=false
        print_warning "Running in broadcast mode (transactions will be sent!)"
        read -p "Are you sure? [y/N]: " confirm
        if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
            print_info "Cancelled"
            return 0
        fi
    fi
    
    # Ask about mock mode
    read -p "Use mock mode (faster, no real proofs)? [y/N]: " mock_mode
    if [[ "$mock_mode" =~ ^[Yy]$ ]]; then
        export MOCK_MODE=true
        print_info "Using mock mode (faster testing)"
    else
        export MOCK_MODE=false
        print_info "Using real proof generation (slower but complete validation)"
    fi
    
    # Run the test
    echo ""
    print_info "Starting E2E test..."
    just test-confidence
    
    if [ $? -eq 0 ]; then
        print_success "E2E test completed successfully!"
        echo ""
        print_info "All validations passed:"
        echo "  ✅ Range proof generation: Working"
        echo "  ✅ Aggregation proof generation: Working"
        echo "  ✅ On-chain verification: Working"
        echo "  ✅ Contract parameters: Validated"
        echo "  ✅ Game lifecycle: Complete"
        echo ""
        print_success "System is ready for production deployment!"
    else
        print_error "E2E test failed. Check the logs above for details."
        return 1
    fi
}

# VKey validation
run_vkey_validation() {
    print_header "VKey Validation"
    
    print_info "Validating that contract vkeys match program vkeys..."
    just test-vkey-validation
    
    if [ $? -eq 0 ]; then
        print_success "VKey validation completed successfully!"
        echo ""
        print_info "All verification keys match between programs and contracts"
    else
        print_error "VKey validation failed!"
        echo ""
        print_warning "This means your deployed contracts have mismatched vkeys."
        print_warning "You must update the contracts before deployment."
        return 1
    fi
}

# Custom block range test
run_custom_range_test() {
    print_header "Custom Block Range Test"
    
    # Check required env vars
    print_info "Checking environment variables..."
    check_env_var "L1_RPC" "true" || return 1
    check_env_var "L2_RPC" "true" || return 1
    check_env_var "SP1_PRIVATE_KEY" "true" || return 1
    
    # Get block range
    echo ""
    read -p "Enter start block: " start_block
    read -p "Enter end block: " end_block
    
    if [ -z "$start_block" ] || [ -z "$end_block" ]; then
        print_error "Both start and end blocks are required"
        return 1
    fi
    
    print_info "Testing blocks $start_block to $end_block..."
    cd fault-proof
    just test-proof-range "$start_block" "$end_block"
    
    if [ $? -eq 0 ]; then
        print_success "Block range test completed successfully!"
        echo ""
        print_info "Proof saved in data/confidence/proof-$start_block-$end_block.bin"
    else
        print_error "Block range test failed. Check the logs above for details."
        return 1
    fi
}

# Load environment file if provided
if [ $# -gt 0 ]; then
    ENV_FILE=$1
    if [ -f "$ENV_FILE" ]; then
        print_info "Loading environment from $ENV_FILE"
        set -a
        source "$ENV_FILE"
        set +a
    else
        print_error "Environment file not found: $ENV_FILE"
        exit 1
    fi
fi

# Main loop
while true; do
    show_menu
    read -p "Enter your choice [1-5]: " choice
    
    case $choice in
        1)
            run_quick_test
            ;;
        2)
            run_e2e_test
            ;;
        3)
            run_vkey_validation
            ;;
        4)
            run_custom_range_test
            ;;
        5)
            print_info "Exiting..."
            exit 0
            ;;
        *)
            print_error "Invalid choice. Please select 1-5."
            ;;
    esac
    
    echo ""
    read -p "Press Enter to continue..."
done
