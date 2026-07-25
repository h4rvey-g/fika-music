<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  AlertCircle,
  ArrowLeft,
  Ban,
  Disc3,
  Download,
  ListMusic,
  Music2,
  Pause,
  Play,
  RefreshCw,
  Search,
  UserRound,
  X,
} from "@lucide/vue";
import type { AudioSourceRecord } from "../generated/bindings";
import OnlineTrackTable from "./OnlineTrackTable.vue";
import { cancelSourceRequest } from "../lib/plugin-api";
import { normalizeError } from "../lib/errors";
import {
  cancelOnlineDownloadTask,
  createOnlineDownloadTask,
  getOnlineAlbumTracks,
  getOnlineArtistTracks,
  getOnlineMusicSearchPage,
  getOnlineMusicSettings,
  getOnlineMusicSuggestions,
  getOnlinePlaylistTracks,
  listOnlineMusicChannels,
  listOnlineDownloadTasks,
  listenOnlineDownloadCompletions,
  listenOnlineDownloadTasks,
  listenOnlineMusicSearch,
  onlinePlaylistDetailError,
  pauseOnlineDownloadTask,
  refreshOnlineDownloadItemCandidates,
  retryOnlineDownloadItem,
  selectOnlineDownloadDirectory,
  startOnlineDownloadTask,
  startOnlineMusicSearch,
  updateOnlineMusicSettings,
  type OnlineAlbum,
  type OnlineArtist,
  type OnlineMusicSettings,
  type OnlineDownloadTask,
  type OnlinePlaylist,
  type OnlineSearchSection,
  type OnlineSearchSectionEvent,
  type OnlineSearchSectionResult,
  type OnlineTrack,
} from "../lib/online-music-api";

type SectionState = {
  loading: boolean;
  result: OnlineSearchSectionResult | null;
  error: string | null;
  page: number;
  loadingMore: boolean;
};

type DetailState =
  | { kind: "artist"; entity: OnlineArtist }
  | { kind: "album"; entity: OnlineAlbum }
  | { kind: "playlist"; entity: OnlinePlaylist };

const props = defineProps<{
  audioSources: AudioSourceRecord[];
  selectedAudioSourceId: string;
  activeOnlineTrackKey: string | null;
  resolvingOnlineTrackKey: string | null;
  isPlaying: boolean;
  localMusicFolder: string | null;
}>();

const emit = defineEmits<{
  playRequest: [
    track: OnlineTrack,
    queue: OnlineTrack[],
    index: number,
    appendable: boolean,
  ];
  openAudioSources: [];
  downloadCompleted: [destination: string];
  togglePlayback: [];
  openPlugin: [pluginId: string];
}>();

const sections: Array<{ id: OnlineSearchSection; label: string; icon: typeof Music2 }> = [
  { id: "songs", label: "Songs", icon: Music2 },
  { id: "artists", label: "Artists", icon: UserRound },
  { id: "albums", label: "Albums", icon: Disc3 },
  { id: "playlists", label: "Playlists", icon: ListMusic },
];

const query = ref("");
const submittedQuery = ref("");
const expandedSection = ref<OnlineSearchSection | null>(null);
const activeTab = ref<"search" | "downloads">("search");
const suggestions = ref<string[]>([]);
const suggestionsOpen = ref(false);
const suggestionLoading = ref(false);
const globalError = ref<string | null>(null);
const loginRequiredPluginId = ref<string | null>(null);
const detailRetryAvailable = ref(false);
const completionMessage = ref<string | null>(null);
const searchId = ref<string | null>(null);
const detail = ref<DetailState | null>(null);
const detailTracks = ref<OnlineTrack[]>([]);
const detailLoading = ref(false);
const detailLoadingMore = ref(false);
const detailHasMore = ref(false);
const detailPage = ref(1);
const detailAppendGeneration = ref(0);
const downloadTasks = ref<OnlineDownloadTask[]>([]);
const downloadActionId = ref<string | null>(null);
const settings = ref<OnlineMusicSettings | null>(null);
const sectionStates = ref<Record<OnlineSearchSection, SectionState>>(newSectionStates());
const resultScrollPosition = ref(0);
const summaryScrollPosition = ref(0);

