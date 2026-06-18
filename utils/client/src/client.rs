use alloy_consensus::BlockBody;
use alloy_primitives::B256;
use alloy_rlp::Decodable;
use anyhow::Result;
use kona_derive::{Pipeline, PipelineError, PipelineErrorKind, Signal, SignalReceiver};
use kona_driver::{Driver, DriverError, DriverPipeline, DriverResult, Executor, TipCursor};
use kona_genesis::RollupConfig;
use kona_preimage::{CommsClient, PreimageKey};
use kona_proof::{errors::OracleProviderError, HintType};
use kona_protocol::L2BlockInfo;
use op_alloy_consensus::{OpBlock, OpTxEnvelope, OpTxType};
use std::fmt::Debug;
use tracing::{error, info, warn};

/// Fetches the safe head hash of the L2 chain based on the agreed upon L2 output root in the
/// [BootInfo].
pub(crate) async fn fetch_safe_head_hash<O>(
    caching_oracle: &O,
    agreed_l2_output_root: B256,
) -> Result<B256, OracleProviderError>
where
    O: CommsClient,
{
    let mut output_preimage = [0u8; 128];
    HintType::StartingL2Output
        .with_data(&[agreed_l2_output_root.as_ref()])
        .send(caching_oracle)
        .await?;
    caching_oracle
        .get_exact(PreimageKey::new_keccak256(*agreed_l2_output_root), output_preimage.as_mut())
        .await?;

    output_preimage[96..128].try_into().map_err(OracleProviderError::SliceConversion)
}

