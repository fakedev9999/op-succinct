# Proof Generation and Verification Confidence Testing

This document provides a quick reference for running confidence tests before upgrading to production with real proof generation and verification.

## Quick Start

### Interactive Script (Recommended)

The easiest way to run confidence tests is using the interactive script:

```bash
# Run interactive test menu
./scripts/run-confidence-tests.sh

# Or with a specific environment file
./scripts/run-confidence-tests.sh .env.sepolia
```

This will guide you through:
1. Choosing the right test for your needs
2. Checking environment variables
3. Configuring test parameters
4. Interpreting results

### Option 1: Quick Test with Multi Script (Minimal Approach)

Generate a real proof for a block range to validate the proof pipeline:

```bash
# Set up environment
export SP1_PRIVATE_KEY=your_sp1_private_key
export RANGE_PROOF_STRATEGY=hosted  # Use Succinct network for faster proving

# Run multi script with proof generation
just run-multi 1000 1300 false true

# Example output:
# ✓ Witness generation completed
# ✓ Range proof generated
# ✓ Proof saved to data/<chain_id>/proofs/1000-1300.bin
```

**What this validates:**
- ✅ Range proof generation works
- ✅ Block data can be processed correctly
- ✅ Proof format is valid

**Time:** ~10-30 minutes

### Option 2: Full E2E Test with Real Contracts (Comprehensive)

Test the complete pipeline with deployed contracts in simulation mode:

```bash
# Set up environment
export FACTORY_ADDRESS=0x...        # Your deployed DisputeGameFactory
export VERIFIER_ADDRESS=0x...       # Your deployed SP1Verifier
export SIMULATION_MODE=true         # Don't broadcast transactions
export MOCK_MODE=false              # Use real proof generation

# Run confidence test suite
just test-confidence

# Or run from fault-proof directory
cd fault-proof
just test-confidence
```

**What this validates:**
- ✅ Range proof generation works
- ✅ Aggregation proof generation works  
- ✅ On-chain verification succeeds
- ✅ Contract parameters are correct
- ✅ Full game lifecycle completes

**Time:** ~1-2 hours

## Available Test Commands

### 1. Full Confidence Test Suite

```bash
# Run all confidence tests
just test-confidence
```

Tests:
- Proof generation with real contracts
- On-chain verification simulation
- Contract parameter validation
- Full game lifecycle

### 2. Proof Generation for Specific Block Range

```bash
# Test a specific problematic block range
cd fault-proof
just test-proof-range 1000 1100
```

Useful for:
- Validating specific block ranges before deployment
- Testing edge cases
- Debugging proof generation issues

### 3. Contract VKey Validation

```bash
# Verify contract vkeys match program vkeys
just test-vkey-validation
```

Validates:
- Range VKey commitment matches
- Aggregation VKey matches
- Prevents deployment with mismatched keys

## Environment Variables

### Required for All Tests

```bash
L1_RPC=https://ethereum-sepolia-rpc.publicnode.com
L2_RPC=https://sepolia.optimism.io
L1_BEACON_RPC=https://ethereum-sepolia-beacon-api.publicnode.com
```

### Required for Proof Generation

```bash
SP1_PRIVATE_KEY=your_sp1_network_private_key
PROVER_NETWORK_RPC=https://rpc.succinct.xyz/

# Optional: Strategy for proof generation
RANGE_PROOF_STRATEGY=hosted  # or "reserved"
AGG_PROOF_STRATEGY=hosted    # or "reserved"
```

### Required for E2E Tests with Deployed Contracts

```bash
FACTORY_ADDRESS=0x...     # Your deployed DisputeGameFactory
VERIFIER_ADDRESS=0x...    # Your deployed SP1Verifier
SIMULATION_MODE=true      # Don't broadcast transactions
```

### Optional Settings

```bash
# Use mock mode for faster testing (skips real proof generation)
MOCK_MODE=true

# Enable detailed logging
RUST_LOG=info
```

## Interpreting Results

### Success Indicators

**Quick Test:**
```
✓ Witness generation completed
✓ Generating range proof...
✓ Proof generated successfully
✓ Proof saved to data/<chain_id>/proofs/<start>-<end>.bin
```

**E2E Test:**
```
✓ Created test game
✓ Proof generated and submitted
  - Duration: 123.45s
  - Total Cycles: 123456789
  - Total SP1 Gas: 987654321
✓ On-chain verification succeeded
✓ Game resolved correctly as DEFENDER_WINS
✓ All contract parameters validated
=== Test Complete: All Checks Passed ===
```

### Common Issues

#### "SP1_PRIVATE_KEY not set"
**Solution:** Set your SP1 network private key
```bash
export SP1_PRIVATE_KEY=your_key_here
```

#### "Failed to generate proof"
**Solutions:**
1. Check SP1 network status
2. Verify PROVER_NETWORK_RPC is accessible
3. Try with smaller block range
4. Use MOCK_MODE=true for testing without real proofs

#### "Verifier rejected proof" 
**Solutions:**
1. Verify VERIFIER_ADDRESS matches deployed contract
2. Check vkey configuration: `just test-vkey-validation`
3. Ensure AGGREGATION_VKEY is correct in contract

## Recommended Testing Workflow

### Before Production Deployment

**Week -2: Initial Validation**
```bash
# Quick sanity check
just run-multi 1000 1100 false true
```

**Week -1: Comprehensive Validation**
```bash
# Full E2E test with deployed contracts
export FACTORY_ADDRESS=0x...
export VERIFIER_ADDRESS=0x...
just test-confidence
```

**Day -1: Final Validation**
```bash
# Run both tests again
just run-multi 2000 2100 false true
just test-confidence

# Validate vkeys one more time
just test-vkey-validation
```

## Communicating Results to Customers

### After Quick Test

> "We have validated that range proof generation works correctly for production block data. The proof pipeline has been tested up to compressed proof generation, confirming the zkVM program processes blocks correctly."

### After E2E Test

> "We have completed end-to-end testing with the production contracts in a simulated environment. All tests passed:
> - ✅ Range proof generation: Working
> - ✅ Aggregation proof generation: Working  
> - ✅ On-chain verification: Working
> - ✅ Contract parameters: Validated
> - ✅ Game lifecycle: Complete
>
> The system is ready for production deployment."

## Further Documentation

- [Full Confidence Testing Guide](./book/fault_proofs/proof-confidence-testing.md)
- [Testing Guide](./book/fault_proofs/testing.md)
- [Fault Proof Architecture](./book/fault_proofs/fault_proof_architecture.md)
- [Deployment Guide](./book/fault_proofs/deploy.md)

## Support

If you encounter issues:
1. Check [Troubleshooting Guide](./book/troubleshooting.md)
2. Review test logs for specific error messages
3. Ensure all environment variables are set correctly
4. Try with MOCK_MODE=true to isolate proof generation issues
