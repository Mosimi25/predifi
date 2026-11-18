//! Idempotent projection of on-chain contract events (#45).
//!
//! Every indexed event carries an [`EventKey`] — network, contract, ledger,
//! transaction hash, and the event's position within that transaction — which
//! uniquely identifies it on-chain. Projecting an event first claims that key
//! in `projected_contract_events` inside the same transaction as the domain
//! writes; a key that is already present means the event was projected before
//! and the whole projection is skipped. Retries, listener restarts, and
//! overlapping ledger windows therefore never create duplicate pools,
//! predictions, stake increments, or notifications.

use sqlx::{PgPool, Postgres, Transaction};
use tracing::instrument;

use crate::notifications::{enqueue_notification, NotifType};

use super::{
    insert_pool_from_event_tx, insert_prediction_from_event, resolve_pool_in_db, PoolCreatedEvent,
    PredictionPlacedEvent,
};

/// Canonical on-chain identity of a contract event.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventKey {
    /// Network passphrase or short network name (e.g. `testnet`).
    pub network: String,
    /// Emitting contract ID (`C...`).
    pub contract_id: String,
    pub ledger: u64,
    pub tx_hash: String,
    /// Position of the event within its transaction.
    pub event_index: u32,
}

impl EventKey {
    /// Single-string form, e.g. for logs and tracing fields.
    pub fn canonical(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.network, self.contract_id, self.ledger, self.tx_hash, self.event_index
        )
    }
}

/// Decoded contract event payloads that produce database projections.
#[derive(Debug, Clone)]
pub enum ProjectedEvent {
    PoolCreated(PoolCreatedEvent),
    PredictionPlaced(PredictionPlacedEvent),
    PoolResolved { pool_id: u64, winning_outcome: i32 },
}

impl ProjectedEvent {
    fn event_type(&self) -> &'static str {
        match self {
            ProjectedEvent::PoolCreated(_) => "pool_created",
            ProjectedEvent::PredictionPlaced(_) => "prediction_placed",
            ProjectedEvent::PoolResolved { .. } => "pool_resolved",
        }
    }
}

/// An event together with its deduplication key.
#[derive(Debug, Clone)]
pub struct ContractEvent {
    pub key: EventKey,
    pub event: ProjectedEvent,
}

/// Outcome of projecting a batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionSummary {
    pub applied: u64,
    pub duplicates: u64,
}

/// Claim `key` for projection. Returns `false` when it was already projected.
async fn claim_event_key(
    tx: &mut Transaction<'_, Postgres>,
    key: &EventKey,
    event_type: &str,
) -> Result<bool, sqlx::Error> {
    let claimed = sqlx::query(
        r#"
        INSERT INTO projected_contract_events
            (network, contract_id, ledger, tx_hash, event_index, event_type)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (network, contract_id, ledger, tx_hash, event_index) DO NOTHING
        "#,
    )
    .bind(&key.network)
    .bind(&key.contract_id)
    .bind(key.ledger as i64)
    .bind(&key.tx_hash)
    .bind(key.event_index as i32)
    .bind(event_type)
    .execute(&mut **tx)
    .await?;
    Ok(claimed.rows_affected() == 1)
}

/// Project one event inside the caller's transaction.
///
/// Returns `true` if the projection was applied, `false` if the event key had
/// already been projected (nothing is written).
#[instrument(skip(tx, event), name = "db.project_event_tx",
    fields(event_key = %event.key.canonical()))]
