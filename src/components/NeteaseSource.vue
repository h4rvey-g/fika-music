<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import {
  AlertCircle,
  CircleCheck,
  Clock3,
  ListMusic,
  LogOut,
  Play,
  Plus,
  Power,
  QrCode,
  RefreshCw,
  Trash2,
  UserRound,
  X,
} from "@lucide/vue";
import {
  listPlugins,
  type RemoteTrack,
  type SourceQuality,
} from "../lib/plugin-api";
import {
  cancelWorkspaceQueries,
  cancellableSourceQuery,
  clearWorkspaceQueries,
  useQrLoginSession,
  useSourcePlaybackRequest,
  useVirtualPlaylist,
} from "../composables/source-workspace";
import { normalizeError, queryError } from "../lib/errors";
import {
  audioSourceLabel,
  resolveAudioSourceTrack,
  type AudioSourceId,
  type AudioSourceOption,
} from "../lib/audio-source-api";
import {
  NETEASE_PLUGIN_ID,
  addNeteasePlaylistTrack,
  cancelNeteaseQrLogin,
  disconnectNeteaseAccount,
  getNeteasePlaylist,
  getNeteasePlaylists,
  getNeteaseRecommendations,
  listNeteaseAccounts,
  listNeteaseMutationAudit,
  pollNeteaseQrLogin,
  removeNeteasePlaylistTrack,
  startNeteaseQrLogin,
  type NeteasePlayback,
} from "../lib/netease-api";

const props = defineProps<{
  streamQuality: SourceQuality;
  playbackSource: AudioSourceId;
  audioSources: AudioSourceOption[];
}>();

const emit = defineEmits<{
  playbackReady: [playback: NeteasePlayback];
  "update:playbackSource": [source: AudioSourceId];
  openPlugins: [];
  openAudioSources: [];
}>();

type NeteaseTab = "recommendations" | "playlists" | "audit";
type PendingMutation = {
  operation: "add" | "remove";
  track: RemoteTrack;
};
type PlaylistMutationVariables = PendingMutation & {
  accountRef: string;
  playlistId: string;
};

const PLAYLIST_STALE_TIME_MS = 5 * 60 * 1_000;

const queryKeys = {
  plugins: ["plugins"] as const,
  accounts: ["netease", "accounts"] as const,
  recommendations: (accountRef: string) =>
    ["netease", "recommendations", accountRef] as const,
  playlists: (accountRef: string) => ["netease", "playlists", accountRef] as const,
  audit: (accountRef: string) => ["netease", "audit", accountRef] as const,
  playlist: (accountRef: string, playlistId: string) =>
    ["netease", "playlist", accountRef, playlistId] as const,
  playlistsForAccount: (accountRef: string) =>
    ["netease", "playlist", accountRef] as const,
};

const queryClient = useQueryClient();
const activeAccountRef = ref("");
const activeTab = ref<NeteaseTab>("recommendations");
const selectedPlaylistId = ref("");
const operationDiagnostics = ref<string[]>([]);
const pendingMutation = ref<PendingMutation | null>(null);
const mutationPlaylistId = ref("");
const manualError = ref<string | null>(null);
const dismissedQueryError = ref("");
const sourceNotice = ref<string | null>(null);
const playbackRequest = useSourcePlaybackRequest();
const isPlayingTrackId = playbackRequest.activeTrackId;

const pluginsQuery = useQuery({
  queryKey: queryKeys.plugins,
  queryFn: listPlugins,
  staleTime: 0,
});

const accountsQuery = useQuery({
  queryKey: queryKeys.accounts,
  queryFn: listNeteaseAccounts,
  staleTime: 0,
});

const plugin = computed(
  () => pluginsQuery.data.value?.find((record) => record.id === NETEASE_PLUGIN_ID) ?? null,
);
const accounts = computed(() => accountsQuery.data.value ?? []);

const isPluginReady = computed(
  () => plugin.value?.enabled === true && plugin.value.state === "enabled",
);
const activeAccount = computed(
  () => accounts.value.find((account) => account.accountRef === activeAccountRef.value) ?? null,
);
const workspaceEnabled = computed(
  () => isPluginReady.value && Boolean(activeAccountRef.value),
);