let unlistenSearch: (() => void) | null = null;
let unlistenDownloads: (() => void) | null = null;
let unlistenDownloadCompletions: (() => void) | null = null;
let searchGeneration = 0;
let suggestionGeneration = 0;
let detailRequestGeneration = 0;
let suggestionTimer: number | null = null;
let suggestionRequestId: string | null = null;
let detailRequestId: string | null = null;
let pendingSearchEvents: OnlineSearchSectionEvent[] = [];

const hasSubmittedSearch = computed(() => Boolean(submittedQuery.value));
const visibleSections = computed(() =>
  expandedSection.value
    ? sections.filter((section) => section.id === expandedSection.value)
    : sections,
);
const visibleDetailTitle = computed(() => {
  if (!detail.value) return "";
  if (detail.value.kind === "artist") return detail.value.entity.name;
  if (detail.value.kind === "album") return detail.value.entity.title;
  return detail.value.entity.name;
});

onMounted(async () => {
  [unlistenSearch, unlistenDownloads, unlistenDownloadCompletions] = await Promise.all([
    listenOnlineMusicSearch(onSearchSection),
    listenOnlineDownloadTasks(upsertDownloadTask),
    listenOnlineDownloadCompletions((task) => {
      completionMessage.value = `${task.title}: ${task.completedItems} downloaded`;
      window.setTimeout(() => {
        completionMessage.value = null;
      }, 5_000);
    }),
  ]);
  try {
    const [loadedSettings, loadedTasks] = await Promise.all([
      getOnlineMusicSettings(),
      listOnlineDownloadTasks(),
    ]);
    settings.value = loadedSettings;
    downloadTasks.value = Array.isArray(loadedTasks) ? loadedTasks : [];
  } catch (error) {
    globalError.value = normalizeError(error);
  }
});

onBeforeUnmount(() => {
  unlistenSearch?.();
  unlistenDownloads?.();
  unlistenDownloadCompletions?.();
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  if (searchId.value) void cancelSourceRequest(searchId.value);
});

function newSectionStates(): Record<OnlineSearchSection, SectionState> {
  return {
    songs: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    artists: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    albums: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    playlists: { loading: false, result: null, error: null, page: 1, loadingMore: false },
  };
}

function onQueryInput() {
  globalError.value = null;
  suggestionsOpen.value = query.value.trim().length >= 2;
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  const generation = ++suggestionGeneration;
  const keyword = query.value.trim();
  if (keyword.length < 2) {
    suggestions.value = [];
    suggestionLoading.value = false;
    return;
  }
  suggestionLoading.value = true;
  suggestionTimer = window.setTimeout(async () => {
    const requestId = `online-suggest-${Date.now()}-${generation}`;
    suggestionRequestId = requestId;
    try {
      const result = await getOnlineMusicSuggestions(keyword, requestId);
      if (generation === suggestionGeneration) suggestions.value = result.suggestions;
    } catch {
      if (generation === suggestionGeneration) suggestions.value = [];
    } finally {
      if (generation === suggestionGeneration) suggestionLoading.value = false;
      if (suggestionRequestId === requestId) suggestionRequestId = null;
    }
  }, 300);
}

async function submitSearch(suggestion?: string) {
  const keyword = (suggestion ?? query.value).trim();
  if (!keyword) return;
  query.value = keyword;
  suggestionsOpen.value = false;
  detail.value = null;
  expandedSection.value = null;
  activeTab.value = "search";
  globalError.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  const generation = ++searchGeneration;
  const previousSearchId = searchId.value;
  searchId.value = null;
  pendingSearchEvents = [];
  if (previousSearchId) void cancelSourceRequest(previousSearchId);
  submittedQuery.value = keyword;
  sectionStates.value = newSectionStates();
  for (const section of sections) sectionStates.value[section.id].loading = true;
  try {
    const id = await startOnlineMusicSearch(keyword);
    if (generation !== searchGeneration) {
      void cancelSourceRequest(id);
      return;
    }
    searchId.value = id;
    for (const event of pendingSearchEvents.filter((event) => event.searchId === id)) {
      applySearchSection(event);
    }
    pendingSearchEvents = [];
  } catch (error) {
    for (const section of sections) sectionStates.value[section.id].loading = false;
    globalError.value = normalizeError(error);
  }
}