pub async fn project_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ContractEvent,
) -> Result<bool, sqlx::Error> {
    if !claim_event_key(tx, &event.key, event.event.event_type()).await? {
        return Ok(false);
    }

    match &event.event {
        ProjectedEvent::PoolCreated(e) => insert_pool_from_event_tx(tx, e).await?,
        ProjectedEvent::PredictionPlaced(e) => insert_prediction_from_event(tx, e).await?,
        ProjectedEvent::PoolResolved {
            pool_id,
            winning_outcome,
        } => {
            resolve_pool_in_db(&mut **tx, *pool_id, *winning_outcome).await?;
            let users: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT user_address FROM predictions WHERE pool_id = $1 ORDER BY 1",
            )
            .bind(*pool_id as i64)
            .fetch_all(&mut **tx)
            .await?;
            for user in &users {
                enqueue_notification(
                    tx,
                    user,
                    NotifType::PoolResolved,
                    "Pool resolved",
                    &format!("Pool #{pool_id} has been resolved. Check your result."),
                    *pool_id as i64,
                )
                .await?;
            }
        }
    }
    Ok(true)
}

/// Project a batch of events atomically. Safe to call repeatedly with the same
/// or overlapping batches: already-projected events are skipped.
#[instrument(skip(pool, events), name = "db.project_events", fields(count = events.len()))]
pub async fn project_events(
    pool: &PgPool,
    events: &[ContractEvent],
) -> Result<ProjectionSummary, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut summary = ProjectionSummary::default();
    for event in events {
        if project_event_tx(&mut tx, event).await? {
            summary.applied += 1;
        } else {
            summary.duplicates += 1;
        }
    }
    tx.commit().await?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_key_includes_every_component() {
        let key = EventKey {
            network: "testnet".into(),
            contract_id: "CABC".into(),
            ledger: 42,
            tx_hash: "deadbeef".into(),
            event_index: 3,
        };
        assert_eq!(key.canonical(), "testnet:CABC:42:deadbeef:3");
    }

    // --- Data ownership contract tests (#356) -------------------------------
    //
    // Ownership matrix (see docs/data-ownership.md):
    //   * `projected_contract_events` is the authoritative record of which
    //     chain events have been projected. The chain is the source of truth
    //     for event identity; the indexer never rewrites a claimed key.
    //   * Domain rows (pools, predictions, resolutions) are derived from the
    //     chain event and must reflect the chain values verbatim.
    //   * Conflict behavior: a duplicate event key is a no-op — the first
    //     projection wins and later conflicting payloads are ignored.

    fn sample_key(event_index: u32) -> EventKey {
        EventKey {
            network: "testnet".into(),
            contract_id: "CABC".into(),
            ledger: 42,
            tx_hash: "deadbeef".into(),
            event_index,
        }
    }

    #[test]
    fn event_key_identity_is_stable_across_retries() {
        // The same on-chain event must always produce the same canonical key,
        // so retries and overlapping ledger windows dedupe to one projection.
        let a = sample_key(3);
        let b = sample_key(3);
        assert_eq!(a, b);
        assert_eq!(a.canonical(), b.canonical());
    }

    #[test]
    fn distinct_event_positions_are_distinct_keys() {
        // Two events in the same tx are distinct on-chain facts and must not
        // collapse into one projection.
        assert_ne!(sample_key(0), sample_key(1));
        assert_ne!(sample_key(0).canonical(), sample_key(1).canonical());
    }

    #[test]
    fn event_type_labels_match_ownership_matrix() {
        // The persisted `event_type` column is part of the ownership contract;
        // these labels are consumed by analytics and must stay stable.
        let pool_created = ProjectedEvent::PoolCreated(PoolCreatedEvent {
            pool_id: 1,
            creator: "GABC".into(),
            question: "q".into(),
            options: vec!["a".into(), "b".into()],
            deadline: 0,
            created_at: 0,
        });
        assert_eq!(pool_created.event_type(), "pool_created");

        let resolved = ProjectedEvent::PoolResolved {
            pool_id: 1,
            winning_outcome: 0,
        };
        assert_eq!(resolved.event_type(), "pool_resolved");
    }

    #[test]
    fn projection_summary_defaults_to_zero() {
        // A batch with no events must report no applied/duplicate projections.
        let summary = ProjectionSummary::default();
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.duplicates, 0);
    }
}
