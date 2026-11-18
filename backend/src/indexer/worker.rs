use std::sync::Arc;

use redis::AsyncCommands;
use tracing::{debug, warn};

use crate::indexer::events::IndexedEvent;
use crate::indexer::state::IndexerState;
use crate::rpc::quota::{PriorityClass, RpcError, RpcQuotaManager};

/// TTL fallback bound (in seconds) for cached market views.
///
/// Even if an invalidation is missed (e.g. worker crash between DB commit and
/// cache delete), no cached market detail or list view will persist beyond this
/// bound. Keep this in sync with the TTL used when writing the cache entries.
pub const MARKET_CACHE_TTL_SECS: u64 = 300;

/// Cache key for a single market detail view.
fn market_detail_key(market_id: &str) -> String {
    format!("market:detail:{market_id}")
}

/// Cache keys for market list views that include the given market.
fn market_list_keys(market_id: &str) -> Vec<String> {
    vec![
        "market:list:all".to_string(),
        "market:list:active".to_string(),
        format!("market:list:by-id:{market_id}"),
    ]
}

/// Map an indexed event to the cache keys it invalidates.
///
/// - Prediction created: the market detail (aggregates/odds) and list views
///   (volume/activity) change.
/// - Market resolved / cancelled: detail and list views change (status).
/// - Market metadata updated: detail and list views change (title, tags, etc.).
fn affected_cache_keys(event: &IndexedEvent) -> Vec<String> {
    match event {
        IndexedEvent::PredictionCreated { market_id, .. }
        | IndexedEvent::MarketResolved { market_id, .. }
        | IndexedEvent::MarketCancelled { market_id, .. }
        | IndexedEvent::MarketMetadataUpdated { market_id, .. } => {
            let mut keys = vec![market_detail_key(market_id)];
            keys.extend(market_list_keys(market_id));
            keys
        }
    }
}

/// Invalidate the cache keys affected by an indexed event.
///
/// Deletion is idempotent: deleting a key that does not exist is a no-op, so
/// duplicate or replayed events are safe. Errors are logged but not propagated
/// so a transient Redis failure cannot block event processing; the TTL fallback
/// (`MARKET_CACHE_TTL_SECS`) bounds staleness in that case.
async fn invalidate_market_views(state: &IndexerState, event: &IndexedEvent) {
    let keys = affected_cache_keys(event);
    if keys.is_empty() {
        return;
    }

    let mut conn = match state.redis.get_async_connection().await {
        Ok(conn) => conn,
        Err(err) => {
            warn!(error = %err, "failed to acquire redis connection for cache invalidation");
            return;
        }
    };

    let deleted: Result<u64, _> = conn.del(keys.clone()).await;
    match deleted {
        Ok(count) => {
            debug!(keys = ?keys, deleted = count, "invalidated market view caches");
        }
        Err(err) => {
            warn!(error = %err, keys = ?keys, "failed to invalidate market view caches");
        }
    }
}

/// Network whose RPC budget governs indexing work for this event.
///
/// Indexing is background work, so it runs in the `Background` priority class:
/// user-facing transaction preparation in the `UserTx` class always wins when a
/// network's budget is contended.
fn event_network(event: &IndexedEvent) -> &'static str {
    match event {
        IndexedEvent::PredictionCreated { .. }
        | IndexedEvent::MarketResolved { .. }
        | IndexedEvent::MarketCancelled { .. }
        | IndexedEvent::MarketMetadataUpdated { .. } => "default",
    }
}

/// Fetch the latest ledger/block height for the event's network through the
/// quota/failover manager.
///
/// The manager enforces the per-network request budget, applies provider health
/// and fallback ordering, and bounds retries on throttling or provider outage.
/// Because the manager deduplicates in-flight submissions and only retries
/// idempotent reads, a throttled or failing provider cannot cause duplicate
/// transaction submission. Throttling and fallback are recorded as metrics by
/// the manager itself.
async fn fetch_network_head(
    rpc: &RpcQuotaManager,
    network: &str,
) -> Result<u64, RpcError> {
    rpc.request(network, PriorityClass::Background, |provider| async move {
        provider.get_latest_block_height().await
    })
    .await
}

