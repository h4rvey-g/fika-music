import { computed, ref, shallowRef, type ComputedRef } from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";
import type { QueryClient, QueryKey } from "@tanstack/vue-query";
import { cancelSourceRequest } from "../lib/plugin-api";

type QrLoginStart = {
  sessionId: string;
};

type QrAccount = {
  accountRef: string;
  displayName: string;
};

type QrLoginPoll<Account extends QrAccount> = {
  status: "waitingForScan" | "waitingForConfirmation" | "expired" | "connected";
  account: Account | null;
};

type QrLoginOptions<Account extends QrAccount, Start extends QrLoginStart> = {
  providerName: string;
  start: () => Promise<Start>;
  poll: (sessionId: string) => Promise<QrLoginPoll<Account>>;
  cancel: (sessionId: string) => Promise<void>;
  onConnected: (account: Account) => Promise<void>;
  onError: (error: unknown) => void;
  pollIntervalMs?: number;
};

export function useQrLoginSession<Account extends QrAccount, Start extends QrLoginStart>(
  options: QrLoginOptions<Account, Start>,
) {
  const login = shallowRef<Start | null>(null);
  const status = ref("");
  const isConnecting = ref(false);
  const isPolling = ref(false);
  let pollTimer: ReturnType<typeof setTimeout> | null = null;

  async function start(): Promise<void> {
    if (isConnecting.value) return;
    cancel();
    isConnecting.value = true;
    status.value = "Waiting for scan";
    try {
      login.value = await options.start();
      schedulePoll();
    } catch (error) {
      options.onError(error);
      login.value = null;
      status.value = "";
    } finally {
      isConnecting.value = false;
    }
  }

  function schedulePoll(): void {
    stopPolling();
    pollTimer = setTimeout(
      () => void poll(),
      options.pollIntervalMs ?? 1_600,
    );
  }

  async function poll(): Promise<void> {
    const sessionId = login.value?.sessionId;
    if (!sessionId || isPolling.value) return;
    isPolling.value = true;
    try {
      const result = await options.poll(sessionId);
      if (login.value?.sessionId !== sessionId) return;
      if (result.status === "waitingForScan") {
        status.value = "Waiting for scan";
        schedulePoll();
        return;
      }
      if (result.status === "waitingForConfirmation") {
        status.value = `Confirm in ${options.providerName}`;
        schedulePoll();
        return;
      }
      if (result.status === "expired") {
        options.onError(
          new Error(`${options.providerName} login QR code expired. Start a new connection.`),
        );
        login.value = null;
        status.value = "";
        return;
      }
      if (result.account) {
        login.value = null;
        status.value = "";
        try {
          await options.onConnected(result.account);
        } catch (error) {
          options.onError(error);
        }
        return;
      }
      throw new Error(`${options.providerName} login completed without an account.`);
    } catch (error) {
      if (login.value?.sessionId === sessionId) {
        options.onError(error);
        cancel();
      }
    } finally {
      isPolling.value = false;
    }
  }

  function stopPolling(): void {
    if (pollTimer) {
      clearTimeout(pollTimer);
      pollTimer = null;
    }
  }

  function cancel(): void {
    const sessionId = login.value?.sessionId;
    stopPolling();
    login.value = null;
    status.value = "";
    if (sessionId) {
      void options.cancel(sessionId).catch(() => undefined);
    }
  }

  return { login, status, isConnecting, isPolling, start, cancel };
}

export function useSourcePlaybackRequest() {
  const activeTrackId = ref<string | null>(null);
  let activeRequestId: string | null = null;

  async function run<T>(
    trackId: string,
    operation: (requestId: string, isCurrent: () => boolean) => Promise<T>,
  ): Promise<T | undefined> {
    if (activeRequestId) return undefined;
    const requestId = crypto.randomUUID();
    activeRequestId = requestId;
    activeTrackId.value = trackId;
    const isCurrent = () => activeRequestId === requestId;
    try {
      try {
        const result = await operation(requestId, isCurrent);
        return isCurrent() ? result : undefined;
      } catch (error) {
        if (isCurrent()) throw error;
        return undefined;
      }
    } finally {
      if (isCurrent()) {
        activeRequestId = null;
        activeTrackId.value = null;
      }
    }
  }

  function abandon(): void {
    const requestId = activeRequestId;
    activeRequestId = null;
    activeTrackId.value = null;
    if (requestId) {
      void cancelSourceRequest(requestId).catch(() => undefined);
    }
  }

  return { activeTrackId, run, abandon };
}

export async function cancellableSourceQuery<T>(
  signal: AbortSignal,
  query: (requestId: string) => Promise<T>,
): Promise<T> {
  const requestId = crypto.randomUUID();
  const cancel = () => {
    void cancelSourceRequest(requestId).catch(() => undefined);
  };
  if (signal.aborted) {
    cancel();
    throw new DOMException("Source request cancelled", "AbortError");
  }
  signal.addEventListener("abort", cancel, { once: true });
  try {
    return await query(requestId);
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}

export type WorkspaceQueryScope = {
  queryKey: QueryKey;
  exact?: boolean;
};

export async function cancelWorkspaceQueries(
  queryClient: QueryClient,
  scopes: WorkspaceQueryScope[],
): Promise<void> {
  await Promise.all(
    scopes.map((scope) =>
      queryClient.cancelQueries({
        queryKey: scope.queryKey,
        exact: scope.exact,
      }),
    ),
  );
}

export function clearWorkspaceQueries(
  queryClient: QueryClient,
  scopes: WorkspaceQueryScope[],
): void {
  for (const scope of scopes) {
    queryClient.removeQueries({
      queryKey: scope.queryKey,
      exact: scope.exact,
    });
  }
}

type PlaylistDetail<Track> = {
  playlist: { id: string };
  tracks: Track[];
};

export function useVirtualPlaylist<Track extends { id: string }>(
  detail: ComputedRef<PlaylistDetail<Track> | null>,
  rowHeight = 48,
) {
  const viewport = ref<HTMLElement | null>(null);
  const virtualizer = useVirtualizer(
    computed(() => ({
      count: detail.value?.tracks.length ?? 0,
      getScrollElement: () => viewport.value,
      estimateSize: () => rowHeight,
      overscan: 12,
      getItemKey: (index: number) => {
        const current = detail.value;
        return `${current?.playlist.id ?? "playlist"}:${index}:${current?.tracks[index]?.id ?? "track"}`;
      },
    })),
  );
  const rows = computed(() => {
    const tracks = detail.value?.tracks ?? [];
    return virtualizer.value
      .getVirtualItems()
      .map((virtual) => ({ virtual, track: tracks[virtual.index] }));
  });
  const topPadding = computed(() => rows.value[0]?.virtual.start ?? 0);
  const bottomPadding = computed(() => {
    const lastRow = rows.value[rows.value.length - 1];
    return Math.max(0, virtualizer.value.getTotalSize() - (lastRow?.virtual.end ?? 0));
  });

  function resetAndMeasure(): void {
    if (viewport.value) viewport.value.scrollTop = 0;
    virtualizer.value.measure();
  }

  function setViewport(element: unknown): void {
    viewport.value = element instanceof HTMLElement ? element : null;
  }

  return { viewport, rows, topPadding, bottomPadding, resetAndMeasure, setViewport };
}
