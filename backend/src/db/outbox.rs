//! Notification outbox domain types.
//!
//! A [`NotificationIntent`] is a pending user notification produced by a
//! domain event (pool resolved, prediction settled, referral paid…). It is
//! serialised to a JSON payload for delivery through the transactional
//! outbox implemented in [`crate::notifications`], which owns the
//! `notification_outbox` table, claiming, retries, and terminal failure
//! handling.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Outcome of a notification provider credential rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationOutcome {
    /// Rotation started but has not yet been verified.
    Pending,
    /// New credential verified and old credential revoked.
    Completed,
    /// Rotation failed; the previous credential may still be active.
    Failed,
    /// Rotation was rolled back to the previous credential.
    RolledBack,
}

/// Audit record for a notification provider credential rotation.
///
/// Recorded whenever a provider credential is rotated (for example after a
/// suspected compromise) so that detection, revocation, replacement,
/// deployment, and verification steps are traceable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialRotationAudit {
    /// Notification provider whose credential was rotated.
    pub provider: String,
    /// Identifier of the credential being replaced (never the secret itself).
    pub old_key_id: Option<String>,
    /// Identifier of the replacement credential (never the secret itself).
    pub new_key_id: Option<String>,
    /// Actor who initiated the rotation.
    pub actor: String,
    /// Reason for the rotation (e.g. `compromise_suspected`).
    pub reason: String,
    /// Unix timestamp (seconds) when the rotation event was recorded.
    pub timestamp: i64,
    /// Outcome of the rotation at the time of recording.
    pub outcome: RotationOutcome,
}

impl CredentialRotationAudit {
    /// Create a new audit record with a `Pending` outcome.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: impl Into<String>,
        old_key_id: Option<String>,
        new_key_id: Option<String>,
        actor: impl Into<String>,
        reason: impl Into<String>,
        timestamp: i64,
    ) -> Self {
        Self {
            provider: provider.into(),
            old_key_id,
            new_key_id,
            actor: actor.into(),
            reason: reason.into(),
            timestamp,
            outcome: RotationOutcome::Pending,
        }
    }

    /// Serialize the audit record to a JSON string for storage.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("credential rotation audit is serializable")
    }

    /// Deserialize an audit record from its stored JSON representation.
    pub fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }
}

/// A pending notification intent awaiting delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationIntent {
    /// Identifier of the user the notification is addressed to.
    pub user_id: String,
    /// Logical notification kind (e.g. `pool_resolved`).
    pub kind: String,
    /// Arbitrary JSON payload delivered alongside the notification.
    pub payload: Value,
    /// Delivery attempts so far.
    pub attempts: u32,
}

impl NotificationIntent {
    /// Create a new intent with zero delivery attempts.
    pub fn new(user_id: impl Into<String>, kind: impl Into<String>, payload: Value) -> Self {
        Self {
            user_id: user_id.into(),
            kind: kind.into(),
            payload,
            attempts: 0,
        }
    }

    /// Serialize the payload to a JSON string for storage in the outbox.
    pub fn payload_json(&self) -> String {
        self.payload.to_string()
    }
}

/// A notification intent claimed by a delivery worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedNotification {
    /// Outbox row identifier.
    pub id: i64,
    /// Identifier of the user the notification is addressed to.
    pub user_id: String,
    /// Logical notification kind.
    pub kind: String,
    /// Delivery payload.
    pub payload: Value,
    /// Delivery attempts made so far.
    pub attempts: u32,
    /// Maximum delivery attempts before terminal failure.
    pub max_attempts: u32,
}

impl ClaimedNotification {
    /// Whether another delivery attempt is still allowed.
    pub fn can_retry(&self) -> bool {
        self.attempts < self.max_attempts
    }

    /// Delivery attempts remaining before terminal failure.
    pub fn remaining_attempts(&self) -> u32 {
        self.max_attempts.saturating_sub(self.attempts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_defaults_to_zero_attempts() {
        let intent = NotificationIntent::new("GABC", "pool_resolved", serde_json::json!({}));
        assert_eq!(intent.attempts, 0);
        assert_eq!(intent.user_id, "GABC");
    }

    #[test]
    fn claimed_retry_budget_is_saturating() {
        let claimed = ClaimedNotification {
            id: 1,
            user_id: "GABC".into(),
            kind: "referral_paid".into(),
            payload: serde_json::json!({}),
            attempts: 5,
            max_attempts: 5,
        };
        assert!(!claimed.can_retry());
        assert_eq!(claimed.remaining_attempts(), 0);
    }

    #[test]
    fn claimed_retry_allowed_below_max() {
        let claimed = ClaimedNotification {
            id: 2,
            user_id: "GABC".into(),
            kind: "pool_resolved".into(),
            payload: serde_json::json!({"pool": 7}),
            attempts: 1,
            max_attempts: 3,
        };
        assert!(claimed.can_retry());
        assert_eq!(claimed.remaining_attempts(), 2);
    }

    #[test]
    fn rotation_audit_round_trips_through_json() {
        let audit = CredentialRotationAudit::new(
            "push",
            Some("key-old".into()),
            Some("key-new".into()),
            "ops@example.com",
            "compromise_suspected",
            1_700_000_000,
        );
        assert_eq!(audit.outcome, RotationOutcome::Pending);

        let raw = audit.to_json();
        let decoded = CredentialRotationAudit::from_json(&raw).expect("valid audit json");
        assert_eq!(decoded.provider, "push");
        assert_eq!(decoded.old_key_id.as_deref(), Some("key-old"));
        assert_eq!(decoded.new_key_id.as_deref(), Some("key-new"));
        assert_eq!(decoded.actor, "ops@example.com");
        assert_eq!(decoded.reason, "compromise_suspected");
        assert_eq!(decoded.timestamp, 1_700_000_000);
        assert_eq!(decoded.outcome, RotationOutcome::Pending);
    }

    #[test]
    fn rotation_audit_serializes_outcome_as_snake_case() {
        let mut audit = CredentialRotationAudit::new(
            "email",
            None,
            Some("key-new".into()),
            "ops@example.com",
            "scheduled_rotation",
            1_700_000_100,
        );
        audit.outcome = RotationOutcome::Completed;

        let raw = audit.to_json();
        assert!(raw.contains("\"outcome\":\"completed\""));
        assert!(raw.contains("\"old_key_id\":null"));
    }
}