/// Process a single indexed event: persist it, then invalidate affected caches.
///
/// RPC access is routed through the quota/failover manager so indexing respects
/// per-network budgets and priority classes. A bounded RPC failure is logged and
/// does not abort event processing: cache invalidation still runs so views stay
/// fresh, and the next poll retries the read.
pub async fn process_event(state: Arc<IndexerState>, event: IndexedEvent) -> anyhow::Result<()> {
    let network = event_network(&event);
    match fetch_network_head(&state.rpc, network).await {
        Ok(height) => {
            debug!(network, height, "indexer rpc head fetched via quota manager");
        }
        Err(err) => {
            warn!(
                network,
                error = %err,
                "indexer rpc request failed after bounded retries; continuing with cache invalidation"
            );
        }
    }

    state.persist_event(&event).await?;
    invalidate_market_views(&state, &event).await;
    Ok(())
}

/// Contract tests for indexer-derived fields.
///
/// Per the data ownership matrix, the indexer is the source of truth for
/// lifecycle state (market status transitions) and on-chain balances/volume
/// aggregates. These tests pin the freshness/fallback contract:
/// - every lifecycle/balance-affecting event invalidates the derived views,
/// - the TTL fallback bounds staleness when invalidation is missed,
/// - invalidation is idempotent so replayed events are safe.
#[cfg(test)]
mod contract_tests {
    use super::*;

    fn prediction_created(market_id: &str) -> IndexedEvent {
        IndexedEvent::PredictionCreated {
            market_id: market_id.to_string(),
            ..Default::default()
        }
    }

    fn market_resolved(market_id: &str) -> IndexedEvent {
        IndexedEvent::MarketResolved {
            market_id: market_id.to_string(),
            ..Default::default()
        }
    }

    fn market_cancelled(market_id: &str) -> IndexedEvent {
        IndexedEvent::MarketCancelled {
            market_id: market_id.to_string(),
            ..Default::default()
        }
    }

    fn market_metadata_updated(market_id: &str) -> IndexedEvent {
        IndexedEvent::MarketMetadataUpdated {
            market_id: market_id.to_string(),
            ..Default::default()
        }
    }

    /// Lifecycle events must invalidate both the detail view (status) and the
    /// list views (status/activity) so the indexer remains the source of truth.
    #[test]
    fn lifecycle_events_invalidate_detail_and_list_views() {
        for event in [
            market_resolved("m1"),
            market_cancelled("m1"),
            market_metadata_updated("m1"),
        ] {
            let keys = affected_cache_keys(&event);
            assert!(
                keys.contains(&market_detail_key("m1")),
                "lifecycle event must invalidate market detail view"
            );
            for list_key in market_list_keys("m1") {
                assert!(
                    keys.contains(&list_key),
                    "lifecycle event must invalidate list view {list_key}"
                );
            }
        }
    }

    /// Balance/volume aggregates change on new predictions, so the derived
    /// views must be invalidated to keep the indexer authoritative.
    #[test]
    fn balance_affecting_events_invalidate_aggregate_views() {
        let keys = affected_cache_keys(&prediction_created("m2"));
        assert!(keys.contains(&market_detail_key("m2")));
        assert!(keys.contains(&"market:list:all".to_string()));
        assert!(keys.contains(&"market:list:active".to_string()));
        assert!(keys.contains(&"market:list:by-id:m2".to_string()));
    }

    /// The TTL fallback must be a positive, bounded value so a missed
    /// invalidation cannot leave stale indexer-derived fields indefinitely.
    #[test]
    fn ttl_fallback_is_bounded() {
        assert!(MARKET_CACHE_TTL_SECS > 0, "TTL fallback must be positive");
        assert!(
            MARKET_CACHE_TTL_SECS <= 600,
            "TTL fallback must bound staleness to a small window"
        );
    }

    /// Invalidation must be idempotent: replaying the same event yields the
    /// same key set, so duplicate events cannot corrupt derived state.
    #[test]
    fn invalidation_is_idempotent_for_replayed_events() {
        let event = market_resolved("m3");
        let first = affected_cache_keys(&event);
        let second = affected_cache_keys(&event);
        assert_eq!(first, second);
    }

    /// Every indexed event must map to at least one invalidated key, otherwise
    /// indexer-derived fields could silently go stale.
    #[test]
    fn every_event_invalidates_some_view() {
        for event in [
            prediction_created("m4"),
            market_resolved("m4"),
            market_cancelled("m4"),
            market_metadata_updated("m4"),
        ] {
            assert!(
                !affected_cache_keys(&event).is_empty(),
                "every indexed event must invalidate at least one view"
            );
        }
    }
}
