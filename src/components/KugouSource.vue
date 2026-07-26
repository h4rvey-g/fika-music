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
  Power,
  QrCode,
  RefreshCw,
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
  KUGOU_PLUGIN_ID,
  cancelKugouQrLogin,
  disconnectKugouAccount,
  getKugouPlaylist,
  getKugouPlaylists,
  getKugouRecommendations,
  listKugouAccounts,
  pollKugouQrLogin,
  resolveKugouTrack,
  startKugouQrLogin,
  type KugouPlayback,
} from "../lib/kugou-api";

const props = defineProps<{
  streamQuality: SourceQuality;
  playbackSource: AudioSourceId;
  audioSources: AudioSourceOption[];
}>();

const emit = defineEmits<{
  playbackReady: [playback: KugouPlayback];
  "update:playbackSource": [source: AudioSourceId];
  openPlugins: [];
  openAudioSources: [];
}>();

type KugouTab = "recommendations" | "playlists";

const PLAYLIST_STALE_TIME_MS = 5 * 60 * 1_000;

const queryKeys = {
  plugins: ["plugins"] as const,
  accounts: ["kugou", "accounts"] as const,
  recommendations: (accountRef: string) =>
    ["kugou", "recommendations", accountRef] as const,
  playlists: (accountRef: string) => ["kugou", "playlists", accountRef] as const,
  playlist: (accountRef: string, playlistId: string) =>
    ["kugou", "playlist", accountRef, playlistId] as const,
  playlistsForAccount: (accountRef: string) =>
    ["kugou", "playlist", accountRef] as const,
};

const queryClient = useQueryClient();
const activeAccountRef = ref("");
const activeTab = ref<KugouTab>("recommendations");
const selectedPlaylistId = ref("");
const operationDiagnostics = ref<string[]>([]);
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
  queryFn: listKugouAccounts,
  staleTime: 0,
});

const plugin = computed(
  () => pluginsQuery.data.value?.find((record) => record.id === KUGOU_PLUGIN_ID) ?? null,
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
      getKugouRecommendations(queryKey[2], requestId),
    ),
}));

const playlistsQuery = useQuery(() => ({
  queryKey: queryKeys.playlists(activeAccountRef.value),
  enabled: workspaceEnabled.value,
  queryFn: ({ queryKey, signal }) =>
    cancellableSourceQuery(signal, (requestId) =>
      getKugouPlaylists(queryKey[2], requestId),
    ),
}));

const selectedPlaylistQuery = useQuery(() => ({
  queryKey: queryKeys.playlist(activeAccountRef.value, selectedPlaylistId.value),
  enabled: workspaceEnabled.value && Boolean(selectedPlaylistId.value),
  staleTime: PLAYLIST_STALE_TIME_MS,
  structuralSharing: false,
  refetchOnWindowFocus: false,
  queryFn: ({ queryKey, signal }) =>
    cancellableSourceQuery(signal, (requestId) =>
      getKugouPlaylist(queryKey[2], queryKey[3], requestId),
    ),
}));

const disconnectMutation = useMutation({
  mutationFn: (accountRef: string) => disconnectKugouAccount(accountRef),
});

const recommendations = computed(() => recommendationsQuery.data.value?.data ?? []);
const playlists = computed(() => playlistsQuery.data.value?.data ?? []);
const selectedPlaylist = computed(() => selectedPlaylistQuery.data.value?.data ?? null);
const playlistVirtual = useVirtualPlaylist(selectedPlaylist);
const virtualPlaylistRows = playlistVirtual.rows;
const playlistTopPadding = playlistVirtual.topPadding;
const playlistBottomPadding = playlistVirtual.bottomPadding;
const isLoading = computed(
  () =>
    pluginsQuery.isPending.value ||
    accountsQuery.isPending.value ||
    recommendationsQuery.isFetching.value ||
    playlistsQuery.isFetching.value ||
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
  providerName: "KuGou Music",
  start: startKugouQrLogin,
  poll: pollKugouQrLogin,
  cancel: cancelKugouQrLogin,
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
  if (tab === "playlists") {
    await nextTick();
    playlistVirtual.resetAndMeasure();
  }
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
  await Promise.all([recommendationsQuery.refetch(), playlistsQuery.refetch()]);
  if (selectedPlaylistId.value) {
    await selectedPlaylistQuery.refetch();
  }
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
      let audioSourceFailure: unknown;
      try {
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
      } catch (error) {
        if (!isCurrent()) return null;
        audioSourceFailure = error;
      }

      const account = activeAccount.value;
      if (!account) throw audioSourceFailure;
      const fallback = await resolveKugouTrack(
        track,
        props.streamQuality,
        account.accountRef,
        requestId,
      ).catch((error) => {
        throw new Error(
          `Audio Source failed: ${normalizeError(audioSourceFailure)} KuGou fallback failed: ${normalizeError(error)}`,
        );
      });
      if (!isCurrent()) return null;
      operationDiagnostics.value = [
        "Audio Source failed; using KuGou Music.",
        ...fallback.diagnostics.map((diagnostic) => diagnostic.message),
      ];
      return fallback;
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
    { queryKey: queryKeys.playlistsForAccount(accountRef) },
  ];
}

