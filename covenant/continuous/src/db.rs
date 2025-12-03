use std::fmt::Display;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use alloy_consensus::{Block, BlockHeader};
use eyre::eyre;
use host_executor::ExecutionHooks;
use reth_primitives_traits::NodePrimitives;
use store::localdb::LocalDB;
use zkm_prover::ZKM_CIRCUIT_VERSION;
use zkm_sdk::{ExecutionReport, HashableKey, ZKMVerifyingKey};

lazy_static::lazy_static! {
    static ref LAST_REMOVED_NUMBER: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));
}

const PROOF_COUNT: u64 = 200;

#[derive(Clone)]
pub struct PersistToDB {
    pub local_db: LocalDB,
}

impl PersistToDB {
    pub async fn new(local_db: &LocalDB) -> Self {
        Self { local_db: local_db.clone() }
    }

    pub async fn get_last_number(&self) -> eyre::Result<Option<i64>> {
        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

        storage_process
            .get_last_continuous_number()
            .await
            .map_err(|e| eyre!("Failed to get last number: {e}"))
    }

    pub async fn set_block_proof_concurrency(&self, concurrency: u32) -> eyre::Result<()> {
        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

        storage_process
            .set_block_proof_concurrency(concurrency as i64)
            .await
            .map_err(|e| eyre!("Failed to start block execution: {e}"))?;
        Ok(())
    }
}

impl ExecutionHooks for PersistToDB {
    async fn on_execution_start(&self, block_number: u64) -> eyre::Result<()> {
        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

        storage_process
            .create_block_proving_task(block_number as i64, ProvableBlockStatus::Queued.to_string())
            .await
            .map_err(|e| eyre!("Failed to start block execution: {e}"))?;

        Ok(())
    }

    async fn on_execution_end<P: NodePrimitives>(
        &self,
        executed_block: &Block<P::SignedTx>,
        _execution_report: &ExecutionReport,
    ) -> eyre::Result<()> {
        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

        storage_process
            .update_block_executed(
                executed_block.number() as i64,
                executed_block.body.transactions.len() as i64,
                executed_block.header.gas_used() as i64,
                ProvableBlockStatus::Executed.to_string(),
            )
            .await
            .map_err(|e| eyre!("Failed to end block execution: {e}"))?;

        Ok(())
    }

    async fn on_proving_end(
        &self,
        block_number: u64,
        proof_bytes: &[u8],
        public_values_bytes: &[u8],
        zkm_version: &str,
        vk: &ZKMVerifyingKey,
        cycles: u64,
        proving_duration: Duration,
    ) -> eyre::Result<()> {
        assert_eq!(
            zkm_version,
            ZKM_CIRCUIT_VERSION,
            "{}",
            format_args!(
                "Ziren version mismatch, expected {}, actual {}",
                ZKM_CIRCUIT_VERSION, zkm_version,
            ),
        );

        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

        storage_process
            .update_block_proved(
                block_number as i64,
                (proving_duration.as_secs_f32() * 1000.0) as i64,
                cycles as i64,
                proof_bytes,
                public_values_bytes,
                vk.bytes32(),
                zkm_version,
                ProvableBlockStatus::Proved.to_string(),
            )
            .await
            .map_err(|e| eyre!("Failed to end block proving: {e}"))?;

        let vk_bytes = bincode::serialize(vk).unwrap();
        storage_process
            .create_verifier_key(vk.bytes32().as_ref(), vk_bytes.as_ref())
            .await
            .map_err(|e| eyre!("Failed to create vk: {e}"))?;

        #[cfg(feature = "test")]
        self.remove_old_proofs(block_number).await?;

        Ok(())
    }
}

impl PersistToDB {
    async fn remove_old_proofs(&self, block_number: u64) -> eyre::Result<()> {
        let last_removed_number = LAST_REMOVED_NUMBER.load(Ordering::Relaxed);
        tracing::info!("last removed number: {}", last_removed_number);

        if block_number < last_removed_number + PROOF_COUNT {
            return Ok(());
        }

        let mut storage_process =
            self.local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;
        let remove_number = (block_number - PROOF_COUNT) as i64;
        storage_process
            .delete_block_proofs(remove_number)
            .await
            .map_err(|e| eyre!("Failed to delete block proofs: {e}"))?;

        LAST_REMOVED_NUMBER.store(block_number, Ordering::Relaxed);
        Ok(())
    }
}

pub async fn task_failed(
    local_db: Arc<LocalDB>,
    block_number: u64,
    err: String,
) -> eyre::Result<()> {
    let mut storage_process =
        local_db.acquire().await.map_err(|e| eyre!("Failed to acquire local db: {e}"))?;

    storage_process
        .update_block_proving_failed(
            block_number as i64,
            ProvableBlockStatus::Failed.to_string(),
            err,
        )
        .await
        .map_err(|e| eyre!("Failed to mark task as failed: {e}"))?;

    Ok(())
}

#[derive(Debug)]
pub enum ProvableBlockStatus {
    Queued,
    Executed,
    Proved,
    Failed,
}

impl Display for ProvableBlockStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvableBlockStatus::Queued => write!(f, "queued"),
            ProvableBlockStatus::Executed => write!(f, "executed"),
            ProvableBlockStatus::Proved => write!(f, "proved"),
            ProvableBlockStatus::Failed => write!(f, "failed"),
        }
    }
}
