"use client";

import useSWR from "swr";
import { fetchProfile, profileUrl, type UserProfile } from "@/lib/api/profile";

export interface UseProfileResult {
  profile: UserProfile | undefined;
  isLoading: boolean;
  isError: boolean;
  /** True when serving cached profile data despite a failing request. */
  isStale: boolean;
  error: Error | undefined;
  refresh: () => void;
}

/**
 * useProfile — SWR-cached access to a user's full profile: aggregate stats,
 * per-pool claim status, and the daily activity series used for charts.
 *
 * The cache key includes both the address and the network so a profile
 * fetched on one network is never reused after a network switch. While the
 * identity is transitioning (no address), no data is returned to avoid
 * flashing the previous user's profile.
 *
 * Data ownership: the API is the authoritative source for profile fields
 * (aggregate stats, claim status, activity series). When a request fails but
 * cached data exists, the hook surfaces the cached profile and flags
 * `isStale` so callers can render the documented fallback.
 */
export function useProfile(
  address: string | undefined,
  network: string | undefined,
): UseProfileResult {
  const key = address && network ? profileUrl(address, network) : null;

  const { data, error, isLoading, mutate } = useSWR<UserProfile>(key, fetchProfile);

  return {
    profile: key ? data : undefined,
    isLoading,
    isError: Boolean(error),
    isStale: Boolean(error && data),
    error: error as Error | undefined,
    refresh: () => {
      void mutate();
    },
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
// These tests verify the hook surfaces the authoritative profile fields and
// applies the documented fallback (serve cached data, flag `isStale`) when a
// request fails. They are guarded so they only register under a test runner
// that provides the expected globals, keeping production bundles unaffected.

if (typeof describe === "function" && typeof it === "function") {
  /* eslint-disable @typescript-eslint/no-var-requires */
  const { renderHook, waitFor } = require("@testing-library/react") as typeof import("@testing-library/react");
  const { SWRConfig } = require("swr") as typeof import("swr");
  const React = require("react") as typeof import("react");

  const authoritativeProfile: UserProfile = {
    address: "0xabc",
    network: "mainnet",
    stats: {
      totalClaimed: "1000",
      totalAllocated: "2000",
      pools: 3,
    },
    pools: [
      { poolId: "1", claimable: "100", claimed: "50", status: "active" },
    ],
    activity: [
      { date: "2024-01-01", claimed: "10" },
    ],
  } as unknown as UserProfile;

  const wrapper = ({ children }: { children: React.ReactNode }) =>
    React.createElement(
      SWRConfig,
      { value: { provider: () => new Map(), dedupingInterval: 0 } },
      children,
    );

  describe("useProfile", () => {
    it("surfaces the authoritative profile fields from the API", async () => {
      const fetcher = jest.fn().mockResolvedValue(authoritativeProfile);
      jest.mock("@/lib/api/profile", () => ({
        fetchProfile: fetcher,
        profileUrl: (a: string, n: string) => `/api/profile/${n}/${a}`,
      }));

      const { result } = renderHook(() => useProfile("0xabc", "mainnet"), {
        wrapper,
      });

      await waitFor(() => expect(result.current.profile).toBeDefined());

      expect(result.current.profile).toEqual(authoritativeProfile);
      expect(result.current.profile?.stats).toEqual(authoritativeProfile.stats);
      expect(result.current.profile?.pools).toEqual(authoritativeProfile.pools);
      expect(result.current.profile?.activity).toEqual(
        authoritativeProfile.activity,
      );
      expect(result.current.isError).toBe(false);
      expect(result.current.isStale).toBe(false);
    });

    it("applies the documented fallback when data is stale", async () => {
      const fetcher = jest
        .fn()
        .mockResolvedValueOnce(authoritativeProfile)
        .mockRejectedValueOnce(new Error("network down"));

      jest.mock("@/lib/api/profile", () => ({
        fetchProfile: fetcher,
        profileUrl: (a: string, n: string) => `/api/profile/${n}/${a}`,
      }));

      const { result } = renderHook(() => useProfile("0xabc", "mainnet"), {
        wrapper,
      });

      await waitFor(() => expect(result.current.profile).toBeDefined());

      // Trigger a revalidation that fails; cached data must still be served.
      result.current.refresh();

      await waitFor(() => expect(result.current.isError).toBe(true));

      expect(result.current.profile).toEqual(authoritativeProfile);
      expect(result.current.isStale).toBe(true);
      expect(result.current.error).toBeInstanceOf(Error);
    });

    it("returns no profile while the identity is transitioning", () => {
      const { result } = renderHook(() => useProfile(undefined, "mainnet"), {
        wrapper,
      });

      expect(result.current.profile).toBeUndefined();
      expect(result.current.isLoading).toBe(false);
    });
  });
  /* eslint-enable @typescript-eslint/no-var-requires */
}