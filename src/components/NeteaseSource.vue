<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  AlertCircle,
  CircleCheck,
  Clock3,
  ListMusic,
  LogOut,
  Play,
  Plus,
  QrCode,
  RefreshCw,
  ShieldAlert,
  Trash2,
  UserRound,
  X,
} from "@lucide/vue";
import { listPlugins, type PluginRecord, type RemoteTrack, type SourcePlaylist, type SourcePlaylistDetail, type SourceQuality } from "../lib/plugin-api";
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
  resolveNeteaseTrack,
  startNeteaseQrLogin,
  type NeteaseAccount,
  type NeteaseMutationAudit,
  type NeteasePlayback,
  type NeteaseQrLoginStart,
} from "../lib/netease-api";

const props = defineProps<{
  streamQuality: SourceQuality;
}>();

const emit = defineEmits<{
  playbackReady: [playback: NeteasePlayback];
  openPlugins: [];
}>();

type NeteaseTab = "recommendations" | "playlists" | "audit";
type PendingMutation = {
  operation: "add" | "remove";
  track: RemoteTrack;
};

const plugin = ref<PluginRecord | null>(null);
const accounts = ref<NeteaseAccount[]>([]);
const activeAccountRef = ref("");
const activeTab = ref<NeteaseTab>("recommendations");
const recommendations = ref<RemoteTrack[]>([]);
const playlists = ref<SourcePlaylist[]>([]);
const selectedPlaylistId = ref("");
const selectedPlaylist = ref<SourcePlaylistDetail | null>(null);
const audit = ref<NeteaseMutationAudit[]>([]);
const diagnostics = ref<string[]>([]);
const qrLogin = ref<NeteaseQrLoginStart | null>(null);
const qrStatus = ref("");
const pendingMutation = ref<PendingMutation | null>(null);
const mutationPlaylistId = ref("");
const sourceError = ref<string | null>(null);
const sourceNotice = ref<string | null>(null);
const isLoading = ref(false);
const isConnecting = ref(false);
const isPollingQr = ref(false);
const isPlayingTrackId = ref<string | null>(null);
const isMutating = ref(false);

let qrPollTimer: ReturnType<typeof setTimeout> | null = null;

const isPluginReady = computed(
  () => plugin.value?.enabled === true && plugin.value.state === "enabled",
);
const activeAccount = computed(
  () => accounts.value.find((account) => account.accountRef === activeAccountRef.value) ?? null,
);
const mutablePlaylists = computed(() => playlists.value.filter((playlist) => playlist.canMutate));
const mutationPlaylist = computed(
  () => playlists.value.find((playlist) => playlist.id === mutationPlaylistId.value) ?? null,
);

onMounted(() => {
  void loadInitialState();
});

onBeforeUnmount(() => {
  cancelQrLogin();
});

