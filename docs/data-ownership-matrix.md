# Data Ownership Matrix

This document defines the **authoritative source of truth** for every field surfaced across the chain, indexer, API, and UI layers. Each field has exactly one authoritative layer. All other layers are derived views and must not be treated as canonical.

The goal is to prevent conflicting values from being displayed by different layers and to make freshness, fallback, and conflict behavior explicit and testable.

## Layers

| Layer | Description |
| --- | --- |
| **Chain** | On-chain state (contracts, events, storage). Ultimate source of truth for anything that settles on-chain. |
| **Indexer** | Off-chain derived store built from chain events. Authoritative only for data that cannot be read directly from chain or that requires aggregation. |
| **API** | Service layer that reads from chain/indexer and applies business logic. Authoritative only for API-owned computed fields. |
| **UI** | Presentation layer. Never authoritative for persisted data; authoritative only for ephemeral client state (e.g. unsaved form input). |

## Freshness contract legend

- **Realtime**: must reflect chain state within one block.
- **Near-realtime**: must reflect chain state within N blocks (documented per field).
- **Eventual**: may lag; bounded staleness documented per field.
- **Static**: changes only on explicit update.

## Field-level ownership matrix

### Balances

| Field | Authoritative layer | Freshness contract | Fallback behavior | Conflict resolution | Guarding tests |
| --- | --- | --- | --- | --- | --- |
| `balance.onChain` (token/account balance) | **Chain** | Realtime (read at request time) | If chain read fails, API returns last indexed value flagged `stale: true`; UI shows stale badge | Chain always wins over indexer/API cached value | `tests/contract/balances.contract.test.ts` |
| `balance.indexed` (indexer-maintained balance) | **Indexer** | Near-realtime (≤ 5 blocks) | If indexer lags beyond bound, API marks `stale: true` and surfaces chain value | Chain value overrides when discrepancy detected | `tests/contract/balances.contract.test.ts` |
| `balance.display` (formatted balance shown in UI) | **UI** (formatting only) | Derived from API response | Falls back to raw value if formatting fails | Must never diverge from API-provided numeric value; formatting only | `tests/contract/balances.contract.test.ts` |
| `balance.pending` (unconfirmed delta) | **Chain** (mempool/pending) | Realtime | Omitted if unavailable | Pending never overrides confirmed balance; shown separately | `tests/contract/balances.contract.test.ts` |

### Lifecycle

| Field | Authoritative layer | Freshness contract | Fallback behavior | Conflict resolution | Guarding tests |
| --- | --- | --- | --- | --- | --- |
| `lifecycle.state` (entity state machine) | **Chain** | Realtime | If chain unavailable, API returns last known state flagged `stale: true` | Chain state is canonical; indexer state is advisory only | `tests/contract/lifecycle.contract.test.ts` |
| `lifecycle.transitions` (allowed next states) | **API** (derived from chain state + rules) | Realtime (computed per request) | If rules unavailable, return empty transitions and disable actions | API-computed transitions must match chain state; mismatch is a bug | `tests/contract/lifecycle.contract.test.ts` |
| `lifecycle.history` (past transitions) | **Indexer** | Eventual (≤ 30 blocks) | If indexer unavailable, API returns empty history with `incomplete: true` | Indexer history must reconcile with chain events; chain wins | `tests/contract/lifecycle.contract.test.ts` |
| `lifecycle.displayLabel` (UI label) | **UI** (presentation) | Derived from API state | Falls back to raw state string | Must map 1:1 from API state; unknown states shown verbatim | `tests/contract/lifecycle.contract.test.ts` |

### Prices

| Field | Authoritative layer | Freshness contract | Fallback behavior | Conflict resolution | Guarding tests |
| --- | --- | --- | --- | --- | --- |
| `price.oracle` (on-chain oracle price) | **Chain** | Realtime | If oracle read fails, API returns last cached oracle price flagged `stale: true` | Chain oracle is canonical for settlement | `tests/contract/prices.contract.test.ts` |
| `price.market` (aggregated market price) | **Indexer** | Near-realtime (≤ 2 blocks) | If indexer unavailable, API falls back to oracle price flagged `source: "oracle"` | Oracle wins for settlement; market price is display-only | `tests/contract/prices.contract.test.ts` |
| `price.display` (UI price) | **UI** (formatting only) | Derived from API response | Falls back to raw value | Must reflect API-provided price and `source`; never recomputed client-side | `tests/contract/prices.contract.test.ts` |
| `price.staleness` (age indicator) | **API** | Realtime (computed per request) | Defaults to `stale: true` if age unknown | API-computed age is canonical | `tests/contract/prices.contract.test.ts` |

### Profiles