// Sourced from the `kona-driver` crate's `Driver::advance_to_target`
// (`kona-client/v1.2.14`, `crates/proof/driver/src/core.rs`), reimplemented as a free function
// over `&mut Driver` with three op-succinct-specific deltas:
//
//   - SP1 zkVM cycle-tracker instrumentation: under `#[cfg(target_os = "zkvm")]`, the loop emits
//     `cycle-tracker-report-{start,end}: payload-derivation` around `produce_payload` and
//     `cycle-tracker-report-{start,end}: block-execution` around `execute_payload`. SP1 sums the
//     cycles between matching markers across every iteration; those per-phase totals are read back
//     by key ("payload-derivation", "block-execution") in `utils/host/src/stats.rs` and
//     `validity/src/types.rs`. Because the markers bracket individual phases inside a single loop
//     iteration, they cannot be reconstructed by wrapping a call to the upstream method.
//   - `#[cfg(target_os = "zkvm")] std::mem::forget(block)` at the end of each iteration skips the
//     per-block `Drop` to save proven cycles (guest memory is reclaimed when the proof ends).
//   - Upstream's `self.safe_head_artifacts = Some((outcome, txs))` bookkeeping is omitted: it is
//     never read in op-succinct, so we skip retaining it and move `outcome.header` into the cursor
//     instead of cloning.
/// Advances the derivation pipeline to the target block number.
///
/// ## Takes
/// - `cfg`: The rollup configuration.
/// - `target`: The target block number.
///
/// ## Returns
/// - `Ok((l2_safe_head, output_root))` - A tuple containing the [L2BlockInfo] of the produced block
///   and the output root.
/// - `Err(e)` - An error if the block could not be produced.
pub async fn advance_to_target<E, DP, P>(
    driver: &mut Driver<E, DP, P>,
    cfg: &RollupConfig,
    mut target: Option<u64>,
) -> DriverResult<(L2BlockInfo, B256), E::Error>
where
    E: Executor + Send + Sync + Debug,
    DP: DriverPipeline<P> + Send + Sync + Debug,
    P: Pipeline + SignalReceiver + Send + Sync + Debug,
{
    loop {
        // Check if we have reached the target block number.
        let pipeline_cursor = driver.cursor.read();
        let tip_cursor = pipeline_cursor.tip();
        if let Some(tb) = target {
            if tip_cursor.l2_safe_head.block_info.number >= tb {
                info!(target: "client", "Derivation complete, reached L2 safe head.");
                return Ok((tip_cursor.l2_safe_head, tip_cursor.l2_safe_head_output_root));
            }
        }

        #[cfg(target_os = "zkvm")]
        println!("cycle-tracker-report-start: payload-derivation");
        let mut attributes = match driver.pipeline.produce_payload(tip_cursor.l2_safe_head).await {
            Ok(attrs) => attrs.take_inner(),
            Err(PipelineErrorKind::Critical(PipelineError::EndOfSource)) => {
                warn!(target: "client", "Exhausted data source; Halting derivation and using current safe head.");

                // Adjust the target block number to the current safe head, as no more blocks
                // can be produced.
                if target.is_some() {
                    target = Some(tip_cursor.l2_safe_head.block_info.number);
                };

                // If we are in interop mode, this error must be handled by the caller.
                // Otherwise, we continue the loop to halt derivation on the next iteration.
                if cfg.is_interop_active(driver.cursor.read().l2_safe_head().block_info.number) {
                    return Err(PipelineError::EndOfSource.crit().into());
                } else {
                    continue;
                }
            }
            Err(e) => {
                error!(target: "client", "Failed to produce payload: {:?}", e);
                return Err(DriverError::Pipeline(e));
            }
        };
        #[cfg(target_os = "zkvm")]
        println!("cycle-tracker-report-end: payload-derivation");

        driver.executor.update_safe_head(tip_cursor.l2_safe_head_header.clone());

        #[cfg(target_os = "zkvm")]
        println!("cycle-tracker-report-start: block-execution");
        let outcome = match driver.executor.execute_payload(attributes.clone()).await {
            Ok(outcome) => outcome,
            Err(e) => {
                error!(target: "client", "Failed to execute L2 block: {}", e);

                if cfg.is_holocene_active(attributes.payload_attributes.timestamp) {
                    // Retry with a deposit-only block.
                    warn!(target: "client", "Flushing current channel and retrying deposit only block");

                    // Flush the current batch and channel - if a block was replaced with a
                    // deposit-only block due to execution failure, the
                    // batch and channel it is contained in is forwards
                    // invalidated.
                    driver.pipeline.signal(Signal::FlushChannel).await?;

                    // Strip out all transactions that are not deposits.
                    attributes.transactions = attributes.transactions.map(|txs| {
                        txs.into_iter()
                            .filter(|tx| !tx.is_empty() && tx[0] == OpTxType::Deposit as u8)
                            .collect::<Vec<_>>()
                    });

                    // Retry the execution.
                    driver.executor.update_safe_head(tip_cursor.l2_safe_head_header.clone());
                    match driver.executor.execute_payload(attributes.clone()).await {
                        Ok(header) => header,
                        Err(e) => {
                            error!(
                                target: "client",
                                "Critical - Failed to execute deposit-only block: {e}",
                            );
                            return Err(DriverError::Executor(e));
                        }
                    }
                } else {
                    // Pre-Holocene, discard the block if execution fails.
                    continue;
                }
            }
        };
        #[cfg(target_os = "zkvm")]
        println!("cycle-tracker-report-end: block-execution");

        // Construct the block.
        let block = OpBlock {
            header: outcome.header.inner().clone(),
            body: BlockBody {
                transactions: attributes
                    .transactions
                    .as_ref()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .map(|tx| OpTxEnvelope::decode(&mut tx.as_ref()).map_err(DriverError::Rlp))
                    .collect::<DriverResult<Vec<OpTxEnvelope>, E::Error>>()?,
                ommers: Vec::new(),
                withdrawals: None,
            },
        };

        // Get the pipeline origin and update the tip cursor.
        let origin = driver.pipeline.origin().ok_or(PipelineError::MissingOrigin.crit())?;
        let l2_info =
            L2BlockInfo::from_block_and_genesis(&block, &driver.pipeline.rollup_config().genesis)?;
        let tip_cursor = TipCursor::new(
            l2_info,
            outcome.header,
            driver.executor.compute_output_root().map_err(DriverError::Executor)?,
        );

        // Advance the derivation pipeline cursor
        drop(pipeline_cursor);
        driver.cursor.write().advance(origin, tip_cursor);

        // Add forget calls to save cycles
        #[cfg(target_os = "zkvm")]
        std::mem::forget(block);
    }
}