function onSearchSection(event: OnlineSearchSectionEvent) {
  if (!searchId.value) {
    pendingSearchEvents.push(event);
    return;
  }
  if (event.searchId !== searchId.value) return;
  applySearchSection(event);
}

function applySearchSection(event: OnlineSearchSectionEvent) {
  const state = sectionStates.value[event.result.section];
  state.loading = false;
  state.result = event.result;
  state.error = event.result.supportedChannels === 0
    ? "No enabled channel supports this search type."
    : event.result.completedChannels === event.result.failures.length
      ? "This section could not be loaded."
      : null;
}

function sectionItems<T>(section: OnlineSearchSection): T[] {
  const data = sectionStates.value[section].result?.data;
  return data?.section === section ? (data.items as T[]) : [];
}

async function loadMore(section: OnlineSearchSection) {
  const state = sectionStates.value[section];
  if (state.loadingMore || !submittedQuery.value) return;
  state.loadingMore = true;
  const expandSummary = state.page === 1 && (state.result?.data.items.length ?? 0) <= 5;
  const nextPage = expandSummary ? 1 : state.page + 1;
  const requestId = `online-page-${section}-${Date.now()}`;
  try {
    const next = await getOnlineMusicSearchPage(
      submittedQuery.value,
      section,
      nextPage,
      20,
      requestId,
    );
    if (expandSummary) {
      state.result = next;
    } else {
      appendSectionData(state, next);
    }
    state.page = nextPage;
  } catch (error) {
    state.error = normalizeError(error);
  } finally {
    state.loadingMore = false;
  }
}

function appendSectionData(state: SectionState, next: OnlineSearchSectionResult) {
  if (!state.result || state.result.data.section !== next.data.section) {
    state.result = next;
    return;
  }
  const existingKeys = new Set(state.result.data.items.map((item: { key: string }) => item.key));
  state.result.data.items.push(
    ...next.data.items.filter((item: { key: string }) => !existingKeys.has(item.key)) as never[],
  );
  state.result.hasMore = next.hasMore;
  state.result.failures = next.failures;
  state.result.completedChannels = next.completedChannels;
}

async function retrySection(section: OnlineSearchSection) {
  const state = sectionStates.value[section];
  state.loading = true;
  state.error = null;
  try {
    state.result = await getOnlineMusicSearchPage(submittedQuery.value, section, 1, 20);
    state.page = 1;
  } catch (error) {
    state.error = normalizeError(error);
  } finally {
    state.loading = false;
  }
}

async function openDetail(next: DetailState, rememberScroll = true) {
  if (rememberScroll) {
    resultScrollPosition.value = document.querySelector("main")?.scrollTop ?? 0;
  }
  detailAppendGeneration.value += 1;
  detail.value = next;
  detailTracks.value = [];
  detailPage.value = 1;
  detailHasMore.value = false;
  detailLoading.value = true;
  globalError.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  const requestId = `online-detail-${Date.now()}-${++detailRequestGeneration}`;
  detailRequestId = requestId;
  try {
    const page = await loadDetailPage(next, 1, requestId);
    if (detailRequestId !== requestId) return;
    detailTracks.value = page.items;
    detailHasMore.value = page.hasMore;
  } catch (error) {
    if (detailRequestId !== requestId) return;
    detailRetryAvailable.value = true;
    const playlistError = next.kind === "playlist" ? onlinePlaylistDetailError(error) : null;
    if (
      playlistError &&
      (playlistError.code === "credential-expired" || playlistError.code === "account-not-found")
    ) {
      loginRequiredPluginId.value = playlistError.pluginId;
      globalError.value = `${playlistError.channelName} requires login to read this playlist.`;
    } else {
      globalError.value = playlistError?.message ?? normalizeError(error);
    }
  } finally {
    if (detailRequestId === requestId) {
      detailLoading.value = false;
      detailRequestId = null;
    }
  }
}

async function retryDetail() {
  if (detail.value) await openDetail(detail.value, false);
}

async function loadDetailPage(next: DetailState, page: number, requestId?: string) {
  if (next.kind === "artist") return getOnlineArtistTracks(next.entity, requestId);
  if (next.kind === "album") return getOnlineAlbumTracks(next.entity, page, 100, requestId);
  return getOnlinePlaylistTracks(next.entity, page, 100, requestId);
}

