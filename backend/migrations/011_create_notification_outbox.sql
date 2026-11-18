-- Transactional outbox for user notifications.
--
-- Notification intents are written here in the same transaction as the
-- domain mutation that produced them. A worker then claims pending rows,
-- delivers them (inserting into `notifications`), and marks them sent.
-- Delivery failures are retried with exponential backoff until a terminal
-- failure state is reached.

CREATE TABLE IF NOT EXISTS notification_outbox (
    id                BIGSERIALL PRIMARY KEY,
    -- Delivery payload: the notification intent to persist into `notifications`.
    user_address      TEXT        NOT NULL,
    notif_type         TEXT        NOT NULL,
    title              TEXT        NOT NULL,
    message            TEXT        NOT NULL,
    pool_id            BIGINT      NOT NULL,
    -- Lifecycle state.
    status              TEXT        NOT NULL DEFAULT 'pending',
    attempts            INTEGER     NOT NULL DEFAULT 0,
    max_attempts        INTEGER     NOT NULL DEFAULT 5,
    next_attempt_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    last_error         TEXT,
    claimed_at         TIMESTAMPTZ,
    claimed_by         TEXT,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CONSTRAINT notification_outbox_status_check
        CHECK (status IN ('pending', 'claimed', 'sent', 'failed'))
);

-- Worker claiming scans for due pending rows; this index keeps that
-- scan cheap as the table grows.
CREATE INDEX IF NOT EXISTS idx_notification_outbox_due
    ON notification_outbox (next_attempt_at)
    WHERE status = 'pending';

-- Supports recovery of rows claimed by a worker that died before
-- completing delivery.
CREATE INDEX IF NOT EXISTS idx_notification_outbox_claimed
    ON notification_outbox (claimed_at)
    WHERE status = 'claimed';

-- Audit trail for notification provider credential rotations.
--
-- Each row records one rotation event for a provider credential: which
-- provider, the old and new key identifiers (never the secret material
-- itself), who performed it, why, when, and how it turned out. This is
-- the durable record used to prove that a compromised credential was
-- revoked and replaced, and to reconstruct the timeline during an
-- incident review or tabletop exercise.
CREATE TABLE IF NOT EXISTS notification_credential_rotations (
    id                  BIGSERIAL   PRIMARY KEY,
    -- Provider whose credential was rotated (e.g. 'email', 'sms', 'push').
    provider            TEXT        NOT NULL,
    -- Stable, non-secret identifiers for the credentials involved. These
    -- are fingerprints/labels, never the key material itself.
    old_key_id          TEXT,
    new_key_id          TEXT,
    -- Who triggered the rotation and the recorded justification.
    actor               TEXT        NOT NULL,
    reason              TEXT        NOT NULL,
    -- Transition mode: 'dual_key' when old and new keys were valid
    -- concurrently, 'direct' when the old key was revoked before the new
    -- key was activated.
    transition_mode     TEXT        NOT NULL DEFAULT 'direct',
    -- Lifecycle of the rotation event.
    status              TEXT        NOT NULL DEFAULT 'started',
    outcome             TEXT,
    -- Timestamps for each phase of the rotation.
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at          TIMESTAMPTZ,
    activated_at        TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT notification_credential_rotations_transition_mode_check
        CHECK (transition_mode IN ('dual_key', 'direct')),
    CONSTRAINT notification_credential_rotations_status_check
        CHECK (status IN ('started', 'revoked', 'activated', 'completed', 'failed')),
    CONSTRAINT notification_credential_rotations_outcome_check
        CHECK (outcome IS NULL OR outcome IN ('success', 'partial', 'failed', 'rolled_back'))
);

-- Audit queries are typically scoped to a provider and ordered by time.
CREATE INDEX IF NOT EXISTS idx_notification_credential_rotations_provider
    ON notification_credential_rotations (provider, started_at DESC);

-- Supports incident-wide timelines across all providers.
CREATE INDEX IF NOT EXISTS idx_notification_credential_rotations_started
    ON notification_credential_rotations (started_at DESC);

-- Supports finding in-flight rotations and their outcomes.
CREATE INDEX IF NOT EXISTS idx_notification_credential_rotations_status
    ON notification_credential_rotations (status)
    WHERE status IN ('started', 'revoked', 'activated');