| Field | Authoritative layer | Freshness contract | Fallback behavior | Conflict resolution | Guarding tests |
| --- | --- | --- | --- | --- | --- |
| `profile.identity` (address/ID) | **Chain** | Static (immutable) | None; required | Chain identity is canonical | `tests/contract/profiles.contract.test.ts` |
| `profile.metadata` (name, avatar, bio) | **API** (off-chain store) | Eventual (≤ 60s) | If API unavailable, UI shows placeholder with identity only | API metadata wins; chain has no authority here | `tests/contract/profiles.contract.test.ts` |
| `profile.verification` (verified badge) | **API** | Eventual (≤ 60s) | Defaults to `unverified` if unknown | API is canonical; UI must not infer verification | `tests/contract/profiles.contract.test.ts` |
| `profile.display` (UI rendering) | **UI** (presentation) | Derived from API response | Falls back to truncated identity | Must not fabricate metadata; show placeholders for missing fields | `tests/contract/profiles.contract.test.ts` |

### Analytics

| Field | Authoritative layer | Freshness contract | Fallback behavior | Conflict resolution | Guarding tests |
| --- | --- | --- | --- | --- | --- |
| `analytics.aggregates` (volume, counts) | **Indexer** | Eventual (≤ 100 blocks) | If indexer unavailable, API returns `unavailable: true`; UI hides charts | Indexer is canonical for aggregates; chain used only to reconcile | `tests/contract/analytics.contract.test.ts` |
| `analytics.timeSeries` | **Indexer** | Eventual (≤ 100 blocks) | Returns empty series with `incomplete: true` | Indexer canonical; gaps flagged, never interpolated silently | `tests/contract/analytics.contract.test.ts` |
| `analytics.derived` (ratios, growth) | **API** | Eventual (computed from indexer) | Omitted if inputs incomplete | API canonical; must not compute on incomplete inputs | `tests/contract/analytics.contract.test.ts` |
| `analytics.display` (UI charts) | **UI** (presentation) | Derived from API response | Shows "data unavailable" state | Must render API values as-is; no client-side recomputation | `tests/contract/analytics.contract.test.ts` |

## Fallback and conflict behavior (summary)

1. **Chain is always canonical** for anything that settles on-chain (balances, lifecycle state, oracle prices, identity). If a derived layer disagrees with chain, the derived layer is wrong.
2. **Derived layers must flag staleness.** When a layer cannot reach its authoritative source within the freshness bound, it must set `stale: true` (or `incomplete: true` / `unavailable: true`) rather than silently serving old data.
3. **UI never recomputes authoritative values.** The UI may format, label, and lay out values, but must render API-provided numeric/state values verbatim. Client-side recomputation of balances, prices, lifecycle state, or analytics is prohibited.
4. **Conflict resolution order**: Chain > Indexer > API-derived > UI. A lower layer must never override a higher layer's value for the same field.
5. **Missing data is explicit.** Layers must return explicit "unknown/unavailable" markers rather than defaulting to zero, empty, or fabricated values.

## Review of representative API and UI fields

The following representative fields were reviewed against this matrix to confirm each has a single authoritative layer, a documented freshness contract, and a guarding contract test:

| Representative field | Layer reviewed | Matrix entry | Result |
| --- | --- | --- | --- |
| `balance.onChain` | API response | Balances | Matches: chain-authoritative, realtime, stale-flagged fallback |
| `balance.display` | UI | Balances | Matches: formatting-only, no recomputation |
| `lifecycle.state` | API response | Lifecycle | Matches: chain-authoritative, realtime |
| `lifecycle.displayLabel` | UI | Lifecycle | Matches: 1:1 mapping from API state |
| `price.oracle` | API response | Prices | Matches: chain-authoritative for settlement |
| `price.display` | UI | Prices | Matches: reflects API `source`, no client recompute |
| `profile.metadata` | API response | Profiles | Matches: API-authoritative, eventual |
| `profile.display` | UI | Profiles | Matches: placeholders for missing fields |
| `analytics.aggregates` | API response | Analytics | Matches: indexer-authoritative, unavailable-flagged |
| `analytics.display` | UI | Analytics | Matches: renders API values as-is |

No mismatches were found between the reviewed fields and the matrix. Any future mismatch discovered during review must be resolved by either correcting the implementation to match the matrix or updating the matrix with justification, and a contract test must be added or updated to guard the corrected behavior.

## Where contract tests live

Contract tests that guard high-risk fields live under `tests/contract/`:

- `tests/contract/balances.contract.test.ts` — balances ownership, staleness flags, and UI formatting-only behavior.
- `tests/contract/lifecycle.contract.test.ts` — lifecycle state authority, transition derivation, and history reconciliation.
- `tests/contract/prices.contract.test.ts` — oracle vs. market price authority, source attribution, and staleness.
- `tests/contract/profiles.contract.test.ts` — identity vs. metadata authority and verification defaults.
- `tests/contract/analytics.contract.test.ts` — aggregate authority, incomplete-data flags, and no client recomputation.

Each contract test asserts that the field's authoritative layer wins in conflict scenarios, that fallback behavior sets the documented flags, and that lower layers do not override higher layers.