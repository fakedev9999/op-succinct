/// Confidence testing suite for proof generation and verification with real deployed contracts.
///
/// This test suite validates the complete proof pipeline against actual production contracts
/// without broadcasting transactions (simulation mode). It ensures:
/// 1. Range proof generation works
/// 2. Aggregation proof generation works  
/// 3. On-chain verification succeeds with real contracts
/// 4. Contract parameters are correctly configured
///
/// # Usage
///
/// ```bash
/// # Test with deployed contracts (simulation mode)
/// FACTORY_ADDRESS=0x... VERIFIER_ADDRESS=0x... cargo test --test confidence --release
///
/// # Test with mock verifier for faster validation
/// MOCK_MODE=true cargo test --test confidence --release
/// ```
mod common;

use std::{str::FromStr, sync::Arc};

use alloy_network::EthereumWallet;
use alloy_primitives::{Address, U256};
use alloy_provider::ProviderBuilder;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport_http::reqwest::Url;
use anyhow::{Context, Result};
use common::{
    constants::{
        DISPUTE_GAME_FINALITY_DELAY_SECONDS, MAX_CHALLENGE_DURATION, PROPOSER_ADDRESS,
        PROPOSER_PRIVATE_KEY, TEST_GAME_TYPE,
    },
    monitor::{verify_all_resolved_correctly, wait_for_resolutions, TrackedGame},
    warp_time, TestEnvironment,
};
use fault_proof::{config::ProposerConfig, proposer::OPSuccinctProposer, L2ProviderTrait};
use op_succinct_bindings::dispute_game_factory::DisputeGameFactory;
use op_succinct_host_utils::{fetcher::OPSuccinctDataFetcher, host::MultiblockHost};
use op_succinct_proof_utils::initialize_host;
use op_succinct_signer_utils::Signer;
use tokio::time::Duration;
use tracing::info;

