import { useState, useEffect, useCallback } from "react";

export type AsyncState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "success"; data: T }
  | { status: "error"; error: string };

/**
 * Generic hook for calling an async API function.
 * Auto-fetches on mount and whenever `refreshKey` changes.
 * Returns `refetch` to manually trigger a reload.
 */
export function useApi<T>(
  fn: () => Promise<T>,
  deps: unknown[] = [],
): [AsyncState<T>, () => void] {
  const [state, setState] = useState<AsyncState<T>>({ status: "idle" });
  const [tick, setTick] = useState(0);

  const refetch = useCallback(() => setTick((t) => t + 1), []);

  useEffect(() => {
    let cancelled = false;
    setState({ status: "loading" });
    fn()
      .then((data) => {
        if (!cancelled) setState({ status: "success", data });
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setState({
            status: "error",
            error: err instanceof Error ? err.message : String(err),
          });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, ...deps]);

  return [state, refetch];
}

/** Convenience: auto-refresh every `intervalMs` milliseconds. */
export function usePolling<T>(
  fn: () => Promise<T>,
  intervalMs = 5000,
): [AsyncState<T>, () => void] {
  const [state, refetch] = useApi<T>(fn);

  useEffect(() => {
    const id = setInterval(refetch, intervalMs);
    return () => clearInterval(id);
  }, [refetch, intervalMs]);

  return [state, refetch];
}