async function loadMoreDetail() {
  if (!detail.value || detailLoadingMore.value || !detailHasMore.value) return;
  detailLoadingMore.value = true;
  const page = detailPage.value + 1;
  const requestId = `online-detail-page-${Date.now()}`;
  try {
    const result = await loadDetailPage(detail.value, page, requestId);
    detailTracks.value.push(...result.items);
    detailHasMore.value = result.hasMore;
    detailPage.value = page;
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    detailLoadingMore.value = false;
  }
}

async function playTrack(track: OnlineTrack, queue: OnlineTrack[], appendable = false) {
  globalError.value = null;
  let currentTrack = track;
  try {
    const enabledChannels = new Set(
      (await listOnlineMusicChannels()).map((channel) => channel.id),
    );
    currentTrack = {
      ...track,
      candidates: track.candidates.filter((candidate) =>
        enabledChannels.has(candidate.channelId),
      ),
    };
  } catch {
    globalError.value = "Playback is unavailable from the configured Audio Sources.";
    return;
  }
  if (!currentTrack.candidates.length) {
    globalError.value = "Playback is unavailable from the configured Audio Sources.";
    return;
  }
  const queueIndex = queue.findIndex((item) => item.key === track.key);
  const currentQueue = appendable ? queue : [...queue];
  if (queueIndex >= 0) currentQueue[queueIndex] = currentTrack;
  emit(
    "playRequest",
    currentTrack,
    currentQueue,
    queueIndex,
    appendable,
  );
}

function requestTrackPlayback(track: OnlineTrack, queue: OnlineTrack[]) {
  if (
    props.activeOnlineTrackKey === track.key &&
    props.resolvingOnlineTrackKey !== track.key
  ) {
    emit("togglePlayback");
    return;
  }
  void playTrack(track, queue);
}

async function openSection(section: OnlineSearchSection) {
  summaryScrollPosition.value = document.querySelector("main")?.scrollTop ?? 0;
  expandedSection.value = section;
  await nextTick();
  const state = sectionStates.value[section];
  if ((state.result?.data.items.length ?? 0) <= 5 && state.result?.hasMore) {
    await loadMore(section);
  }
  const main = document.querySelector("main");
  if (main) main.scrollTop = 0;
}

function closeSection() {
  expandedSection.value = null;
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = summaryScrollPosition.value;
  });
}

async function playAllDetail() {
  if (!detail.value || !detailTracks.value.length) return;
  const queue = [...detailTracks.value];
  await playTrack(queue[0], queue, true);
  if (!detailHasMore.value || detail.value.kind === "artist") return;
  const generation = ++detailAppendGeneration.value;
  let page = detailPage.value;
  let hasMore: boolean = detailHasMore.value;
  while (hasMore && generation === detailAppendGeneration.value && detail.value) {
    try {
      const result = await loadDetailPage(detail.value, ++page, `online-play-all-${Date.now()}-${page}`);
      for (const track of result.items) {
        if (!queue.some((existing) => existing.key === track.key)) queue.push(track);
      }
      hasMore = result.hasMore;
    } catch {
      break;
    }
  }
}

async function downloadTrack(track: OnlineTrack) {
  await createDownload("track", track.title, [track]);
}

async function downloadAll() {
  if (!detail.value || !detailTracks.value.length) return;
  const tracks = [...detailTracks.value];
  if (detail.value.kind !== "artist") {
    let page = detailPage.value;
    let hasMore = detailHasMore.value;
    while (hasMore) {
      try {
        const result = await loadDetailPage(detail.value, ++page, `online-download-all-${Date.now()}-${page}`);
        for (const track of result.items) {
          if (!tracks.some((existing) => existing.key === track.key)) tracks.push(track);
        }
        hasMore = result.hasMore;
      } catch (error) {
        globalError.value = normalizeError(error);
        return;
      }
    }
  }
  await createDownload(detail.value.kind, visibleDetailTitle.value, tracks);
}

async function createDownload(kind: string, title: string, tracks: OnlineTrack[]) {
  if (settings.value && !settings.value.downloadDirectory) {
    try {
      const directory = await selectOnlineDownloadDirectory(props.localMusicFolder);
      if (!directory) return;
      settings.value = await updateOnlineMusicSettings({
        ...settings.value,
        downloadDirectory: directory,
      });
    } catch (error) {
      globalError.value = normalizeError(error);
      return;
    }
  }
  activeTab.value = "downloads";
  downloadActionId.value = "create";
  try {
    const task = await createOnlineDownloadTask(
      kind,
      title,
      tracks,
      props.selectedAudioSourceId,
      props.localMusicFolder,
    );
    upsertDownloadTask(task);
    upsertDownloadTask(await startOnlineDownloadTask(task.taskId));
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    downloadActionId.value = null;
  }
}

