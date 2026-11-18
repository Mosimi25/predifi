# Runbook Tabletop Exercise

Use this to check that an operator can resolve an incident **using only the
linked runbook**. Run it when a runbook is added or substantially changed, and
at least once a quarter for each owner group.

## Roles

- **Facilitator:** knows the scenario, reveals one clue at a time, does not help.
- **Responder:** an on-call engineer who did not write the runbook. Has only
  the alert payload and the links in it.
- **Scribe:** records the timeline and every point where the responder needed
  something the runbook did not provide.

## Procedure (45 minutes)

1. Facilitator reads the alert exactly as it would arrive: `summary`,
   `description`, `impact`, `severity`, `owner`, `runbook_url`, `dashboard_url`.
2. Responder opens the runbook from `runbook_url` and narrates each step.
3. Facilitator answers each query or command with the scenario's prepared
   output. Anything the runbook does not mention is not answered.
4. Stop when the responder reaches **Verification** and confirms recovery, or
   when they would escalate.
5. Debrief: list every gap. Fix them in the runbook in the same week.

**Pass criteria:** The responder identifies user impact, finds the cause,
applies a mitigation the runbook marks as safe, verifies recovery, and knows
the rollback and escalation path, without outside help.

## Scenario 1 — Pool exhaustion after a migration

- **Alert:** `DatabaseConnectionPoolCritical` (then `HighApiErrorRate`).
- **Setup:** A release ran a migration with a plain `CREATE INDEX` on
  `predictions`; writes queue behind the `ShareLock`.
- **Clues to prepare:** `pg_stat_activity` showing one `CREATE INDEX` and many
  `INSERT INTO predictions` waiting on `Lock`; `_sqlx_migrations` latest row
  with a recent `installed_on`.
- **Expected path:** [database.md](database.md#databaseconnectionpoolcritical)
  → find blocker → cancel it → follow the migration's recovery steps →
  verify utilization and 5xx rate recover → escalate to infra-oncall only if
  RDS itself is unhealthy.

## Scenario 2 — RPC throttling

- **Alert:** `WorkerQueueBacklog`, then `IndexerFreshnessFastBurn`.
- **Setup:** The Soroban RPC provider returns HTTP 429 to the indexer.
- **Clues to prepare:** Worker logs with `429 Too Many Requests` and backoff;
  `/api/v1/health` RPC check degraded; database healthy.
- **Expected path:** [worker.md](worker.md#workerqueuebacklog) → identify RPC
  throttling → switch to backup RPC → verify backlog drains → restore original
  URL when the provider recovers.

## Scenario 3 — Bad deploy

- **Alert:** `ApiAvailabilityFastBurn` and `BackendPanic`.
- **Setup:** A release panics on `GET /api/v1/pools/:id` for pools without tags.
- **Clues to prepare:** Panic log with file/line; 5xx concentrated on one path;
  deploy 20 minutes ago.
- **Expected path:** [slo.md](slo.md#apiavailabilityfastburn) →
  [api.md](api.md#highapierrorrate) → roll back after confirming schema
  compatibility → verify → file a bug.

## Scenario 4 — Compromised notification provider credential

- **Alert:** `NotificationProviderAuthFailure` (or a security report of a
  leaked provider API key).
- **Setup:** The notification provider API key used by the delivery worker is
  believed compromised. The provider is a **test/mock provider** so the
  exercise can be run end to end without touching production credentials.
- **Clues to prepare:** Worker logs with `401 Unauthorized` from the provider;
  the key's last-used timestamp; the audit/outbox entries for the last
  successful rotation.
- **Expected path:** [notifications.md](notifications.md#compromised-provider-credential)
  → detect → revoke the old key at the provider → issue a replacement →
  deploy the new key → verify delivery → confirm the revoked key can no longer
  deliver → confirm the rotation event is in the audit log/outbox.

### Credential-compromise recovery steps

1. **Detect.** Confirm the credential is compromised (security report, leaked
   key, or repeated provider `401`/`403` with a valid-looking key). Record the
   time and the key identifier (never the secret) in the incident channel.
2. **Revoke.** Disable/delete the old key at the provider console first. This
   stops continued use immediately; expect delivery failures until the
   replacement is deployed.
3. **Replace.** Issue a new key with the minimum required scope. Store it in
   the secret manager, not in the repo or the ticket.
4. **Deploy.** Roll out the new key to the delivery worker via the normal
   config/secret rollout. Restart workers so no process keeps the old key in
   memory.
5. **Verify.** Send a test notification through the worker and confirm it is
   delivered. Then attempt delivery with the **revoked** key and confirm the
   provider rejects it.
6. **Audit.** Confirm the rotation event (who, when, which key id, old key
   revoked, new key deployed) is recorded in the audit log/outbox. If the
   provider supports it, also record the provider-side revocation.

**Dual-key transition:** Only use an overlap window (old + new key both valid)
where the provider's semantics permit it — i.e. the provider supports multiple
active keys and lets you revoke one without invalidating the other. If the
provider only allows a single active key, revoke-then-replace is mandatory and
a short delivery gap is expected and acceptable. Never leave the old key valid
after verification.

**Pass criteria for this scenario:** The responder revokes before replacing,
deploys the new key, verifies delivery, proves the revoked key cannot deliver,
and shows the rotation event in the audit log/outbox.

## Exercise record

| Field | Value |
|---|---|
| Date | |
| Scenario | |
| Facilitator / responder / scribe | |
| Alert received → runbook opened | |
| Cause identified at | |
| Mitigation applied at | |
| Recovery verified at | |
| Passed without outside help? | yes / no |
| Runbook gaps found | |
| Follow-up PRs | |

For Scenario 4, also record: old key revoked at, new key deployed at, revoked
key rejected by provider (yes/no), rotation event in audit log/outbox (yes/no).