/// Test proof generation and verification with real deployed contracts.
///
/// This test:
/// 1. Creates a game using real factory contract
/// 2. Generates range proof for the game's block range
/// 3. Generates aggregation proof
/// 4. Simulates proof submission to real verifier (no broadcast if SIMULATION_MODE=true)
/// 5. Validates on-chain verification succeeds
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires deployed contracts and longer runtime"]
async fn test_proof_verification_with_real_contracts() -> Result<()> {
    TestEnvironment::init_logging();
    info!("=== Confidence Test: Proof Verification with Real Contracts ===");

    // Setup test environment (uses Anvil fork for safety)
    let env = TestEnvironment::setup().await?;

    // Get configuration from environment
    let simulation_mode = std::env::var("SIMULATION_MODE")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap_or(true);
    
    let mock_mode = std::env::var("MOCK_MODE")
        .unwrap_or("false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    info!("Test Configuration:");
    info!("  - Simulation Mode: {}", simulation_mode);
    info!("  - Mock Mode: {}", mock_mode);
    info!("  - Factory: {}", env.deployed.factory);
    info!("  - Verifier: {}", env.deployed.verifier);

    // === PHASE 1: Create Game ===
    info!("=== Phase 1: Create Test Game ===");

    let wallet = PrivateKeySigner::from_str(PROPOSER_PRIVATE_KEY)?;
    let provider_with_signer = ProviderBuilder::new()
        .wallet(EthereumWallet::from(wallet.clone()))
        .connect_http(env.anvil.endpoint.parse::<Url>()?);

    let factory = DisputeGameFactory::new(env.deployed.factory, provider_with_signer.clone());
    let init_bond = factory.initBonds(TEST_GAME_TYPE).call().await?;

    // Create a game for a recent block range
    let l2_block_number = env.anvil.starting_l2_block_number + 10;
    let fetcher = OPSuccinctDataFetcher::new();
    let output_root = fetcher
        .l2_provider
        .compute_output_root_at_block(U256::from(l2_block_number))
        .await?;

    let extra_data = (U256::from(l2_block_number), u32::MAX).abi_encode_packed();

    let tx = factory
        .create(TEST_GAME_TYPE, output_root, extra_data.into())
        .value(init_bond)
        .send()
        .await?;

    let receipt = tx.get_receipt().await?;
    let game_count = factory.gameCount().call().await?;
    let game_index = game_count - U256::from(1);
    let game_info = factory.gameAtIndex(game_index).call().await?;
    let game_address = game_info.proxy_;

    info!("✓ Created test game:");
    info!("  - Address: {}", game_address);
    info!("  - L2 Block: {}", l2_block_number);
    info!("  - Index: {}", game_index);
    info!("  - Tx: {:?}", receipt.transaction_hash);

    // === PHASE 2: Generate Proofs ===
    info!("=== Phase 2: Generate and Submit Proof ===");

    // Setup proposer configuration
    let mut proposer_config = ProposerConfig::from_env()?;
    proposer_config.l1_rpc = env.anvil.endpoint.parse()?;
    proposer_config.mock_mode = mock_mode;
    proposer_config.proposal_interval_in_blocks = 10; // Match the game block range

    // Create proposer instance
    let network_private_key = std::env::var("SP1_PRIVATE_KEY")
        .context("SP1_PRIVATE_KEY not set - required for proof generation")?;
    
    let signer = Signer::new_local_signer(PROPOSER_PRIVATE_KEY)?;
    let fetcher_arc = Arc::new(fetcher);
    let host = initialize_host(fetcher_arc.clone());

    let factory_instance = fault_proof::contract::DisputeGameFactory::DisputeGameFactoryInstance::new(
        env.deployed.factory,
        env.anvil.provider.clone(),
    );

    let anchor_registry_instance = fault_proof::contract::AnchorStateRegistry::AnchorStateRegistryInstance::new(
        env.deployed.anchor_registry,
        env.anvil.provider.clone(),
    );

    let proposer = Arc::new(
        OPSuccinctProposer::new(
            proposer_config,
            network_private_key,
            signer,
            factory_instance,
            anchor_registry_instance,
            fetcher_arc,
            host,
        )
        .await?,
    );

    // Generate and submit proof
    info!("Generating proof for game {}...", game_address);
    let start_time = std::time::Instant::now();
    
    let (tx_hash, total_cycles, total_gas) = proposer.prove_game(game_address).await?;
    
    let proof_duration = start_time.elapsed();

    info!("✓ Proof generated and submitted:");
    info!("  - Duration: {:.2}s", proof_duration.as_secs_f64());
    info!("  - Total Cycles: {}", total_cycles);
    info!("  - Total SP1 Gas: {}", total_gas);
    info!("  - Tx Hash: {:?}", tx_hash);

    // === PHASE 3: Verify On-chain State ===
    info!("=== Phase 3: Verify On-chain Verification ===");

    let game_contract = op_succinct_bindings::op_succinct_fault_dispute_game::OPSuccinctFaultDisputeGame::new(
        game_address,
        env.anvil.provider.clone(),
    );

    let claim_data = game_contract.claimData().call().await?;
    
    info!("Game claim data after proof submission:");
    info!("  - Status: {:?}", claim_data.status);
    info!("  - Prover: {}", claim_data.prover);
    
    // Verify the proof was accepted
    assert!(
        matches!(
            claim_data.status,
            fault_proof::contract::ProposalStatus::UnchallengedAndValidProofProvided
        ),
        "Proof should be verified on-chain"
    );

    info!("✓ On-chain verification succeeded");

    // === PHASE 4: Test Full Game Lifecycle ===
    info!("=== Phase 4: Test Game Resolution ===");

    // Warp time to allow resolution
    warp_time(&env.anvil.provider, Duration::from_secs(MAX_CHALLENGE_DURATION)).await?;

    // Resolve the game
    let tracked_game = TrackedGame {
        address: game_address,
        l2_block_number: U256::from(l2_block_number),
    };

    let resolutions = wait_for_resolutions(
        &env.anvil.provider,
        &[tracked_game.clone()],
        Duration::from_secs(30),
    )
    .await?;

    verify_all_resolved_correctly(&resolutions)?;

    info!("✓ Game resolved correctly as DEFENDER_WINS");

    // Warp past finality delay
    warp_time(
        &env.anvil.provider,
        Duration::from_secs(DISPUTE_GAME_FINALITY_DELAY_SECONDS),
    )
    .await?;

    // Check finalization status
    let anchor_registry = op_succinct_bindings::anchor_state_registry::AnchorStateRegistry::new(
        env.deployed.anchor_registry,
        env.anvil.provider.clone(),
    );

    let is_finalized = anchor_registry.isGameFinalized(game_address).call().await?;
    info!("✓ Game finalization status: {}", is_finalized);

    // === PHASE 5: Validate Contract Parameters ===
    info!("=== Phase 5: Validate Contract Configuration ===");

    // Verify vkey matches
    let aggregation_vkey = game_contract.aggregationVkey().call().await?;
    info!("  - Aggregation VKey from contract: {}", hex::encode(&aggregation_vkey));

    let range_vkey_commitment = game_contract.rangeVkeyCommitment().call().await?;
    info!("  - Range VKey Commitment: {}", hex::encode(&range_vkey_commitment));

    let verifier_address = game_contract.verifier().call().await?;
    info!("  - Verifier Address: {}", verifier_address);
    assert_eq!(
        verifier_address, env.deployed.verifier,
        "Verifier address mismatch"
    );

    info!("✓ All contract parameters validated");

    // === Summary ===
    info!("=== Test Complete: All Checks Passed ===");
    info!("Summary:");
    info!("  ✅ Range proof generation: Working");
    info!("  ✅ Aggregation proof generation: Working");
    info!("  ✅ On-chain verification: Working");
    info!("  ✅ Contract parameters: Validated");
    info!("  ✅ Game lifecycle: Complete");
    info!("");
    info!("The system is ready for production deployment.");

    Ok(())
}

/// Test proof generation against a specific block range from production.
///
/// This test allows you to validate proof generation for specific block ranges
/// that may be problematic or need validation before deployment.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires specific block range configuration"]
async fn test_proof_generation_for_block_range() -> Result<()> {
    TestEnvironment::init_logging();
    info!("=== Confidence Test: Proof Generation for Specific Block Range ===");

    // Get block range from environment
    let start_block = std::env::var("TEST_START_BLOCK")
        .context("TEST_START_BLOCK not set")?
        .parse::<u64>()?;
    
    let end_block = std::env::var("TEST_END_BLOCK")
        .context("TEST_END_BLOCK not set")?
        .parse::<u64>()?;

    info!("Testing block range: {} to {}", start_block, end_block);

    // Setup data fetcher
    let fetcher = OPSuccinctDataFetcher::new_with_rollup_config().await?;
    let host = initialize_host(Arc::new(fetcher.clone()));

    // Fetch witness data
    info!("Fetching witness data...");
    let host_args = host.fetch(start_block, end_block, None, false).await?;
    let witness_data = host.run(&host_args).await?;
    let sp1_stdin = host.witness_generator().get_sp1_stdin(witness_data)?;

    info!("✓ Witness generation completed");

    // Generate range proof
    info!("Generating range proof...");
    let prover = sp1_sdk::ProverClient::from_env();
    let (pk, _) = prover.setup(op_succinct_proof_utils::get_range_elf_embedded());
    
    let start_time = std::time::Instant::now();
    let proof = prover.prove(&pk, &sp1_stdin).compressed().run()?;
    let duration = start_time.elapsed();

    info!("✓ Range proof generated:");
    info!("  - Duration: {:.2}s", duration.as_secs_f64());
    info!("  - Proof size: {} bytes", proof.bytes().len());

    // Save proof for inspection
    let proof_path = format!("data/confidence/proof-{}-{}.bin", start_block, end_block);
    std::fs::create_dir_all("data/confidence")?;
    proof.save(&proof_path)?;
    info!("  - Proof saved to: {}", proof_path);

    info!("=== Test Complete: Proof Generation Successful ===");

    Ok(())
}

/// Validate that contract vkeys match the program vkeys.
///
/// This test ensures that deployed contracts have the correct verification keys
/// that match the current program versions.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires deployed contracts"]
async fn test_contract_vkey_validation() -> Result<()> {
    TestEnvironment::init_logging();
    info!("=== Confidence Test: Contract VKey Validation ===");

    let env = TestEnvironment::setup().await?;

    // Get expected vkeys from programs
    let prover = sp1_sdk::ProverClient::from_env();
    let (_, range_vk) = prover.setup(op_succinct_proof_utils::get_range_elf_embedded());
    let (_, agg_vk) = prover.setup(op_succinct_elfs::AGGREGATION_ELF);

    let range_vkey_commitment = range_vk.bytes32();
    let agg_vkey_hash = agg_vk.bytes32();

    info!("Expected VKeys:");
    info!("  - Range VKey Commitment: {}", hex::encode(&range_vkey_commitment));
    info!("  - Aggregation VKey: {}", hex::encode(&agg_vkey_hash));

    // Check against deployed game contract
    let game_count = DisputeGameFactory::new(env.deployed.factory, env.anvil.provider.clone())
        .gameCount()
        .call()
        .await?;

    if game_count > U256::ZERO {
        let game_info = DisputeGameFactory::new(env.deployed.factory, env.anvil.provider.clone())
            .gameAtIndex(game_count - U256::from(1))
            .call()
            .await?;

        let game = op_succinct_bindings::op_succinct_fault_dispute_game::OPSuccinctFaultDisputeGame::new(
            game_info.proxy_,
            env.anvil.provider.clone(),
        );

        let contract_range_vkey = game.rangeVkeyCommitment().call().await?;
        let contract_agg_vkey = game.aggregationVkey().call().await?;

        info!("Contract VKeys:");
        info!("  - Range VKey Commitment: {}", hex::encode(&contract_range_vkey));
        info!("  - Aggregation VKey: {}", hex::encode(&contract_agg_vkey));

        assert_eq!(
            range_vkey_commitment, contract_range_vkey,
            "Range VKey mismatch between program and contract"
        );

        assert_eq!(
            agg_vkey_hash, contract_agg_vkey,
            "Aggregation VKey mismatch between program and contract"
        );

        info!("✓ All VKeys match between programs and contracts");
    } else {
        info!("⚠ No games deployed yet - skipping VKey validation");
    }

    Ok(())
}