function upsertDownloadTask(task: OnlineDownloadTask) {
  const index = downloadTasks.value.findIndex((item) => item.taskId === task.taskId);
  const previousState = index >= 0 ? downloadTasks.value[index].state : null;
  if (index >= 0) downloadTasks.value[index] = task;
  else downloadTasks.value.unshift(task);
  downloadTasks.value.sort((left, right) => right.updatedAt - left.updatedAt);
  if (
    previousState !== task.state &&
    (task.state === "completed" || task.state === "completedWithErrors") &&
    task.completedItems > 0
  ) {
    emit("downloadCompleted", task.destination);
  }
}

async function runDownloadAction(
  task: OnlineDownloadTask,
  action: "start" | "pause" | "cancel",
) {
  downloadActionId.value = task.taskId;
  try {
    const updated = action === "start"
      ? await startOnlineDownloadTask(task.taskId)
      : action === "pause"
        ? await pauseOnlineDownloadTask(task.taskId)
        : await cancelOnlineDownloadTask(task.taskId);
    upsertDownloadTask(updated);
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    downloadActionId.value = null;
  }
}

async function retryDownloadItem(task: OnlineDownloadTask, itemId: string) {
  downloadActionId.value = itemId;
  try {
    upsertDownloadTask(await retryOnlineDownloadItem(task.taskId, itemId));
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    downloadActionId.value = null;
  }
}

async function refreshAndRetryDownloadItem(task: OnlineDownloadTask, itemId: string) {
  downloadActionId.value = itemId;
  try {
    upsertDownloadTask(await refreshOnlineDownloadItemCandidates(task.taskId, itemId));
    upsertDownloadTask(await retryOnlineDownloadItem(task.taskId, itemId));
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    downloadActionId.value = null;
  }
}

function taskProgress(task: OnlineDownloadTask) {
  if (!task.totalItems) return 0;
  return Math.round(
    ((task.completedItems + task.skippedItems + task.failedItems) / task.totalItems) * 100,
  );
}

function backToResults() {
  detailAppendGeneration.value += 1;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  detailRequestId = null;
  detailLoading.value = false;
  detailLoadingMore.value = false;
  detail.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = resultScrollPosition.value;
  });
}

function dismissGlobalError() {
  globalError.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
}

</script>