async function loadInitialState() {
  isLoading.value = true;
  sourceError.value = null;
  try {
    const [pluginRecords, connectedAccounts] = await Promise.all([
      listPlugins(),
      listNeteaseAccounts(),
    ]);
    plugin.value = pluginRecords.find((record) => record.id === NETEASE_PLUGIN_ID) ?? null;
    accounts.value = connectedAccounts;
    activeAccountRef.value =
      connectedAccounts.find((account) => account.status === "active")?.accountRef ??
      connectedAccounts[0]?.accountRef ??
      "";
    if (isPluginReady.value && activeAccountRef.value) {
      await loadWorkspace();
    }
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function refreshWorkspace() {
  if (!activeAccountRef.value || !isPluginReady.value) {
    return;
  }
  isLoading.value = true;
  sourceError.value = null;
  sourceNotice.value = null;
  try {
    await loadWorkspace();
  } catch (error) {
    sourceError.value = normalizeError(error);
  } finally {
    isLoading.value = false;
  }
}

async function loadWorkspace() {
  const accountRef = activeAccountRef.value;
  if (!accountRef) {
    return;
  }
  const requestId = crypto.randomUUID();
  const [recommendationResult, playlistResult, auditResult] = await Promise.allSettled([
    getNeteaseRecommendations(accountRef, requestId),
    getNeteasePlaylists(accountRef),
    listNeteaseMutationAudit(accountRef),
  ]);
  const errors: string[] = [];
  const messages: string[] = [];

  if (recommendationResult.status === "fulfilled") {
    recommendations.value = recommendationResult.value.data;
    messages.push(...recommendationResult.value.diagnostics.map((item) => item.message));
  } else {
    recommendations.value = [];
    errors.push(`Recommendations: ${normalizeError(recommendationResult.reason)}`);
  }

  if (playlistResult.status === "fulfilled") {
    playlists.value = playlistResult.value.data;
    messages.push(...playlistResult.value.diagnostics.map((item) => item.message));
  } else {
    playlists.value = [];
    selectedPlaylistId.value = "";
    selectedPlaylist.value = null;
    errors.push(`Playlists: ${normalizeError(playlistResult.reason)}`);
  }

  if (auditResult.status === "fulfilled") {
    audit.value = auditResult.value;
  } else {
    audit.value = [];
    errors.push(`Audit: ${normalizeError(auditResult.reason)}`);
  }

  diagnostics.value = messages;
  if (errors.length) {
    sourceError.value = errors.join(" ");
    await refreshAccountStatuses();
  }

  const selectedStillExists = playlists.value.some(
    (playlist) => playlist.id === selectedPlaylistId.value,
  );
  if (!selectedStillExists) {
    selectedPlaylistId.value = playlists.value[0]?.id ?? "";
  }
  if (selectedPlaylistId.value) {
    await loadSelectedPlaylist();
  } else {
    selectedPlaylist.value = null;
  }
}

async function selectAccount() {
  selectedPlaylistId.value = "";
  selectedPlaylist.value = null;
  await refreshWorkspace();
}

async function loadSelectedPlaylist() {
  if (!activeAccountRef.value || !selectedPlaylistId.value) {
    selectedPlaylist.value = null;
    return;
  }
  try {
    const result = await getNeteasePlaylist(
      activeAccountRef.value,
      selectedPlaylistId.value,
      crypto.randomUUID(),
    );
    selectedPlaylist.value = result.data;
    diagnostics.value = result.diagnostics.map((diagnostic) => diagnostic.message);
  } catch (error) {
    sourceError.value = normalizeError(error);
    await refreshAccountStatuses();
  }
}

async function startQrLogin() {
  if (!isPluginReady.value || isConnecting.value) {
    return;
  }
  cancelQrLogin();
  isConnecting.value = true;
  sourceError.value = null;
  sourceNotice.value = null;
  qrStatus.value = "Waiting for scan";
  try {
    qrLogin.value = await startNeteaseQrLogin();
    scheduleQrPoll();
  } catch (error) {
    sourceError.value = normalizeError(error);
    qrLogin.value = null;
  } finally {
    isConnecting.value = false;
  }
}

function scheduleQrPoll() {
  stopQrPolling();
  qrPollTimer = setTimeout(() => void pollQrLogin(), 1600);
}

async function pollQrLogin() {
  const sessionId = qrLogin.value?.sessionId;
  if (!sessionId || isPollingQr.value) {
    return;
  }
  isPollingQr.value = true;
  try {
    const result = await pollNeteaseQrLogin(sessionId);
    if (qrLogin.value?.sessionId !== sessionId) {
      return;
    }
    if (result.status === "waitingForScan") {
      qrStatus.value = "Waiting for scan";
      scheduleQrPoll();
      return;
    }
    if (result.status === "waitingForConfirmation") {
      qrStatus.value = "Confirm in NetEase Cloud Music";
      scheduleQrPoll();
      return;
    }
    if (result.status === "expired") {
      sourceError.value = "NetEase login QR code expired. Start a new connection.";
      qrLogin.value = null;
      qrStatus.value = "";
      return;
    }
    if (result.account) {
      qrLogin.value = null;
      qrStatus.value = "";
      accounts.value = await listNeteaseAccounts();
      activeAccountRef.value = result.account.accountRef;
      sourceNotice.value = `${result.account.displayName} connected.`;
      await refreshWorkspace();
    }
  } catch (error) {
    if (qrLogin.value?.sessionId === sessionId) {
      sourceError.value = normalizeError(error);
      cancelQrLogin();
    }
  } finally {
    isPollingQr.value = false;
  }
}

function stopQrPolling() {
  if (qrPollTimer) {
    clearTimeout(qrPollTimer);
    qrPollTimer = null;
  }
}

function cancelQrLogin() {
  const sessionId = qrLogin.value?.sessionId;
  stopQrPolling();
  qrLogin.value = null;
  qrStatus.value = "";
  if (sessionId) {
    void cancelNeteaseQrLogin(sessionId).catch(() => undefined);
  }
}

async function disconnectAccount() {
  const account = activeAccount.value;
  if (!account || !window.confirm(`Disconnect ${account.displayName}?`)) {
    return;
  }
  isLoading.value = true;
  sourceError.value = null;
  try {
    await disconnectNeteaseAccount(account.accountRef);
    accounts.value = await listNeteaseAccounts();
    activeAccountRef.value = accounts.value[0]?.accountRef ?? "";
    recommendations.value = [];
    playlists.value = [];
    selectedPlaylist.value = null;
    audit.value = [];
    if (activeAccountRef.value) {
      await loadWorkspace();
    }
  } catch (error) {
    sourceError.value = normalizeError(error);
    await refreshAccountStatuses();
  } finally {
    isLoading.value = false;
  }
}

async function playTrack(track: RemoteTrack) {
  if (isPlayingTrackId.value) {
    return;
  }
  isPlayingTrackId.value = track.id;
  sourceError.value = null;
  try {
    const playback = await resolveNeteaseTrack(
      track,
      props.streamQuality,
      activeAccountRef.value || undefined,
      crypto.randomUUID(),
    );
    diagnostics.value = playback.diagnostics.map((diagnostic) => diagnostic.message);
    emit("playbackReady", playback);
  } catch (error) {
    sourceError.value = normalizeError(error);
    await refreshAccountStatuses();
  } finally {
    isPlayingTrackId.value = null;
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
  isMutating.value = true;
  sourceError.value = null;
  try {
    let result: Awaited<ReturnType<typeof addNeteasePlaylistTrack>>;
    if (mutation.operation === "add") {
      result = await addNeteasePlaylistTrack(
        accountRef,
        mutationPlaylistId.value,
        mutation.track,
      );
      sourceNotice.value = `${mutation.track.title} added to ${mutationPlaylist.value?.name ?? "Playlist"}.`;
    } else {
      result = await removeNeteasePlaylistTrack(
        accountRef,
        mutationPlaylistId.value,
        mutation.track,
      );
      sourceNotice.value = `${mutation.track.title} removed from ${mutationPlaylist.value?.name ?? "Playlist"}.`;
    }
    diagnostics.value = result.diagnostics.map((diagnostic) => diagnostic.message);
    pendingMutation.value = null;
    audit.value = await listNeteaseMutationAudit(accountRef);
    if (selectedPlaylistId.value === mutationPlaylistId.value) {
      await loadSelectedPlaylist();
    }
  } catch (error) {
    sourceError.value = normalizeError(error);
    try {
      audit.value = await listNeteaseMutationAudit(accountRef);
    } catch {
      // Keep the mutation error as the primary failure.
    }
    await refreshAccountStatuses();
  } finally {
    isMutating.value = false;
  }
}

async function refreshAccountStatuses() {
  try {
    accounts.value = await listNeteaseAccounts();
  } catch {
    // Preserve the operation error; account refresh is secondary context.
  }
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

function normalizeError(error: unknown): string {
  let candidate: unknown = error;
  if (typeof candidate === "string") {
    try {
      candidate = JSON.parse(candidate);
    } catch {
      return error as string;
    }
  }
  if (candidate instanceof Error) {
    return candidate.message;
  }
  if (candidate && typeof candidate === "object" && "message" in candidate) {
    const message = (candidate as { message?: unknown }).message;
    if (typeof message === "string") {
      return message;
    }
  }
  return "Unexpected NetEase error.";
}
</script>

<template>
  <section class="overflow-hidden rounded border border-base-300 bg-base-100">
    <header class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 lg:flex-row lg:items-center lg:justify-between">
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

    <div v-if="!isPluginReady" class="flex flex-col gap-3 p-5 sm:flex-row sm:items-center sm:justify-between">
      <div class="flex items-start gap-3">
        <ShieldAlert class="mt-0.5 shrink-0 text-warning" :size="18" aria-hidden="true" />
        <div>
          <div class="text-sm font-medium">Plugin permission review required</div>
          <div class="mt-1 text-xs text-base-content/60">NetEase requests remain blocked until the bundled Plugin is enabled.</div>
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
                    :disabled="Boolean(isPlayingTrackId)"
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
                @click="selectedPlaylistId = playlist.id; loadSelectedPlaylist()"
              >
                <span class="min-w-0 flex-1 truncate text-left">{{ playlist.name }}</span>
                <span class="text-xs tabular-nums opacity-60">{{ playlist.trackCount }}</span>
              </button>
            </li>
          </ul>
          <div v-if="!playlists.length" class="p-5 text-sm text-base-content/60">No Playlists returned.</div>
        </aside>

        <div class="min-w-0 overflow-x-auto">
          <div v-if="selectedPlaylist" class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
            <img v-if="selectedPlaylist.playlist.coverUrl" class="size-11 object-cover" :src="selectedPlaylist.playlist.coverUrl" alt="" />
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm font-semibold">{{ selectedPlaylist.playlist.name }}</div>
              <div class="text-xs text-base-content/60">{{ selectedPlaylist.playlist.ownerName }} · {{ selectedPlaylist.tracks.length }} tracks</div>
            </div>
            <span v-if="!selectedPlaylist.playlist.canMutate" class="badge badge-ghost badge-sm">Read only</span>
          </div>
          <table v-if="selectedPlaylist" class="table table-sm">
            <thead>
              <tr>
                <th class="w-12"></th>
                <th>Track</th>
                <th class="hidden sm:table-cell">Artist</th>
                <th class="hidden text-right md:table-cell">Time</th>
                <th class="w-20"></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="track in selectedPlaylist.tracks" :key="track.id">
                <td>
                  <button class="btn btn-square btn-ghost btn-sm" type="button" :disabled="Boolean(isPlayingTrackId)" :aria-label="`Play ${track.title}`" title="Play" @click="playTrack(track)">
                    <RefreshCw v-if="isPlayingTrackId === track.id" class="animate-spin" :size="15" aria-hidden="true" />
                    <Play v-else :size="15" aria-hidden="true" />
                  </button>
                </td>
                <td class="min-w-40 font-medium sm:min-w-52">
                  {{ track.title }}
                  <div class="mt-0.5 text-xs font-normal text-base-content/60 sm:hidden">{{ track.artist }}</div>
                </td>
                <td class="hidden sm:table-cell">{{ track.artist }}</td>
                <td class="hidden text-right tabular-nums md:table-cell">{{ formatDuration(track.durationSeconds) }}</td>
                <td>
                  <button
                    v-if="selectedPlaylist.playlist.canMutate"
                    class="btn btn-square btn-ghost btn-sm text-error"
                    type="button"
                    :aria-label="`Remove ${track.title} from Playlist`"
                    title="Remove from Playlist"
                    @click="requestRemove(track)"
                  >
                    <Trash2 :size="15" aria-hidden="true" />
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
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
