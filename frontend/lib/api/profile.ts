/**
 * User profile API client.
 *
 * Thin, typed wrapper around `GET /api/v1/users/:address/profile` — the
 * aggregated payload behind the profile page (prediction history, win/loss
 * stats, total earnings, active positions, and claim status).
 *
 * Data ownership (see docs/data-ownership-matrix.md):
 * - `stats.*` (balances, lifecycle, analytics) are owned by the indexer and
 *   served by the API. The UI must treat them as read-only projections and
 *   never recompute them client-side.
 * - `claims[].claimed` / `claimed_amount` are owned by the chain (claim
 *   registry). The API mirrors them; the UI must not optimistically flip
 *   `claimed` without a refetch.
 * - `claims[].pool_state` / `pool_result` are owned by the chain (pool
 *   contract) and mirrored by the indexer.
 * - `performance[]` is an indexer-owned analytics rollup; it is advisory and
 *   may lag behind `stats`.
 *
 * Freshness / fallback contract:
 * - The API is the single source of truth for the profile page. There is no
 *   client-side fallback to chain reads; on failure the UI surfaces an error
 *   state rather than stale or synthesized values.
 * - Malformed payloads are rejected at the boundary (see type guards) so the
 *   UI never renders partially-valid data.
 */

import { API_BASE_URL, ApiError } from "@/lib/api/pools";

export interface ProfileStats {
  total_predictions: number;
  wins: number;
  losses: number;
  pending: number;
  /** Win rate as a percentage (0-100) of settled predictions. */
  win_rate: number;
  total_staked: number;
  total_earnings: number;
  active_positions: number;
}

export interface ClaimStatus {
  prediction_id: number;
  pool_id: number;
  pool_name: string;
  outcome: number;
  amount: number;
  pool_state: string;
  pool_result: string | null;
  is_winner: boolean | null;
  claimed: boolean;
  claimed_amount: number;
  claim_window_expires_at: string | null;
  claim_expired: boolean;
}

export interface PerformancePoint {
  day: string;
  staked: number;
  earnings: number;
  predictions: number;
}

export interface UserProfile {
  address: string;
  stats: ProfileStats;
  claims: ClaimStatus[];
  performance: PerformancePoint[];
}

/** Type guard to validate ProfileStats shape. */
function isProfileStats(obj: unknown): obj is ProfileStats {
  if (!obj || typeof obj !== "object") return false;
  const stats = obj as Record<string, unknown>;
  return (
    typeof stats.total_predictions === "number" &&
    typeof stats.wins === "number" &&
    typeof stats.losses === "number" &&
    typeof stats.pending === "number" &&
    typeof stats.win_rate === "number" &&
    typeof stats.total_staked === "number" &&
    typeof stats.total_earnings === "number" &&
    typeof stats.active_positions === "number"
  );
}

/** Type guard to validate ClaimStatus shape. */
function isClaimStatus(obj: unknown): obj is ClaimStatus {
  if (!obj || typeof obj !== "object") return false;
  const claim = obj as Record<string, unknown>;
  return (
    typeof claim.prediction_id === "number" &&
    typeof claim.pool_id === "number" &&
    typeof claim.pool_name === "string" &&
    typeof claim.outcome === "number" &&
    typeof claim.amount === "number" &&
    typeof claim.pool_state === "string" &&
    (claim.pool_result === null || typeof claim.pool_result === "string") &&
    (claim.is_winner === null || typeof claim.is_winner === "boolean") &&
    typeof claim.claimed === "boolean" &&
    typeof claim.claimed_amount === "number" &&
    (claim.claim_window_expires_at === null || typeof claim.claim_window_expires_at === "string") &&
    typeof claim.claim_expired === "boolean"
  );
}

/** Type guard to validate PerformancePoint shape. */
function isPerformancePoint(obj: unknown): obj is PerformancePoint {
  if (!obj || typeof obj !== "object") return false;
  const point = obj as Record<string, unknown>;
  return (
    typeof point.day === "string" &&
    typeof point.staked === "number" &&
    typeof point.earnings === "number" &&
    typeof point.predictions === "number"
  );
}

/** Type guard to validate UserProfile shape. */
function isUserProfile(obj: unknown): obj is UserProfile {
  if (!obj || typeof obj !== "object") return false;
  const profile = obj as Record<string, unknown>;
  return (
    typeof profile.address === "string" &&
    isProfileStats(profile.stats) &&
    Array.isArray(profile.claims) &&
    profile.claims.every(isClaimStatus) &&
    Array.isArray(profile.performance) &&
    profile.performance.every(isPerformancePoint)
  );
}

export function profileUrl(address: string): string {
  return `${API_BASE_URL}/api/v1/users/${encodeURIComponent(address)}/profile`;
}

export async function fetchProfile(url: string): Promise<UserProfile> {
  const res = await fetch(url, { headers: { Accept: "application/json" } });

  if (!res.ok) {
    throw new ApiError(`Failed to load profile (HTTP ${res.status})`, res.status);
  }

  const body = await res.json();

  // Handle both wrapped and unwrapped responses
  const profile =
    body && typeof body === "object" && "data" in body && body.data ? body.data : body;

  // Validate response shape at boundary
  if (!isUserProfile(profile)) {
    throw new ApiError("Invalid profile response shape", 500);
  }

  return profile;
}