<template>
  <div class="mx-auto flex w-full max-w-7xl flex-col gap-4 px-3 py-3 lg:px-5 lg:py-4">
    <div class="flex flex-col gap-3 border-b border-base-300 pb-4 sm:flex-row sm:items-center">
      <div class="relative min-w-0 flex-1">
        <form class="join flex w-full" role="search" @submit.prevent="submitSearch()">
          <label class="input input-sm join-item flex min-w-0 flex-1 items-center gap-2">
            <Search :size="16" aria-hidden="true" />
            <input
              v-model="query"
              class="min-w-0 grow"
              type="search"
              autocomplete="off"
              placeholder="Search songs, artists, albums, and playlists"
              aria-label="Search Online Music"
              @input="onQueryInput"
              @focus="suggestionsOpen = query.trim().length >= 2"
            />
          </label>
          <button class="btn btn-primary btn-sm join-item" type="submit" :disabled="!query.trim()">
            <Search :size="16" aria-hidden="true" />
            Search
          </button>
        </form>

        <div
          v-if="suggestionsOpen && (suggestionLoading || suggestions.length)"
          class="absolute inset-x-0 top-full z-30 mt-1 overflow-hidden rounded border border-base-300 bg-base-100 shadow-lg"
        >
          <div v-if="suggestionLoading" class="flex h-10 items-center gap-2 px-3 text-xs text-base-content/60">
            <span class="loading loading-spinner loading-xs"></span>
            Loading suggestions
          </div>
          <ul v-else class="menu menu-sm w-full p-1">
            <li v-for="suggestion in suggestions" :key="suggestion">
              <button type="button" @mousedown.prevent="submitSearch(suggestion)">
                <Search :size="14" aria-hidden="true" />
                <span class="truncate">{{ suggestion }}</span>
              </button>
            </li>
          </ul>
        </div>
      </div>

      <div role="tablist" class="tabs tabs-border shrink-0">
        <button role="tab" class="tab" :class="{ 'tab-active': activeTab === 'search' }" @click="activeTab = 'search'">
          Search
        </button>
        <button role="tab" class="tab" :class="{ 'tab-active': activeTab === 'downloads' }" @click="activeTab = 'downloads'">
          Downloads
          <span v-if="downloadTasks.length" class="badge badge-sm ml-1">{{ downloadTasks.length }}</span>
        </button>
      </div>
    </div>

    <div v-if="globalError" role="alert" class="alert alert-error py-2">
      <AlertCircle :size="17" aria-hidden="true" />
      <span class="min-w-0 flex-1 text-sm">{{ globalError }}</span>
      <div v-if="loginRequiredPluginId || detailRetryAvailable" class="flex shrink-0 flex-wrap gap-1">
        <button
          v-if="loginRequiredPluginId"
          class="btn btn-sm"
          type="button"
          @click="emit('openPlugin', loginRequiredPluginId)"
        >
          Open channel
        </button>
        <button v-if="detailRetryAvailable" class="btn btn-sm" type="button" @click="retryDetail">
          <RefreshCw :size="14" aria-hidden="true" />
          Retry
        </button>
      </div>
      <button class="btn btn-square btn-ghost btn-xs" type="button" aria-label="Dismiss error" @click="dismissGlobalError">
        <X :size="14" aria-hidden="true" />
      </button>
    </div>
    <div v-if="completionMessage" role="status" class="alert alert-success py-2 text-sm">
      <Download :size="16" aria-hidden="true" />
      {{ completionMessage }}
    </div>

    <div v-if="activeTab === 'downloads'" class="min-h-64">
      <div class="flex items-center justify-between border-b border-base-300 pb-2">
        <h2 class="text-sm font-semibold">Download tasks</h2>
        <span class="text-xs text-base-content/55">{{ downloadTasks.length }} task{{ downloadTasks.length === 1 ? '' : 's' }}</span>
      </div>
      <div v-if="!downloadTasks.length" class="flex min-h-48 flex-col items-center justify-center gap-2 text-base-content/55">
        <Download :size="24" aria-hidden="true" />
        <span class="text-sm">No download tasks</span>
      </div>
      <div v-else class="divide-y divide-base-300">
        <section v-for="task in downloadTasks" :key="task.taskId" class="py-3">
          <div class="flex min-w-0 items-start gap-3">
            <Download :size="17" class="mt-1 shrink-0" aria-hidden="true" />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="min-w-0 flex-1 truncate text-sm font-medium">{{ task.title }}</h3>
                <span class="badge badge-sm" :class="{ 'badge-error': task.state === 'completedWithErrors', 'badge-success': task.state === 'completed' }">
                  {{ task.state }}
                </span>
              </div>
              <div class="mt-1 flex items-center gap-2 text-xs text-base-content/55">
                <span>{{ task.completedItems }} complete</span>
                <span>{{ task.skippedItems }} skipped</span>
                <span v-if="task.failedItems">{{ task.failedItems }} failed</span>
                <span class="ml-auto tabular-nums">{{ taskProgress(task) }}%</span>
              </div>
              <progress class="progress progress-primary mt-1 h-1.5 w-full" :value="taskProgress(task)" max="100"></progress>
            </div>
            <div class="flex shrink-0 gap-1">
              <button
                v-if="task.state === 'paused' || task.state === 'queued' || task.state === 'completedWithErrors'"
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="downloadActionId === task.taskId"
                aria-label="Resume download task"
                title="Resume"
                @click="runDownloadAction(task, 'start')"
              >
                <RefreshCw v-if="downloadActionId === task.taskId" class="animate-spin" :size="14" aria-hidden="true" />
                <Play v-else :size="14" aria-hidden="true" />
              </button>
              <button
                v-if="task.state === 'running'"
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="downloadActionId === task.taskId"
                aria-label="Pause download task"
                title="Pause"
                @click="runDownloadAction(task, 'pause')"
              >
                <Pause :size="14" aria-hidden="true" />
              </button>
              <button
                v-if="!['completed', 'cancelled'].includes(task.state)"
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="downloadActionId === task.taskId"
                aria-label="Cancel download task"
                title="Cancel"
                @click="runDownloadAction(task, 'cancel')"
              >
                <Ban :size="14" aria-hidden="true" />
              </button>
            </div>
          </div>
          <ul class="mt-2 divide-y divide-base-300/70 pl-7">
            <li v-for="item in task.items" :key="item.itemId" class="flex min-w-0 items-center gap-2 py-1.5 text-xs">
              <span class="w-24 shrink-0 text-base-content/50">{{ item.state }}</span>
              <span class="min-w-0 flex-1 truncate">{{ item.track.artist }} - {{ item.track.title }}</span>
              <span v-if="item.totalBytes" class="shrink-0 tabular-nums text-base-content/45">
                {{ Math.round((item.bytesDownloaded / item.totalBytes) * 100) }}%
              </span>
              <span v-if="item.message" class="hidden max-w-56 truncate text-error lg:block" :title="item.message">{{ item.message }}</span>
              <button
                v-if="item.state === 'failed' && task.state !== 'running'"
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="downloadActionId === item.itemId"
                :aria-label="`Refresh candidates for ${item.track.title}`"
                title="Refresh candidates"
                @click="refreshAndRetryDownloadItem(task, item.itemId)"
              >
                <Search :class="{ 'animate-pulse': downloadActionId === item.itemId }" :size="13" aria-hidden="true" />
              </button>
              <button
                v-if="(item.state === 'failed' || item.state === 'cancelled') && task.state !== 'running'"
                class="btn btn-square btn-ghost btn-xs"
                type="button"
                :disabled="downloadActionId === item.itemId"
                :aria-label="`Retry ${item.track.title}`"
                title="Retry"
                @click="retryDownloadItem(task, item.itemId)"
              >
                <RefreshCw :class="{ 'animate-spin': downloadActionId === item.itemId }" :size="13" aria-hidden="true" />
              </button>
            </li>
          </ul>
        </section>
      </div>
    </div>

    <div v-else-if="detail" class="min-w-0">
      <div class="mb-3 flex min-w-0 items-center gap-3 border-b border-base-300 pb-3">
        <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Back to search results" title="Back" @click="backToResults">
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <div class="flex size-12 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
          <img v-if="detail.entity.coverUrl" :src="detail.entity.coverUrl" class="size-full object-cover" alt="" />
          <component :is="detail.kind === 'artist' ? UserRound : detail.kind === 'album' ? Disc3 : ListMusic" v-else :size="22" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ visibleDetailTitle }}</h2>
          <p class="truncate text-xs text-base-content/55">
            {{ detail.kind === 'artist' ? 'Top tracks' : detail.kind === 'album' ? detail.entity.artist : detail.entity.ownerName || detail.entity.channelName }}
          </p>
        </div>
        <div v-if="detailTracks.length" class="flex shrink-0 gap-1">
          <button class="btn btn-sm" type="button" @click="playAllDetail">
            <Play :size="15" aria-hidden="true" />
            Play all
          </button>
          <button class="btn btn-square btn-sm" type="button" aria-label="Download all loaded tracks" title="Download all" @click="downloadAll">
            <Download :size="15" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div v-if="detailLoading" class="space-y-2">
        <div v-for="index in 7" :key="index" class="skeleton h-11 w-full"></div>
      </div>
      <OnlineTrackTable
        v-else
        :tracks="detailTracks"
        :active-key="activeOnlineTrackKey"
        :playing="isPlaying"
        :resolving-key="resolvingOnlineTrackKey"
        @play="playTrack($event, detailTracks)"
        @download="downloadTrack"
      />
      <div v-if="detailHasMore" class="flex justify-center py-4">
        <button class="btn btn-sm" type="button" :disabled="detailLoadingMore" @click="loadMoreDetail">
          <span v-if="detailLoadingMore" class="loading loading-spinner loading-xs"></span>
          Load more
        </button>
      </div>
    </div>

    <div v-else data-online-results class="flex flex-col gap-5">
      <div v-if="!hasSubmittedSearch" class="flex min-h-64 flex-col items-center justify-center gap-3 text-center text-base-content/55">
        <Music2 :size="28" aria-hidden="true" />
        <div class="text-sm font-medium text-base-content/75">Search enabled music channels</div>
      </div>

      <div v-if="expandedSection" class="flex items-center gap-2 border-b border-base-300 pb-3">
        <button class="btn btn-square btn-ghost btn-sm" type="button" aria-label="Back to search summary" title="Back" @click="closeSection">
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <span class="text-sm font-medium">All {{ sections.find((section) => section.id === expandedSection)?.label }}</span>
      </div>

      <section v-for="section in visibleSections" :key="section.id" class="min-w-0">
        <div class="mb-2 flex items-center gap-2 border-b border-base-300 pb-2">
          <component :is="section.icon" :size="17" aria-hidden="true" />
          <h2 class="text-sm font-semibold">{{ section.label }}</h2>
          <span v-if="sectionStates[section.id].result" class="text-xs tabular-nums text-base-content/50">
            {{ sectionStates[section.id].result?.data.items.length }} loaded
          </span>
          <span v-if="sectionStates[section.id].result?.failures.length" class="badge badge-warning badge-sm ml-auto">
            Partial
          </span>
          <button
            v-if="sectionStates[section.id].result?.failures.length"
            class="btn btn-square btn-ghost btn-xs"
            type="button"
            :aria-label="`Retry unavailable ${section.label} channels`"
            title="Retry unavailable channels"
            @click="retrySection(section.id)"
          >
            <RefreshCw :size="13" aria-hidden="true" />
          </button>
        </div>

        <div v-if="sectionStates[section.id].loading" class="space-y-2">
          <div v-for="index in 5" :key="index" class="skeleton h-11 w-full"></div>
        </div>
        <div v-else-if="sectionStates[section.id].error" class="flex min-h-24 items-center justify-between gap-3 text-sm text-base-content/60">
          <span>{{ sectionStates[section.id].error }}</span>
          <button class="btn btn-sm" type="button" @click="retrySection(section.id)">
            <RefreshCw :size="15" aria-hidden="true" />
            Retry
          </button>
        </div>
        <div v-else-if="!sectionStates[section.id].result?.data.items.length" class="flex min-h-20 items-center text-sm text-base-content/50">
          No {{ section.label.toLowerCase() }} found
        </div>

        <OnlineTrackTable
          v-else-if="section.id === 'songs'"
          :tracks="sectionItems<OnlineTrack>('songs')"
          :active-key="activeOnlineTrackKey"
          :playing="isPlaying"
          :resolving-key="resolvingOnlineTrackKey"
          @play="requestTrackPlayback($event, sectionItems<OnlineTrack>('songs'))"
          @download="downloadTrack"
        />

        <ul v-else class="list divide-y divide-base-300">
          <li
            v-for="item in sectionItems<any>(section.id)"
            :key="item.key"
            class="list-row cursor-pointer px-0 py-2 hover:bg-base-200/60"
            @click="openDetail({ kind: section.id === 'artists' ? 'artist' : section.id === 'albums' ? 'album' : 'playlist', entity: item } as DetailState)"
          >
            <div class="flex size-10 items-center justify-center overflow-hidden rounded bg-base-200">
              <img v-if="item.coverUrl" :src="item.coverUrl" class="size-full object-cover" alt="" />
              <component :is="section.icon" v-else :size="18" aria-hidden="true" />
            </div>
            <div class="min-w-0">
              <div class="truncate text-sm font-medium">{{ item.name ?? item.title }}</div>
              <div class="truncate text-xs text-base-content/55">
                {{ item.artist ?? item.ownerName ?? item.candidates?.map((candidate: any) => candidate.channelName).join(' / ') ?? item.channelName }}
              </div>
            </div>
            <span class="text-xs tabular-nums text-base-content/45">{{ item.trackCount ?? '' }}</span>
          </li>
        </ul>

        <div v-if="sectionStates[section.id].result?.hasMore" class="flex justify-center pt-3">
          <button class="btn btn-sm" type="button" :disabled="sectionStates[section.id].loadingMore" @click="expandedSection === section.id ? loadMore(section.id) : openSection(section.id)">
            <span v-if="sectionStates[section.id].loadingMore" class="loading loading-spinner loading-xs"></span>
            {{ expandedSection === section.id ? 'Load more' : `More ${section.label}` }}
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