const recommendationsQuery = useQuery(() => ({
  queryKey: queryKeys.recommendations(activeAccountRef.value),
  enabled: workspaceEnabled.value,
  queryFn: ({ queryKey, signal }) =>
    cancellableSourceQuery(signal, (requestId) =>
      getNeteaseRecommendations(queryKey[2], requestId),
    ),
}));

const playlistsQuery = useQuery(() => ({
  queryKey: queryKeys.playlists(activeAccountRef.value),
  enabled: workspaceEnabled.value,
  queryFn: ({ queryKey, signal }) =>
    cancellableSourceQuery(signal, (requestId) => getNeteasePlaylists(queryKey[2], requestId)),
}));

const auditQuery = useQuery(() => ({
  queryKey: queryKeys.audit(activeAccountRef.value),
  enabled: workspaceEnabled.value,
  queryFn: ({ queryKey }) => listNeteaseMutationAudit(queryKey[2]),
}));

const selectedPlaylistQuery = useQuery(() => ({
  queryKey: queryKeys.playlist(activeAccountRef.value, selectedPlaylistId.value),
  enabled: workspaceEnabled.value && Boolean(selectedPlaylistId.value),
  staleTime: PLAYLIST_STALE_TIME_MS,
  structuralSharing: false,
  refetchOnWindowFocus: false,
  queryFn: ({ queryKey, signal }) =>
    cancellableSourceQuery(signal, (requestId) =>
      getNeteasePlaylist(queryKey[2], queryKey[3], requestId),
    ),
}));

const disconnectMutation = useMutation({
  mutationFn: (accountRef: string) => disconnectNeteaseAccount(accountRef),
});

const playlistMutation = useMutation({
  mutationFn: ({ operation, accountRef, playlistId, track }: PlaylistMutationVariables) =>
    operation === "add"
      ? addNeteasePlaylistTrack(accountRef, playlistId, track)
      : removeNeteasePlaylistTrack(accountRef, playlistId, track),
});

const recommendations = computed(() => recommendationsQuery.data.value?.data ?? []);
const playlists = computed(() => playlistsQuery.data.value?.data ?? []);
const selectedPlaylist = computed(() => selectedPlaylistQuery.data.value?.data ?? null);
const playlistVirtual = useVirtualPlaylist(selectedPlaylist);
const virtualPlaylistRows = playlistVirtual.rows;
const playlistTopPadding = playlistVirtual.topPadding;
const playlistBottomPadding = playlistVirtual.bottomPadding;
const audit = computed(() => auditQuery.data.value ?? []);
const mutablePlaylists = computed(() => playlists.value.filter((playlist) => playlist.canMutate));
const mutationPlaylist = computed(
  () => playlists.value.find((playlist) => playlist.id === mutationPlaylistId.value) ?? null,
);
const isMutating = computed(() => playlistMutation.isPending.value);
const isLoading = computed(
  () =>
    pluginsQuery.isPending.value ||
    accountsQuery.isPending.value ||
    recommendationsQuery.isFetching.value ||
    playlistsQuery.isFetching.value ||
    auditQuery.isFetching.value ||
    selectedPlaylistQuery.isFetching.value ||
    disconnectMutation.isPending.value,
);

const diagnostics = computed(() => {
  const messages = [
    ...(recommendationsQuery.data.value?.diagnostics ?? []),
    ...(playlistsQuery.data.value?.diagnostics ?? []),
    ...(selectedPlaylistQuery.data.value?.diagnostics ?? []),
  ].map((diagnostic) => diagnostic.message);
  return [...new Set([...messages, ...operationDiagnostics.value])];
});

const queryErrorMessage = computed(() => {
  const errors = [
    queryError("Plugin", pluginsQuery.isError.value, pluginsQuery.error.value),
    queryError("Accounts", accountsQuery.isError.value, accountsQuery.error.value),
    queryError(
      "Recommendations",
      recommendationsQuery.isError.value,
      recommendationsQuery.error.value,
    ),
    queryError("Playlists", playlistsQuery.isError.value, playlistsQuery.error.value),
    queryError("Audit", auditQuery.isError.value, auditQuery.error.value),
    queryError(
      "Playlist",
      selectedPlaylistQuery.isError.value,
      selectedPlaylistQuery.error.value,
    ),
  ].filter((message): message is string => Boolean(message));
  return errors.join(" ");
});

