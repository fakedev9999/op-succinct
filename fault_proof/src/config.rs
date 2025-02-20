use std::env;

use alloy_primitives::Address;
use alloy_transport_http::reqwest::Url;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct ProposerConfig {
    /// The L1 RPC URL.
    pub l1_rpc: Url,

    /// The L2 RPC URL.
    pub l2_rpc: Url,

    /// The address of the factory contract.
    pub factory_address: Address,

    /// The interval in blocks between proposing new games.
    pub proposal_interval_in_blocks: u64,

    /// The interval in seconds between checking for new proposals and game resolution.
    /// During each interval, the proposer:
    /// 1. Checks the safe L2 head block number
    /// 2. Gets the latest valid proposal
    /// 3. Creates a new game if conditions are met
    /// 4. Optionally attempts to resolve unchallenged games
    pub fetch_interval: u64,

    /// The type of game to propose.
    pub game_type: u32,

    /// Whether to enable game resolution.
    /// When game resolution is not enabled, the proposer will only propose new games.
    pub enable_game_resolution: bool,

    /// The number of games to check for resolution.
    /// When game resolution is enabled, the proposer will attempt to resolve games that are
    /// unchallenged up to `max_games_to_check_for_resolution` games behind the latest game.
    pub max_games_to_check_for_resolution: u64,
}

impl ProposerConfig {
    pub fn from_env() -> Result<Self> {
        dotenv::from_filename(".env.proposer").ok();

        Ok(Self {
            l1_rpc: env::var("L1_RPC")?.parse().expect("L1_RPC not set"),
            l2_rpc: env::var("L2_RPC")?.parse().expect("L2_RPC not set"),
            factory_address: env::var("FACTORY_ADDRESS")?
                .parse()
                .expect("FACTORY_ADDRESS not set"),
            proposal_interval_in_blocks: env::var("PROPOSAL_INTERVAL_IN_BLOCKS")
                .unwrap_or("1800".to_string())
                .parse()?,
            fetch_interval: env::var("FETCH_INTERVAL")
                .unwrap_or("30".to_string())
                .parse()?,
            game_type: env::var("GAME_TYPE").expect("GAME_TYPE not set").parse()?,
            enable_game_resolution: env::var("ENABLE_GAME_RESOLUTION")
                .unwrap_or("true".to_string())
                .parse()?,
            max_games_to_check_for_resolution: env::var("MAX_GAMES_TO_CHECK_FOR_RESOLUTION")
                .unwrap_or("100".to_string())
                .parse()?,
        })
    }
}