async function refreshAccountStatuses() {
  try {
    await accountsQuery.refetch();
  } catch {
    // Preserve the primary provider error.
  }
}

function formatDuration(seconds: number | null) {
  if (!seconds) {
    return "--:--";
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${Math.floor(seconds % 60).toString().padStart(2, "0")}`;
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
            <h2 class="text-base font-semibold">KuGou Music</h2>
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
          aria-label="KuGou playback source"
          title="Playback source"
          @change="selectPlaybackSource"
        >
          <option v-if="!audioSources.length" value="">No audio source</option>
          <option v-for="source in audioSources" :key="source.value" :value="source.value">
            {{ source.label }}
          </option>
        </select>
        <select
          v-if="accounts.length"
          v-model="activeAccountRef"
          class="select select-sm w-32 max-w-full sm:w-44"
          aria-label="KuGou account"
        >
          <option v-for="account in accounts" :key="account.accountRef" :value="account.accountRef">
            {{ account.displayName }}{{ account.status === "expired" ? " (expired)" : "" }}
          </option>
        </select>
        <button class="btn btn-sm" type="button" :disabled="!isPluginReady || isConnecting" @click="startQrLogin">
          <RefreshCw v-if="isConnecting" class="animate-spin" :size="16" aria-hidden="true" />
          <QrCode v-else :size="16" aria-hidden="true" />
          Connect
        </button>
        <button
          v-if="activeAccount"
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          aria-label="Disconnect KuGou account"
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
          aria-label="Refresh KuGou data"
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
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss KuGou error" @click="sourceError = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="sourceNotice" role="status" class="alert alert-success alert-soft m-4">
      <CircleCheck :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">{{ sourceNotice }}</span>
      <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Dismiss KuGou notice" @click="sourceNotice = null">
        <X :size="16" aria-hidden="true" />
      </button>
    </div>

    <div v-if="isPluginReady && !audioSources.length" role="status" class="alert alert-warning alert-soft m-4">
      <AlertCircle :size="18" aria-hidden="true" />
      <span class="min-w-0 flex-1">No enabled audio source is available.</span>
      <button class="btn btn-sm" type="button" @click="emit('openAudioSources')">Open Audio Sources</button>
    </div>

    <div v-if="!isPluginReady" class="flex flex-col gap-3 p-5 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex items-start gap-3">
        <Power class="mt-0.5 shrink-0 text-warning" :size="18" aria-hidden="true" />
        <div>
          <div class="text-sm font-medium">Plugin is disabled</div>
          <div class="mt-1 text-xs text-base-content/60">Enable the bundled Plugin to use KuGou.</div>
        </div>
      </div>
      <button class="btn btn-sm" type="button" @click="emit('openPlugins')">Open Plugins</button>
    </div>

    <div v-else-if="qrLogin" class="grid gap-5 border-b border-base-300 p-5 sm:grid-cols-[16rem_minmax(0,1fr)] sm:items-center">
      <img
        class="aspect-square w-full max-w-64 border border-base-300 bg-white p-2"
        :src="qrLogin.qrImageDataUrl"
        alt="KuGou login QR code"
      />
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-sm font-medium">
          <RefreshCw v-if="isPollingQr" class="animate-spin" :size="16" aria-hidden="true" />
          <Clock3 v-else :size="16" aria-hidden="true" />
          {{ qrStatus }}
        </div>
        <p class="mt-2 text-sm text-base-content/65">Scan with the KuGou Music mobile app.</p>
        <button class="btn btn-ghost btn-sm mt-4" type="button" @click="cancelQrLogin">
          <X :size="16" aria-hidden="true" />
          Cancel
        </button>
      </div>
    </div>

    <div v-else-if="!activeAccountRef" class="grid min-h-52 place-items-center p-8 text-center">
      <div>
        <UserRound class="mx-auto text-base-content/45" :size="30" aria-hidden="true" />
        <div class="mt-3 text-sm font-medium">No KuGou account connected</div>
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
      </div>

      <div v-if="isLoading && !recommendations.length && !playlists.length" class="flex min-h-52 items-center justify-center gap-2 text-sm text-base-content/60">
        <RefreshCw class="animate-spin" :size="16" aria-hidden="true" />
        Loading KuGou
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
              <th class="w-12"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="track in recommendations" :key="track.id">
              <td class="hidden sm:table-cell">
                <img v-if="track.coverUrl" class="size-9 object-cover" :src="track.coverUrl" alt="" loading="lazy" />
                <div v-else class="size-9 bg-base-200"></div>
              </td>
              <td class="min-w-40 max-w-80 font-medium sm:min-w-52">
                <div class="truncate" :title="track.title">{{ track.title }}</div>
                <div class="mt-0.5 truncate text-xs font-normal text-base-content/60 sm:hidden" :title="track.artist">{{ track.artist }}</div>
              </td>
              <td class="hidden max-w-64 truncate sm:table-cell" :title="track.artist">{{ track.artist }}</td>
              <td class="hidden max-w-64 truncate lg:table-cell" :title="track.album || undefined">{{ track.album || "-" }}</td>
              <td class="hidden text-right tabular-nums md:table-cell">{{ formatDuration(track.durationSeconds) }}</td>
              <td>
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
              </td>
            </tr>
          </tbody>
        </table>
        <div v-if="!recommendations.length" class="p-8 text-center text-sm text-base-content/60">No recommendations returned.</div>
      </div>

      <div v-else class="grid min-h-96 lg:grid-cols-[17rem_minmax(0,1fr)]">
        <aside class="border-b border-base-300 lg:border-b-0 lg:border-r">
          <ul class="menu w-full gap-1 p-2">
            <li v-for="playlist in playlists" :key="playlist.id">
              <button type="button" :class="{ 'menu-active': selectedPlaylistId === playlist.id }" @click="selectedPlaylistId = playlist.id">
                <span class="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
                  <img
                    v-if="playlist.coverUrl"
                    class="size-full object-cover"
                    :src="playlist.coverUrl"
                    alt=""
                    loading="lazy"
                    decoding="async"
                  />
                  <ListMusic v-else :size="18" aria-hidden="true" />
                </span>
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
          <div v-if="selectedPlaylist?.tracks.length" :ref="playlistVirtual.setViewport" class="h-[clamp(20rem,60vh,36rem)] overflow-auto">
            <table class="table table-sm" aria-label="KuGou Playlist tracks" :aria-rowcount="selectedPlaylist.tracks.length + 1">
              <thead class="sticky top-0 z-10 bg-base-100">
                <tr>
                  <th class="w-12"></th>
                  <th>Track</th>
                  <th class="hidden sm:table-cell">Artist</th>
                  <th class="hidden text-right md:table-cell">Time</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="playlistTopPadding" aria-hidden="true">
                  <td class="border-0 p-0" colspan="4" :style="{ height: `${playlistTopPadding}px` }"></td>
                </tr>
                <tr v-for="row in virtualPlaylistRows" :key="String(row.virtual.key)" class="h-12" :aria-rowindex="row.virtual.index + 2">
                  <td>
                    <button
                      class="btn btn-square btn-ghost btn-sm"
                      type="button"
                      :disabled="Boolean(isPlayingTrackId) || !playbackSource"
                      :aria-label="`Play ${row.track.title}`"
                      title="Play"
                      @click="playTrack(row.track)"
                    >
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
                </tr>
                <tr v-if="playlistBottomPadding" aria-hidden="true">
                  <td class="border-0 p-0" colspan="4" :style="{ height: `${playlistBottomPadding}px` }"></td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-if="selectedPlaylist && !selectedPlaylist.tracks.length" class="p-8 text-center text-sm text-base-content/60">This Playlist is empty.</div>
        </div>
      </div>

      <div v-if="diagnostics.length" role="status" class="border-t border-base-300 px-4 py-3 text-xs text-base-content/60">
        <div v-for="message in diagnostics" :key="message">{{ message }}</div>
      </div>
    </template>
  </section>
</template>