const sourceError = computed<string | null>({
  get() {
    if (manualError.value) {
      return manualError.value;
    }
    const queryError = queryErrorMessage.value;
    return queryError && queryError !== dismissedQueryError.value ? queryError : null;
  },
  set(value) {
    if (value === null) {
      manualError.value = null;
      dismissedQueryError.value = queryErrorMessage.value;
    } else {
      manualError.value = value;
    }
  },
});

const qrSession = useQrLoginSession({
  providerName: "NetEase Cloud Music",
  start: startNeteaseQrLogin,
  poll: pollNeteaseQrLogin,
  cancel: cancelNeteaseQrLogin,
  onConnected: connectAccount,
  onError: (error) => {
    sourceError.value = normalizeError(error);
  },
});
const qrLogin = qrSession.login;
const qrStatus = qrSession.status;
const isConnecting = qrSession.isConnecting;
const isPollingQr = qrSession.isPolling;
const cancelQrLogin = qrSession.cancel;

watch(
  () => accountsQuery.data.value,
  (connectedAccounts) => {
    if (!connectedAccounts) {
      return;
    }
    if (connectedAccounts.some((account) => account.accountRef === activeAccountRef.value)) {
      return;
    }
    activeAccountRef.value =
      connectedAccounts.find((account) => account.status === "active")?.accountRef ??
      connectedAccounts[0]?.accountRef ??
      "";
  },
  { immediate: true },
);

watch(activeAccountRef, () => {
  playbackRequest.abandon();
  selectedPlaylistId.value = "";
  operationDiagnostics.value = [];
  pendingMutation.value = null;
  manualError.value = null;
  dismissedQueryError.value = "";
});

watch(
  playlists,
  (availablePlaylists) => {
    if (!availablePlaylists.some((playlist) => playlist.id === selectedPlaylistId.value)) {
      selectedPlaylistId.value = availablePlaylists[0]?.id ?? "";
    }
  },
  { immediate: true },
);

watch(selectedPlaylist, async () => {
  await nextTick();
  playlistVirtual.resetAndMeasure();
});

watch(activeTab, async (tab) => {
  if (tab !== "playlists") {
    return;
  }
  await nextTick();
  playlistVirtual.resetAndMeasure();
});

watch(queryErrorMessage, (message, previousMessage) => {
  if (message !== previousMessage) {
    dismissedQueryError.value = "";
  }
});

watch(
  () => [
    recommendationsQuery.errorUpdatedAt.value,
    playlistsQuery.errorUpdatedAt.value,
    auditQuery.errorUpdatedAt.value,
    selectedPlaylistQuery.errorUpdatedAt.value,
  ],
  (timestamps, previousTimestamps) => {
    const hasNewError = timestamps.some(
      (timestamp, index) => timestamp > (previousTimestamps?.[index] ?? 0),
    );
    if (hasNewError) {
      void refreshAccountStatuses();
    }
  },
);

onBeforeUnmount(() => {
  qrSession.cancel();
  playbackRequest.abandon();
});

async function refreshWorkspace() {
  if (!activeAccountRef.value || !isPluginReady.value) {
    return;
  }
  manualError.value = null;
  dismissedQueryError.value = "";
  sourceNotice.value = null;
  await Promise.all([
    recommendationsQuery.refetch(),
    playlistsQuery.refetch(),
    auditQuery.refetch(),
  ]);
  if (selectedPlaylistId.value) {
    await selectedPlaylistQuery.refetch();
  }
}

function selectAccount() {
  sourceNotice.value = null;
}

async function startQrLogin() {
  if (!isPluginReady.value || isConnecting.value) {
    return;
  }
  sourceError.value = null;
  sourceNotice.value = null;
  await qrSession.start();
}

async function connectAccount(account: { accountRef: string; displayName: string }) {
  await accountsQuery.refetch();
  const isCurrentAccount = activeAccountRef.value === account.accountRef;
  if (!isCurrentAccount) {
    clearAccountWorkspace(account.accountRef);
  }
  activeAccountRef.value = account.accountRef;
  await nextTick();
  if (isCurrentAccount) {
    await refreshWorkspace();
  }
  sourceNotice.value = `${account.displayName} connected.`;
}

async function disconnectAccount() {
  const account = activeAccount.value;
  if (!account || !window.confirm(`Disconnect ${account.displayName}?`)) {
    return;
  }
  manualError.value = null;
  try {
    await cancelAccountQueries(account.accountRef);
    await disconnectMutation.mutateAsync(account.accountRef);
    await accountsQuery.refetch();
    await nextTick();
    clearAccountWorkspace(account.accountRef);
    sourceNotice.value = `${account.displayName} disconnected.`;
  } catch (error) {
    manualError.value = normalizeError(error);
    await refreshAccountStatuses();
  }
}

async function playTrack(track: RemoteTrack) {
  if (isPlayingTrackId.value || !props.playbackSource) {
    return;
  }
  manualError.value = null;
  const playbackSource = props.playbackSource;
  try {
    const playback = await playbackRequest.run(track.id, async (requestId, isCurrent) => {
      const source = await resolveAudioSourceTrack({
        audioSourceId: playbackSource,
        source: track.source,
        trackId: track.id,
        musicInfo: {
          ...track.rawInfo,
          name: track.title,
          singer: track.artist,
          artist: track.artist,
          album: track.album,
        },
        quality: props.streamQuality,
        requestId,
      });
      if (!isCurrent()) return null;
      operationDiagnostics.value = source.diagnostics.map((diagnostic) => diagnostic.message);
      return {
        track,
        url: source.url,
        providerName: audioSourceLabel(playbackSource, props.audioSources),
        diagnostics: source.diagnostics,
      };
    });
    if (playback) emit("playbackReady", playback);
  } catch (error) {
    manualError.value = normalizeError(error);
  }
}

function selectPlaybackSource(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (props.audioSources.some((source) => source.value === value)) {
    emit("update:playbackSource", value);
  }
}

function requestAdd(track: RemoteTrack) {
  pendingMutation.value = { operation: "add", track };
  mutationPlaylistId.value =
    mutablePlaylists.value.find((playlist) => playlist.id === selectedPlaylistId.value)?.id ??
    mutablePlaylists.value[0]?.id ??
    "";
}

function requestRemove(track: RemoteTrack) {
  if (!selectedPlaylist.value?.playlist.canMutate) {
    return;
  }
  pendingMutation.value = { operation: "remove", track };
  mutationPlaylistId.value = selectedPlaylist.value.playlist.id;
}

function cancelMutation() {
  if (!isMutating.value) {
    pendingMutation.value = null;
  }
}

async function confirmMutation() {
  const mutation = pendingMutation.value;
  const accountRef = activeAccountRef.value;
  if (!mutation || !accountRef || !mutationPlaylistId.value) {
    return;
  }
  manualError.value = null;
  try {
    const playlistId = mutationPlaylistId.value;
    const playlistName = mutationPlaylist.value?.name ?? "Playlist";
    const result = await playlistMutation.mutateAsync({
      ...mutation,
      accountRef,
      playlistId,
    });
    if (mutation.operation === "add") {
      sourceNotice.value = `${mutation.track.title} added to ${playlistName}.`;
    } else {
      sourceNotice.value = `${mutation.track.title} removed from ${playlistName}.`;
    }
    operationDiagnostics.value = result.diagnostics.map((diagnostic) => diagnostic.message);
    pendingMutation.value = null;
    await invalidateMutationQueries(accountRef, playlistId);
  } catch (error) {
    manualError.value = normalizeError(error);
    await queryClient.invalidateQueries({
      queryKey: queryKeys.audit(accountRef),
      exact: true,
    });
    await refreshAccountStatuses();
  }
}

async function refreshAccountStatuses() {
  try {
    await accountsQuery.refetch();
  } catch {
    // Preserve the primary provider or mutation error.
  }
}

async function cancelAccountQueries(accountRef: string) {
  await cancelWorkspaceQueries(queryClient, accountQueryScopes(accountRef));
}

function clearAccountWorkspace(accountRef: string) {
  clearWorkspaceQueries(queryClient, accountQueryScopes(accountRef));
}

function accountQueryScopes(accountRef: string) {
  return [
    { queryKey: queryKeys.recommendations(accountRef), exact: true },
    { queryKey: queryKeys.playlists(accountRef), exact: true },
    { queryKey: queryKeys.audit(accountRef), exact: true },
    { queryKey: queryKeys.playlistsForAccount(accountRef) },
  ];
}

async function invalidateMutationQueries(accountRef: string, playlistId: string) {
  await Promise.all([
    queryClient.invalidateQueries({
      queryKey: queryKeys.playlists(accountRef),
      exact: true,
    }),
    queryClient.invalidateQueries({
      queryKey: queryKeys.playlist(accountRef, playlistId),
      exact: true,
    }),
    queryClient.invalidateQueries({
      queryKey: queryKeys.audit(accountRef),
      exact: true,
    }),
  ]);
}

function formatDuration(seconds: number | null) {
  if (!seconds) {
    return "--:--";
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
}

function formatTimestamp(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString();
}

</script>

<template>
  <section class="overflow-hidden rounded border border-base-300 bg-base-100">
    <header class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 2xl:flex-row 2xl:items-center 2xl:justify-between">
      <div class="flex min-w-0 items-center gap-3">
        <div class="flex size-10 shrink-0 items-center justify-center rounded bg-neutral text-neutral-content">
          <ListMusic :size="19" aria-hidden="true" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h2 class="text-base font-semibold">NetEase Cloud Music</h2>
            <span v-if="plugin" class="badge badge-sm" :class="isPluginReady ? 'badge-success' : 'badge-warning'">
              {{ isPluginReady ? "Ready" : "Plugin disabled" }}
            </span>
          </div>
          <p v-if="activeAccount" class="mt-0.5 truncate text-xs text-base-content/60">
            {{ activeAccount.displayName }} · {{ activeAccount.status }}
          </p>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <select
          :value="playbackSource"
          class="select select-sm w-32 max-w-full"
          :disabled="Boolean(isPlayingTrackId) || !audioSources.length"
          aria-label="NetEase playback source"
          title="Playback source"
          @change="selectPlaybackSource"
        >
          <option v-if="!audioSources.length" value="">No audio source</option>
          <option
            v-for="source in audioSources"
            :key="source.value"
            :value="source.value"
          >
            {{ source.label }}
          </option>
        </select>
        <select
          v-if="accounts.length"
          v-model="activeAccountRef"
          class="select select-sm w-32 max-w-full sm:w-44"
          aria-label="NetEase account"
          @change="selectAccount"
        >
          <option v-for="account in accounts" :key="account.accountRef" :value="account.accountRef">
            {{ account.displayName }}{{ account.status === "expired" ? " (expired)" : "" }}
          </option>
        </select>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!isPluginReady || isConnecting"
          @click="startQrLogin"
        >
          <RefreshCw v-if="isConnecting" class="animate-spin" :size="16" aria-hidden="true" />
          <QrCode v-else :size="16" aria-hidden="true" />
          Connect
        </button>
        <button
          v-if="activeAccount"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          aria-label="Disconnect NetEase account"
          title="Disconnect account"
          @click="disconnectAccount"
        >
          <LogOut :size="16" aria-hidden="true" />
        </button>
        <button
          v-if="isPluginReady && activeAccountRef"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :disabled="isLoading"
          aria-label="Refresh NetEase data"
          title="Refresh"
          @click="refreshWorkspace"
        >
          <RefreshCw :class="{ 'animate-spin': isLoading }" :size="16" aria-hidden="true" />
        </button>
      </div>
    </header>

    <div v-if="sourceError" role="alert" class="alert alert-error m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceError }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss NetEase error" @click="sourceError = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="sourceNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceNotice }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss NetEase notice" @click="sourceNotice = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div
      v-if="isPluginReady && !audioSources.length"
      role="status"
      class="alert alert-warning alert-soft m-4"
    >
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">No enabled audio source is available.</span>
      <button class="btn btn-sm" type="button" @click="emit('openAudioSources')">
        Open Audio Sources
      </button>
    </div>

    <div v-if="!isPluginReady" class="flex flex-col gap-3 p-5 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex items-start gap-3">
        <Power class="mt-0.5 shrink-0 text-warning" :size="18" aria-hidden="true" />
        <div>
          <div class="text-sm font-medium">Plugin is disabled</div>
          <div class="mt-1 text-xs text-base-content/60">Enable the bundled Plugin to use NetEase.</div>
        </div>
      </div>
      <button class="btn btn-sm" type="button" @click="emit('openPlugins')">Open Plugins</button>
    </div>

    <div v-else-if="qrLogin" class="grid gap-5 border-b border-base-300 p-5 sm:grid-cols-[16rem_minmax(0,1fr)] sm:items-center">
      <img
        class="aspect-square w-full max-w-64 border border-base-300 bg-white p-2"
        :src="qrLogin.qrImageDataUrl"
        alt="NetEase login QR code"
      />
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-sm font-medium">
          <RefreshCw v-if="isPollingQr" class="animate-spin" :size="16" aria-hidden="true" />
          <Clock3 v-else :size="16" aria-hidden="true" />
          {{ qrStatus }}
        </div>
        <p class="mt-2 text-sm text-base-content/65">Scan with the NetEase Cloud Music mobile app.</p>
        <button class="btn btn-ghost btn-sm mt-4" type="button" @click="cancelQrLogin">
          <X :size="16" aria-hidden="true" />
          Cancel
        </button>
      </div>
    </div>

    <div v-else-if="!activeAccountRef" class="grid min-h-52 place-items-center p-8 text-center">
      <div>
        <UserRound class="mx-auto text-base-content/45" :size="30" aria-hidden="true" />
        <div class="mt-3 text-sm font-medium">No NetEase account connected</div>
      </div>
    </div>

    <template v-else>
      <div role="tablist" class="tabs tabs-border border-b border-base-300 px-4 pt-1">
        <button type="button" role="tab" class="tab" :class="{ 'tab-active': activeTab === 'recommendations' }" :aria-selected="activeTab === 'recommendations'" @click="activeTab = 'recommendations'">
          Recommendations
        </button>
        <button type="button" role="tab" class="tab" :class="{ 'tab-active': activeTab === 'playlists' }" :aria-selected="activeTab === 'playlists'" @click="activeTab = 'playlists'">
          Playlists
        </button>
        <button type="button" role="tab" class="tab" :class="{ 'tab-active': activeTab === 'audit' }" :aria-selected="activeTab === 'audit'" @click="activeTab = 'audit'">
          Audit
        </button>
      </div>

      <div v-if="isLoading && !recommendations.length && !playlists.length" class="flex min-h-52 items-center justify-center gap-2 text-sm text-base-content/60">
        <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
        Loading NetEase
      </div>

      <div v-else-if="activeTab === 'recommendations'" class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th class="hidden w-12 sm:table-cell"></th>
              <th>Track</th>
              <th class="hidden sm:table-cell">Artist</th>
              <th class="hidden lg:table-cell">Album</th>
              <th class="hidden text-right md:table-cell">Time</th>
              <th class="w-24"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="track in recommendations" :key="track.id">
              <td class="hidden sm:table-cell">
                <img v-if="track.coverUrl" class="size-9 object-cover" :src="track.coverUrl" alt="" loading="lazy" />
                <div v-else class="size-9 bg-base-200"></div>
              </td>
              <td class="min-w-40 font-medium sm:min-w-52">
                {{ track.title }}
                <div class="mt-0.5 text-xs font-normal text-base-content/60 sm:hidden">{{ track.artist }}</div>
              </td>
              <td class="hidden sm:table-cell">{{ track.artist }}</td>
              <td class="hidden lg:table-cell">{{ track.album || "-" }}</td>
              <td class="hidden text-right tabular-nums md:table-cell">{{ formatDuration(track.durationSeconds) }}</td>
              <td>
                <div class="flex justify-end gap-1">
                  <button
                    class="btn btn-square btn-ghost btn-sm"
                    type="button"
                    :disabled="Boolean(isPlayingTrackId) || !playbackSource"
                    :aria-label="`Play ${track.title}`"
                    title="Play"
                    @click="playTrack(track)"
                  >
                    <RefreshCw v-if="isPlayingTrackId === track.id" class="animate-spin" :size="15" aria-hidden="true" />
                    <Play v-else :size="15" aria-hidden="true" />
                  </button>
                  <button
                    class="btn btn-square btn-ghost btn-sm"
                    type="button"
                    :disabled="!mutablePlaylists.length"
                    :aria-label="`Add ${track.title} to a Playlist`"
                    title="Add to Playlist"
                    @click="requestAdd(track)"
                  >
                    <Plus :size="15" aria-hidden="true" />
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <div v-if="!recommendations.length" class="p-8 text-center text-sm text-base-content/60">No recommendations returned.</div>
      </div>

      <div v-else-if="activeTab === 'playlists'" class="grid min-h-96 lg:grid-cols-[17rem_minmax(0,1fr)]">
        <aside class="border-b border-base-300 lg:border-b-0 lg:border-r">
          <ul class="menu w-full gap-1 p-2">
            <li v-for="playlist in playlists" :key="playlist.id">
              <button
                type="button"
                :class="{ 'menu-active': selectedPlaylistId === playlist.id }"
                @click="selectedPlaylistId = playlist.id"
              >
                <span class="min-w-0 flex-1 truncate text-left">{{ playlist.name }}</span>
                <span class="text-xs tabular-nums opacity-60">{{ playlist.trackCount }}</span>
              </button>
            </li>
          </ul>
          <div v-if="!playlists.length" class="p-5 text-sm text-base-content/60">No Playlists returned.</div>
        </aside>

        <div class="min-w-0">
          <div v-if="selectedPlaylist" class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
            <img v-if="selectedPlaylist.playlist.coverUrl" class="size-11 object-cover" :src="selectedPlaylist.playlist.coverUrl" alt="" />
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm font-semibold">{{ selectedPlaylist.playlist.name }}</div>
              <div class="text-xs text-base-content/60">{{ selectedPlaylist.playlist.ownerName }} · {{ selectedPlaylist.tracks.length }} tracks</div>
            </div>
            <span v-if="!selectedPlaylist.playlist.canMutate" class="badge badge-ghost badge-sm">Read only</span>
          </div>
          <div
            v-if="selectedPlaylist?.tracks.length"
            :ref="playlistVirtual.setViewport"
            class="h-[clamp(20rem,60vh,36rem)] overflow-auto"
          >
            <table
              class="table table-sm"
              aria-label="Playlist tracks"
              :aria-rowcount="selectedPlaylist.tracks.length + 1"
            >
              <thead class="sticky top-0 z-10 bg-base-100">
                <tr>
                  <th class="w-12"></th>
                  <th>Track</th>
                  <th class="hidden sm:table-cell">Artist</th>
                  <th class="hidden text-right md:table-cell">Time</th>
                  <th class="w-20"></th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="playlistTopPadding" aria-hidden="true">
                  <td class="border-0 p-0" colspan="5" :style="{ height: `${playlistTopPadding}px` }"></td>
                </tr>
                <tr
                  v-for="row in virtualPlaylistRows"
                  :key="String(row.virtual.key)"
                  class="h-12"
                  :aria-rowindex="row.virtual.index + 2"
                >
                  <td>
                    <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="Boolean(isPlayingTrackId) || !playbackSource" :aria-label="`Play ${row.track.title}`" title="Play" @click="playTrack(row.track)">
                      <RefreshCw v-if="isPlayingTrackId === row.track.id" class="animate-spin" :size="15" aria-hidden="true" />
                      <Play v-else :size="15" aria-hidden="true" />
                    </button>
                  </td>
                  <td class="min-w-40 max-w-80 font-medium sm:min-w-52">
                    <div class="truncate" :title="row.track.title">{{ row.track.title }}</div>
                    <div class="mt-0.5 truncate text-xs font-normal text-base-content/60 sm:hidden" :title="row.track.artist">{{ row.track.artist }}</div>
                  </td>
                  <td class="hidden max-w-64 truncate sm:table-cell" :title="row.track.artist">{{ row.track.artist }}</td>
                  <td class="hidden text-right tabular-nums md:table-cell">{{ formatDuration(row.track.durationSeconds) }}</td>
                  <td>
                    <button
                      v-if="selectedPlaylist.playlist.canMutate"
                      class="btn btn-square btn-ghost btn-sm text-error"
                      type="button"
                      :aria-label="`Remove ${row.track.title} from Playlist`"
                      title="Remove from Playlist"
                      @click="requestRemove(row.track)"
                    >
                      <Trash2 :size="15" aria-hidden="true" />
                    </button>
                  </td>
                </tr>
                <tr v-if="playlistBottomPadding" aria-hidden="true">
                  <td class="border-0 p-0" colspan="5" :style="{ height: `${playlistBottomPadding}px` }"></td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-if="selectedPlaylist && !selectedPlaylist.tracks.length" class="p-8 text-center text-sm text-base-content/60">This Playlist is empty.</div>
        </div>
      </div>

      <div v-else class="overflow-x-auto">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Time</th>
              <th>Operation</th>
              <th>Playlist</th>
              <th>Track</th>
              <th>Outcome</th>
              <th>Message</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="record in audit" :key="record.id">
              <td class="whitespace-nowrap text-xs">{{ formatTimestamp(record.occurredAt) }}</td>
              <td><span class="badge badge-outline badge-sm">{{ record.operation }}</span></td>
              <td class="font-mono text-xs">{{ record.playlistId }}</td>
              <td class="font-mono text-xs">{{ record.trackId }}</td>
              <td><span class="badge badge-sm" :class="record.outcome === 'succeeded' ? 'badge-success' : 'badge-error'">{{ record.outcome }}</span></td>
              <td class="max-w-72 truncate text-xs" :title="record.message || undefined">{{ record.message || "-" }}</td>
            </tr>
          </tbody>
        </table>
        <div v-if="!audit.length" class="p-8 text-center text-sm text-base-content/60">No Playlist mutations recorded.</div>
      </div>

      <div v-if="diagnostics.length" role="status" class="border-t border-base-300 px-4 py-3 text-xs text-base-content/60">
        <div v-for="message in diagnostics" :key="message">{{ message }}</div>
      </div>
    </template>

    <div v-if="pendingMutation" class="modal modal-open" role="dialog" tabindex="0" aria-modal="true" aria-labelledby="netease-mutation-title">
      <div class="modal-box max-w-md">
        <h3 id="netease-mutation-title" class="text-base font-semibold">
          {{ pendingMutation.operation === "add" ? "Add to Playlist" : "Remove from Playlist" }}
        </h3>
        <p class="mt-2 text-sm text-base-content/70">{{ pendingMutation.track.title }} · {{ pendingMutation.track.artist }}</p>
        <label v-if="pendingMutation.operation === 'add'" class="mt-4 block text-sm">
          <span class="mb-1 block text-xs text-base-content/60">Playlist</span>
          <select v-model="mutationPlaylistId" class="select select-sm w-full">
            <option v-for="playlist in mutablePlaylists" :key="playlist.id" :value="playlist.id">{{ playlist.name }}</option>
          </select>
        </label>
        <div v-else class="alert alert-warning alert-soft mt-4">
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="text-sm">This removes the track from {{ mutationPlaylist?.name }}.</span>
        </div>
        <div class="modal-action">
          <button class="btn btn-ghost btn-sm" type="button" :disabled="isMutating" @click="cancelMutation">Cancel</button>
          <button
            class="btn btn-sm"
            :class="pendingMutation.operation === 'remove' ? 'btn-error' : 'btn-primary'"
            type="button"
            :disabled="isMutating || !mutationPlaylistId"
            @click="confirmMutation"
          >
            <RefreshCw v-if="isMutating" class="animate-spin" :size="15" aria-hidden="true" />
            <Plus v-else-if="pendingMutation.operation === 'add'" :size="15" aria-hidden="true" />
            <Trash2 v-else :size="15" aria-hidden="true" />
            Confirm
          </button>
        </div>
      </div>
      <button class="modal-backdrop" type="button" aria-label="Close mutation confirmation" @click="cancelMutation"></button>
    </div>
  </section>
</template>
