<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  AlertCircle,
  ArrowLeft,
  Ban,
  CalendarDays,
  ChevronRight,
  Disc3,
  Download,
  House,
  ListPlus,
  ListMusic,
  Music2,
  Pause,
  Play,
  Radar,
  RadioTower,
  RefreshCw,
  Search,
  UserRound,
  X,
} from "@lucide/vue";
import type { AudioSourceRecord } from "../generated/bindings";
import OnlineTrackTable from "./OnlineTrackTable.vue";
import { cancelSourceRequest } from "../lib/plugin-api";
import type { RemoteTrack } from "../lib/plugin-api";
import { normalizeError } from "../lib/errors";
import {
  cancelOnlineDownloadTask,
  createOnlineDownloadTask,
  getOnlineAlbumTracks,
  getOnlineArtistTracks,
  getOnlineMusicPlaylists,
  getOnlineMusicRecommendations,
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
  type MusicRecommendationKind,
  type OnlineRecommendationsResult,
  type OnlinePlaylist,
  type OnlinePlaylistsResult,
  type OnlineSearchSection,
  type OnlineSearchSectionEvent,
  type OnlineSearchSectionResult,
  type OnlineTrack,
} from "../lib/online-music-api";
import { addNeteasePlaylistTrack, NETEASE_PLUGIN_ID } from "../lib/netease-api";
import { addKugouPlaylistTrack, KUGOU_PLUGIN_ID } from "../lib/kugou-api";

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

type RecommendationEntry = {
  id: MusicRecommendationKind;
  label: string;
  icon: typeof Music2;
  providers: Array<{ label: string; pluginId: string }>;
};

type RecommendationPreviewState = {
  result: OnlineRecommendationsResult | null;
  loading: boolean;
  error: string | null;
  requestId: string | null;
  generation: number;
};

type PlaylistTarget = {
  playlist: OnlinePlaylist & { accountRef: string };
  candidate: OnlineTrack["candidates"][number];
};

type PlaylistSelectionTarget = {
  playlist: OnlinePlaylist & { accountRef: string };
  candidates: OnlineTrack["candidates"];
};

const props = defineProps<{
  isActive: boolean;
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
    loadMore?: () => Promise<OnlineTrack[]>,
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

const recommendationEntries: RecommendationEntry[] = [
  {
    id: "daily",
    label: "每日推荐",
    icon: CalendarDays,
    providers: [
      { label: "NetEase", pluginId: NETEASE_PLUGIN_ID },
      { label: "KuGou", pluginId: KUGOU_PLUGIN_ID },
    ],
  },
  {
    id: "roaming",
    label: "私人漫游",
    icon: RadioTower,
    providers: [{ label: "NetEase", pluginId: NETEASE_PLUGIN_ID }],
  },
  {
    id: "radar",
    label: "私人雷达",
    icon: Radar,
    providers: [{ label: "NetEase", pluginId: NETEASE_PLUGIN_ID }],
  },
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
const activeRecommendation = ref<MusicRecommendationKind | null>(null);
const recommendationTracks = ref<OnlineTrack[]>([]);
const recommendationLoading = ref(false);
const recommendationError = ref<string | null>(null);
const recommendationFailures = ref<OnlineRecommendationsResult["failures"]>([]);
const privateRoamingBatchLoading = ref(false);
const recommendationPreviews = ref<Record<MusicRecommendationKind, RecommendationPreviewState>>(
  newRecommendationPreviewStates(),
);
const playlistLibraryResult = ref<OnlinePlaylistsResult | null>(null);
const playlistLibraryLoading = ref(false);
const playlistLibraryError = ref<string | null>(null);
const pendingPlaylistTracks = ref<OnlineTrack[]>([]);
const selectedPlaylistTargetKey = ref("");
const trackActionId = ref<string | null>(null);
const trackActionMessage = ref<string | null>(null);
const favoriteTrackKeys = ref<Set<string>>(new Set());
const favoriteTrackIdentities = ref<Set<string>>(new Set());

let unlistenSearch: (() => void) | null = null;
let unlistenDownloads: (() => void) | null = null;
let unlistenDownloadCompletions: (() => void) | null = null;
let searchGeneration = 0;
let suggestionGeneration = 0;
let detailRequestGeneration = 0;
let suggestionTimer: number | null = null;
let suggestionRequestId: string | null = null;
let detailRequestId: string | null = null;
let recommendationGeneration = 0;
let privateRoamingBatchGeneration = 0;
let privateRoamingBatchRequestId: string | null = null;
let pendingPrivateRoamingBatch: Promise<OnlineRecommendationsResult | null> | null = null;
let playlistLibraryGeneration = 0;
let playlistLibraryRequestId: string | null = null;
let pendingPlaylistLibraryLoad: Promise<OnlinePlaylistsResult | null> | null = null;
let trackActionMessageTimer: number | null = null;
let pendingSearchEvents: OnlineSearchSectionEvent[] = [];
const pendingRecommendationLoads = new Map<
  MusicRecommendationKind,
  Promise<OnlineRecommendationsResult | null>
>();

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
const activeRecommendationEntry = computed(() =>
  recommendationEntries.find((entry) => entry.id === activeRecommendation.value) ?? null,
);
const playlistLibraryItems = computed(() => playlistLibraryResult.value?.items ?? []);
const playlistLibraryFailures = computed(() => playlistLibraryResult.value?.failures ?? []);
const playlistProviders = [
  { label: "NetEase", pluginId: NETEASE_PLUGIN_ID },
  { label: "KuGou", pluginId: KUGOU_PLUGIN_ID },
];
const playlistProviderSections = computed(() => playlistProviders
  .map((provider) => ({
    ...provider,
    items: playlistLibraryItems.value.filter((playlist) =>
      playlist.pluginId === provider.pluginId
    ),
  }))
  .filter((provider) => provider.items.length > 0));
const failedPlaylistProviders = computed(() => playlistProviders.filter((provider) =>
  playlistLibraryFailures.value.some((failure) =>
    failure.channelId === provider.pluginId
    || failure.channelId.startsWith(`${provider.pluginId}:`)
    || failure.channelName.toLowerCase().includes(provider.label.toLowerCase())
  )
));
const pendingPlaylistTargets = computed(() =>
  playlistTargetsForTracks(pendingPlaylistTracks.value)
);
const selectedPlaylistTarget = computed(() =>
  pendingPlaylistTargets.value.find((target) =>
    target.playlist.key === selectedPlaylistTargetKey.value
  ) ?? null
);
const playlistPickerActionId = computed(() => {
  if (!pendingPlaylistTracks.value.length) return null;
  return pendingPlaylistTracks.value.length === 1
    ? `playlist:${pendingPlaylistTracks.value[0].key}`
    : "playlist:selection";
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
  if (props.isActive) void preloadForYou();
});

watch(
  () => props.isActive,
  (isActive) => {
    if (isActive) {
      void preloadForYou(true);
    } else if (playlistLibraryRequestId) {
      cancelPlaylistLibraryLoad();
    }
  },
);

onBeforeUnmount(() => {
  unlistenSearch?.();
  unlistenDownloads?.();
  unlistenDownloadCompletions?.();
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  cancelRecommendationLoads();
  cancelPlaylistLibraryLoad();
  cancelPrivateRoamingBatchLoad();
  if (searchId.value) void cancelSourceRequest(searchId.value);
  if (trackActionMessageTimer !== null) window.clearTimeout(trackActionMessageTimer);
});

function newSectionStates(): Record<OnlineSearchSection, SectionState> {
  return {
    songs: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    artists: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    albums: { loading: false, result: null, error: null, page: 1, loadingMore: false },
    playlists: { loading: false, result: null, error: null, page: 1, loadingMore: false },
  };
}

function newRecommendationPreviewStates(): Record<
  MusicRecommendationKind,
  RecommendationPreviewState
> {
  const createState = (): RecommendationPreviewState => ({
    result: null,
    loading: false,
    error: null,
    requestId: null,
    generation: 0,
  });
  return {
    daily: createState(),
    roaming: createState(),
    radar: createState(),
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

function recommendationCoverUrl(kind: MusicRecommendationKind) {
  return recommendationPreviews.value[kind].result?.items[0]?.coverUrl ?? null;
}

function recommendationResultError(result: OnlineRecommendationsResult) {
  if (result.supportedChannels === 0) {
    return "No enabled channel supports this recommendation.";
  }
  if (!result.items.length) {
    return result.failures[0]?.message ?? "No recommendations available.";
  }
  return null;
}

function applyRecommendationResult(result: OnlineRecommendationsResult) {
  recommendationTracks.value = result.items;
  recommendationFailures.value = result.failures;
  recommendationError.value = recommendationResultError(result);
}

async function loadRecommendation(
  kind: MusicRecommendationKind,
  force = false,
): Promise<OnlineRecommendationsResult | null> {
  const state = recommendationPreviews.value[kind];
  const pending = pendingRecommendationLoads.get(kind);
  if (!force && pending) return pending;
  if (!force && state.result) return state.result;

  if (state.requestId) void cancelSourceRequest(state.requestId);
  const generation = ++state.generation;
  const requestId = `online-recommendation-${kind}-${Date.now()}-${generation}`;
  state.requestId = requestId;
  state.loading = true;
  state.error = null;

  const load = (async () => {
    try {
      const result = await getOnlineMusicRecommendations(kind, requestId);
      if (generation !== state.generation) return null;
      state.result = result;
      return result;
    } catch (error) {
      if (generation === state.generation) state.error = normalizeError(error);
      return null;
    } finally {
      if (generation === state.generation) {
        state.loading = false;
        state.requestId = null;
      }
    }
  })();
  pendingRecommendationLoads.set(kind, load);
  void load.finally(() => {
    if (pendingRecommendationLoads.get(kind) === load) {
      pendingRecommendationLoads.delete(kind);
    }
  });
  return load;
}

async function loadPlaylistLibrary(force = false): Promise<OnlinePlaylistsResult | null> {
  if (!force && pendingPlaylistLibraryLoad) return pendingPlaylistLibraryLoad;
  if (!force && playlistLibraryResult.value) return playlistLibraryResult.value;

  if (playlistLibraryRequestId) void cancelSourceRequest(playlistLibraryRequestId);
  const generation = ++playlistLibraryGeneration;
  const requestId = `online-playlists-${Date.now()}-${generation}`;
  playlistLibraryRequestId = requestId;
  playlistLibraryLoading.value = true;
  playlistLibraryError.value = null;

  const load = (async () => {
    try {
      const result = await getOnlineMusicPlaylists(requestId);
      if (generation !== playlistLibraryGeneration) return null;
      playlistLibraryResult.value = result;
      return result;
    } catch (error) {
      if (generation === playlistLibraryGeneration) {
        playlistLibraryError.value = normalizeError(error);
      }
      return null;
    } finally {
      if (generation === playlistLibraryGeneration) {
        playlistLibraryLoading.value = false;
        playlistLibraryRequestId = null;
      }
    }
  })();
  pendingPlaylistLibraryLoad = load;
  void load.finally(() => {
    if (pendingPlaylistLibraryLoad === load) pendingPlaylistLibraryLoad = null;
  });
  return load;
}

function cancelPlaylistLibraryLoad() {
  playlistLibraryGeneration += 1;
  if (playlistLibraryRequestId) void cancelSourceRequest(playlistLibraryRequestId);
  playlistLibraryRequestId = null;
  playlistLibraryLoading.value = false;
  pendingPlaylistLibraryLoad = null;
}

function supportsLibraryActions(track: OnlineTrack) {
  return track.candidates.some((candidate) =>
    candidate.pluginId === NETEASE_PLUGIN_ID || candidate.pluginId === KUGOU_PLUGIN_ID
  );
}

function supportsPlaylistSelection(tracks: OnlineTrack[]) {
  if (!tracks.length) return false;
  return [NETEASE_PLUGIN_ID, KUGOU_PLUGIN_ID].some((pluginId) =>
    tracks.every((track) =>
      track.candidates.some((candidate) => candidate.pluginId === pluginId)
    )
  );
}

function trackIdentity(candidate: OnlineTrack["candidates"][number]) {
  return `${candidate.pluginId}:${candidate.sourceId}:${candidate.id}`;
}

function isTrackFavorite(track: OnlineTrack) {
  return favoriteTrackKeys.value.has(track.key) || track.candidates.some((candidate) =>
    favoriteTrackIdentities.value.has(trackIdentity(candidate))
  );
}

function isTrackActionPending(track: OnlineTrack, action: "favorite" | "playlist") {
  return trackActionId.value === `${action}:${track.key}`;
}

function isDownloadActionPending() {
  return downloadActionId.value === "create";
}

function playlistTargetsForTrack(track: OnlineTrack, favoritesOnly = false): PlaylistTarget[] {
  return playlistLibraryItems.value.flatMap((playlist) => {
    if (
      !playlist.accountRef ||
      !playlist.canMutate ||
      (favoritesOnly && !playlist.isFavorite)
    ) {
      return [];
    }
    const candidate = track.candidates.find((candidate) =>
      candidate.pluginId === playlist.pluginId
    );
    if (!candidate) return [];
    return [{
      playlist: { ...playlist, accountRef: playlist.accountRef },
      candidate,
    }];
  });
}

function playlistTargetsForTracks(tracks: OnlineTrack[]): PlaylistSelectionTarget[] {
  if (!tracks.length) return [];
  return playlistLibraryItems.value.flatMap((playlist) => {
    if (!playlist.accountRef || !playlist.canMutate) return [];
    const candidates = tracks.map((track) =>
      track.candidates.find((candidate) => candidate.pluginId === playlist.pluginId)
    );
    if (candidates.some((candidate) => !candidate)) return [];
    return [{
      playlist: { ...playlist, accountRef: playlist.accountRef },
      candidates: candidates as OnlineTrack["candidates"],
    }];
  });
}

function remoteTrackFromCandidate(candidate: OnlineTrack["candidates"][number]): RemoteTrack {
  return {
    id: candidate.id,
    source: candidate.sourceId,
    title: candidate.title,
    artist: candidate.artist,
    album: candidate.album,
    durationSeconds: candidate.durationSeconds,
    coverUrl: candidate.coverUrl,
    trackNumber: candidate.trackNumber ?? undefined,
    discNumber: candidate.discNumber ?? undefined,
    platformIds: candidate.platformIds,
    rawInfo: candidate.rawInfo,
  };
}

async function addTrackToPlaylist(target: PlaylistTarget) {
  const track = remoteTrackFromCandidate(target.candidate);
  if (target.playlist.pluginId === NETEASE_PLUGIN_ID) {
    return addNeteasePlaylistTrack(target.playlist.accountRef, target.playlist.id, track);
  }
  if (target.playlist.pluginId === KUGOU_PLUGIN_ID) {
    return addKugouPlaylistTrack(target.playlist.accountRef, target.playlist.id, track);
  }
  throw new Error("The selected playlist does not support adding tracks.");
}

function showTrackActionMessage(message: string) {
  trackActionMessage.value = message;
  if (trackActionMessageTimer !== null) window.clearTimeout(trackActionMessageTimer);
  trackActionMessageTimer = window.setTimeout(() => {
    trackActionMessage.value = null;
    trackActionMessageTimer = null;
  }, 5_000);
}

async function addToFavorites(track: OnlineTrack) {
  if (!supportsLibraryActions(track)) {
    globalError.value = "This track is not available on NetEase or KuGou.";
    return;
  }
  globalError.value = null;
  const library = await loadPlaylistLibrary();
  if (!library) {
    globalError.value ??= "Could not load your playlists.";
    return;
  }
  const targets = playlistTargetsForTrack(track, true);
  if (!targets.length) {
    globalError.value = "No matching My Favorite Music playlist is available.";
    return;
  }

  trackActionId.value = `favorite:${track.key}`;
  const outcomes = await Promise.allSettled(targets.map(addTrackToPlaylist));
  trackActionId.value = null;
  const succeeded = outcomes.filter((outcome) => outcome.status === "fulfilled").length;
  const errors = outcomes.flatMap((outcome) =>
    outcome.status === "rejected" ? [normalizeError(outcome.reason)] : []
  );
  if (succeeded) {
    favoriteTrackKeys.value = new Set([...favoriteTrackKeys.value, track.key]);
    favoriteTrackIdentities.value = new Set([
      ...favoriteTrackIdentities.value,
      ...targets.flatMap((target, index) =>
        outcomes[index].status === "fulfilled"
          ? [trackIdentity(target.candidate)]
          : []
      ),
    ]);
    showTrackActionMessage(
      succeeded === 1
        ? "Added to My Favorite Music."
        : `Added to ${succeeded} favorite playlists.`,
    );
  }
  if (errors.length) {
    globalError.value = errors.join(" ");
  }
}

async function openPlaylistPicker(trackOrTracks: OnlineTrack | OnlineTrack[]) {
  const tracks = Array.isArray(trackOrTracks) ? trackOrTracks : [trackOrTracks];
  if (!tracks.length) return;
  if (tracks.some((track) => !supportsLibraryActions(track))) {
    globalError.value = "One or more selected tracks are not available on NetEase or KuGou.";
    return;
  }
  globalError.value = null;
  const library = await loadPlaylistLibrary();
  if (!library) {
    globalError.value ??= "Could not load your playlists.";
    return;
  }
  const targets = playlistTargetsForTracks(tracks);
  if (!targets.length) {
    globalError.value = tracks.length === 1
      ? "No matching writable playlist is available."
      : "No writable playlist supports every selected track.";
    return;
  }
  pendingPlaylistTracks.value = [...tracks];
  selectedPlaylistTargetKey.value = targets[0].playlist.key;
}

function closePlaylistPicker() {
  if (playlistPickerActionId.value && trackActionId.value === playlistPickerActionId.value) return;
  pendingPlaylistTracks.value = [];
  selectedPlaylistTargetKey.value = "";
}

async function confirmPlaylistAdd() {
  const target = selectedPlaylistTarget.value;
  const tracks = [...pendingPlaylistTracks.value];
  const actionId = playlistPickerActionId.value;
  if (!tracks.length || !target || !actionId) return;
  trackActionId.value = actionId;
  globalError.value = null;
  const outcomes = await Promise.allSettled(
    target.candidates.map((candidate) =>
      addTrackToPlaylist({ playlist: target.playlist, candidate })
    ),
  );
  const succeeded = outcomes.filter((outcome) => outcome.status === "fulfilled").length;
  const errors = outcomes.flatMap((outcome) =>
    outcome.status === "rejected" ? [normalizeError(outcome.reason)] : []
  );
  if (succeeded) {
    showTrackActionMessage(
      tracks.length === 1
        ? `Added to ${target.playlist.name}.`
        : `Added ${succeeded} of ${tracks.length} tracks to ${target.playlist.name}.`,
    );
    pendingPlaylistTracks.value = [];
    selectedPlaylistTargetKey.value = "";
  }
  if (errors.length) globalError.value = errors.join(" ");
  trackActionId.value = null;
}

async function preloadForYou(refreshPlaylists = false) {
  if (
    !props.isActive
    || activeTab.value !== "search"
    || hasSubmittedSearch.value
    || detail.value
    || activeRecommendation.value
  ) {
    return;
  }
  await Promise.all([
    ...recommendationEntries.map((entry) => loadRecommendation(entry.id)),
    loadPlaylistLibrary(refreshPlaylists),
  ]);
}

function cancelRecommendationLoad(kind: MusicRecommendationKind) {
  const state = recommendationPreviews.value[kind];
  state.generation += 1;
  if (state.requestId) void cancelSourceRequest(state.requestId);
  state.requestId = null;
  state.loading = false;
  pendingRecommendationLoads.delete(kind);
}

function cancelRecommendationLoads() {
  for (const entry of recommendationEntries) cancelRecommendationLoad(entry.id);
  pendingRecommendationLoads.clear();
}

function appendUniqueTracks(target: OnlineTrack[], candidates: OnlineTrack[]) {
  const existingKeys = new Set(target.map((track) => track.key));
  const appended = candidates.filter((track) => {
    if (existingKeys.has(track.key)) return false;
    existingKeys.add(track.key);
    return true;
  });
  target.push(...appended);
  return appended;
}

function cancelPrivateRoamingBatchLoad() {
  privateRoamingBatchGeneration += 1;
  if (privateRoamingBatchRequestId) {
    void cancelSourceRequest(privateRoamingBatchRequestId);
  }
  privateRoamingBatchRequestId = null;
  privateRoamingBatchLoading.value = false;
  pendingPrivateRoamingBatch = null;
}

function fetchNextPrivateRoamingBatch() {
  if (pendingPrivateRoamingBatch) return pendingPrivateRoamingBatch;

  const generation = ++privateRoamingBatchGeneration;
  const requestId = `online-recommendation-roaming-next-${Date.now()}-${generation}`;
  privateRoamingBatchRequestId = requestId;
  privateRoamingBatchLoading.value = true;
  if (activeRecommendation.value === "roaming") recommendationError.value = null;

  const load = (async () => {
    try {
      const result = await getOnlineMusicRecommendations("roaming", requestId);
      if (generation !== privateRoamingBatchGeneration) return null;
      return result;
    } catch (error) {
      if (generation !== privateRoamingBatchGeneration) return null;
      const message = normalizeError(error);
      recommendationPreviews.value.roaming.error = message;
      if (activeRecommendation.value === "roaming") recommendationError.value = message;
      return null;
    } finally {
      if (generation === privateRoamingBatchGeneration) {
        privateRoamingBatchRequestId = null;
        privateRoamingBatchLoading.value = false;
      }
    }
  })();
  pendingPrivateRoamingBatch = load;
  void load.finally(() => {
    if (pendingPrivateRoamingBatch === load) pendingPrivateRoamingBatch = null;
  });
  return load;
}

async function loadNextPrivateRoamingBatch(targetQueue = recommendationTracks.value) {
  const result = await fetchNextPrivateRoamingBatch();
  if (!result) return [];

  const appended = appendUniqueTracks(targetQueue, result.items);
  const state = recommendationPreviews.value.roaming;
  const previewItems = state.result?.items ?? [];
  if (previewItems !== targetQueue) appendUniqueTracks(previewItems, result.items);
  state.result = { ...result, items: previewItems };
  state.error = recommendationResultError(state.result);
  if (activeRecommendation.value === "roaming") {
    if (recommendationTracks.value !== targetQueue && recommendationTracks.value !== previewItems) {
      appendUniqueTracks(recommendationTracks.value, result.items);
    }
    recommendationFailures.value = result.failures;
    recommendationError.value = state.error;
  }
  return appended;
}

async function openRecommendation(kind: MusicRecommendationKind, force = false) {
  abandonRecommendationRequest();
  activeRecommendation.value = kind;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  recommendationLoading.value = true;
  activeTab.value = "search";
  globalError.value = null;
  const generation = recommendationGeneration;
  const result = await loadRecommendation(kind, force);
  if (generation !== recommendationGeneration || activeRecommendation.value !== kind) return;
  if (result) applyRecommendationResult(result);
  else recommendationError.value = recommendationPreviews.value[kind].error;
  recommendationLoading.value = false;
}

function abandonRecommendationRequest() {
  recommendationGeneration += 1;
  if (activeRecommendation.value) cancelRecommendationLoad(activeRecommendation.value);
  recommendationLoading.value = false;
}

function closeRecommendation() {
  abandonRecommendationRequest();
  activeRecommendation.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  void preloadForYou();
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = 0;
  });
}

async function submitSearch(suggestion?: string) {
  const keyword = (suggestion ?? query.value).trim();
  if (!keyword) return;
  query.value = keyword;
  suggestionsOpen.value = false;
  abandonRecommendationRequest();
  activeRecommendation.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
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
  const loadMore = activeRecommendation.value === "roaming" && appendable
    ? () => loadNextPrivateRoamingBatch(currentQueue)
    : undefined;
  emit("playRequest", currentTrack, currentQueue, queueIndex, appendable, loadMore);
}

function requestTrackPlayback(track: OnlineTrack, queue: OnlineTrack[]) {
  if (
    props.activeOnlineTrackKey === track.key &&
    props.resolvingOnlineTrackKey !== track.key
  ) {
    emit("togglePlayback");
    return;
  }
  const privateRoaming = activeRecommendation.value === "roaming";
  void playTrack(track, queue, privateRoaming);
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

async function playAllRecommendation() {
  if (!recommendationTracks.value.length) return;
  const queue = activeRecommendation.value === "roaming"
    ? recommendationTracks.value
    : [...recommendationTracks.value];
  await playTrack(queue[0], queue, true);
}

async function downloadAllRecommendation() {
  if (!recommendationTracks.value.length || !activeRecommendationEntry.value) return;
  await createDownload(
    "recommendation",
    activeRecommendationEntry.value.label,
    [...recommendationTracks.value],
  );
}

async function downloadTrack(track: OnlineTrack) {
  await createDownload("track", track.title, [track]);
}

async function downloadTracks(tracks: OnlineTrack[]) {
  if (!tracks.length) return;
  await createDownload(
    tracks.length === 1 ? "track" : "selection",
    tracks.length === 1 ? tracks[0].title : `${tracks.length} selected tracks`,
    [...tracks],
  );
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

function showHome() {
  activeTab.value = "search";
  query.value = "";
  submittedQuery.value = "";
  suggestions.value = [];
  suggestionsOpen.value = false;
  suggestionLoading.value = false;
  suggestionGeneration += 1;
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  suggestionTimer = null;
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  suggestionRequestId = null;

  searchGeneration += 1;
  if (searchId.value) void cancelSourceRequest(searchId.value);
  searchId.value = null;
  pendingSearchEvents = [];
  expandedSection.value = null;
  sectionStates.value = newSectionStates();

  detailAppendGeneration.value += 1;
  detailRequestGeneration += 1;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  detailRequestId = null;
  detail.value = null;
  detailTracks.value = [];
  detailLoading.value = false;
  detailLoadingMore.value = false;
  detailHasMore.value = false;

  abandonRecommendationRequest();
  activeRecommendation.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  globalError.value = null;
  pendingPlaylistTracks.value = [];
  selectedPlaylistTargetKey.value = "";
  trackActionId.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  void preloadForYou();
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = 0;
  });
}

defineExpose({
  addToFavorites,
  downloadTrack,
  isDownloadActionPending,
  isTrackActionPending,
  isTrackFavorite,
  openPlaylistPicker,
  showHome,
});

</script>

<template>
  <div class="flex w-full min-w-0 flex-col gap-4">
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
    <div v-if="trackActionMessage" role="status" class="alert alert-success py-2 text-sm">
      <ListPlus :size="16" aria-hidden="true" />
      {{ trackActionMessage }}
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
        :track-action-id="trackActionId"
        :supports-library-actions="supportsLibraryActions"
        :supports-playlist-selection="supportsPlaylistSelection"
        :is-favorite="isTrackFavorite"
        @play="playTrack($event, detailTracks)"
        @download="downloadTrack"
        @download-selection="downloadTracks"
        @favorite="addToFavorites"
        @add-to-playlist="openPlaylistPicker"
        @add-selection-to-playlist="openPlaylistPicker"
      />
      <div v-if="detailHasMore" class="flex justify-center py-4">
        <button class="btn btn-sm" type="button" :disabled="detailLoadingMore" @click="loadMoreDetail">
          <span v-if="detailLoadingMore" class="loading loading-spinner loading-xs"></span>
          Load more
        </button>
      </div>
    </div>

    <div v-else-if="activeRecommendationEntry" data-online-recommendation class="min-w-0">
      <div class="mb-3 flex min-w-0 flex-wrap items-center gap-3 border-b border-base-300 pb-3">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          aria-label="Back to Online Music home"
          title="Back"
          @click="closeRecommendation"
        >
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <div class="flex size-12 shrink-0 items-center justify-center rounded bg-base-200">
          <component :is="activeRecommendationEntry.icon" :size="22" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ activeRecommendationEntry.label }}</h2>
          <div class="mt-1 flex flex-wrap gap-1">
            <span
              v-for="provider in activeRecommendationEntry.providers"
              :key="provider.pluginId"
              class="badge badge-sm"
            >
              {{ provider.label }}
            </span>
            <span v-if="recommendationFailures.length && recommendationTracks.length" class="badge badge-warning badge-sm">
              Partial
            </span>
          </div>
        </div>
        <div class="ml-auto flex basis-full justify-end gap-1 sm:basis-auto">
          <button
            v-if="activeRecommendationEntry.id !== 'roaming'"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :disabled="recommendationLoading"
            aria-label="Refresh recommendations"
            title="Refresh recommendations"
            @click="openRecommendation(activeRecommendationEntry.id, true)"
          >
            <RefreshCw
              :class="{ 'animate-spin': recommendationLoading }"
              :size="16"
              aria-hidden="true"
            />
          </button>
          <button
            v-if="recommendationTracks.length"
            class="btn btn-sm"
            type="button"
            @click="playAllRecommendation"
          >
            <Play :size="15" aria-hidden="true" />
            Play all
          </button>
          <button
            v-if="recommendationTracks.length"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            aria-label="Download all recommendations"
            title="Download all"
            @click="downloadAllRecommendation"
          >
            <Download :size="15" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div v-if="recommendationError" role="alert" class="alert alert-error mb-3 py-2">
        <AlertCircle :size="17" aria-hidden="true" />
        <span class="min-w-0 flex-1 text-sm">{{ recommendationError }}</span>
        <div v-if="!recommendationTracks.length" class="flex shrink-0 flex-wrap gap-1">
          <button
            v-for="provider in activeRecommendationEntry.providers"
            :key="provider.pluginId"
            class="btn btn-sm"
            type="button"
            @click="emit('openPlugin', provider.pluginId)"
          >
            Open {{ provider.label }}
          </button>
        </div>
      </div>
      <div
        v-else-if="recommendationFailures.length && recommendationTracks.length"
        role="status"
        class="alert alert-warning mb-3 py-2 text-sm"
      >
        <AlertCircle :size="17" aria-hidden="true" />
        {{ recommendationFailures.map((failure) => failure.channelName).join(', ') }} unavailable
      </div>

      <div v-if="recommendationLoading" class="space-y-2">
        <div v-for="index in 8" :key="index" class="skeleton h-11 w-full"></div>
      </div>
      <OnlineTrackTable
        v-else-if="recommendationTracks.length"
        :tracks="recommendationTracks"
        :active-key="activeOnlineTrackKey"
        :track-action-id="trackActionId"
        :supports-library-actions="supportsLibraryActions"
        :supports-playlist-selection="supportsPlaylistSelection"
        :is-favorite="isTrackFavorite"
        @play="requestTrackPlayback($event, recommendationTracks)"
        @download="downloadTrack"
        @download-selection="downloadTracks"
        @favorite="addToFavorites"
        @add-to-playlist="openPlaylistPicker"
        @add-selection-to-playlist="openPlaylistPicker"
      />
      <div
        v-if="activeRecommendationEntry.id === 'roaming' && recommendationTracks.length"
        class="flex justify-center border-t border-base-300 py-4"
      >
        <button
          data-private-roaming-next
          class="btn btn-sm"
          type="button"
          :disabled="privateRoamingBatchLoading"
          aria-label="Load next private roaming batch"
          @click="loadNextPrivateRoamingBatch()"
        >
          <span
            v-if="privateRoamingBatchLoading"
            class="loading loading-spinner loading-xs"
            aria-hidden="true"
          ></span>
          <ListPlus v-else :size="16" aria-hidden="true" />
          Load next songs
        </button>
      </div>
      <div v-else-if="!recommendationError" class="flex min-h-48 items-center justify-center text-sm text-base-content/55">
        No recommendations available
      </div>
    </div>

    <div v-else-if="!hasSubmittedSearch" data-online-home class="min-h-64 py-2">
      <div class="mb-3 flex items-center gap-2">
        <House :size="17" aria-hidden="true" />
        <h2 class="text-sm font-semibold">For you</h2>
      </div>
      <div class="grid grid-cols-1 gap-3 md:grid-cols-3">
        <button
          v-for="entry in recommendationEntries"
          :key="entry.id"
          data-online-recommendation-entry
          class="card card-border card-sm group relative isolate h-36 w-full overflow-hidden bg-base-100 text-left transition-colors hover:bg-base-200/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          type="button"
          :aria-label="entry.label"
          @click="openRecommendation(entry.id)"
        >
          <img
            v-if="recommendationCoverUrl(entry.id)"
            :src="recommendationCoverUrl(entry.id) ?? undefined"
            class="absolute inset-0 size-full object-cover transition-transform duration-300 group-hover:scale-105"
            alt=""
            decoding="async"
          />
          <div
            v-if="recommendationCoverUrl(entry.id)"
            class="absolute inset-0 bg-neutral/60 transition-colors group-hover:bg-neutral/50"
            aria-hidden="true"
          ></div>
          <div class="card-body relative z-10 w-full gap-3">
            <div class="flex items-start justify-between gap-3">
              <div
                class="flex size-10 shrink-0 items-center justify-center rounded"
                :class="recommendationCoverUrl(entry.id)
                  ? 'bg-base-100/20 text-neutral-content backdrop-blur-sm'
                  : 'bg-base-200'"
              >
                <component :is="entry.icon" :size="20" aria-hidden="true" />
              </div>
              <ChevronRight
                :size="18"
                class="mt-2 shrink-0"
                :class="recommendationCoverUrl(entry.id)
                  ? 'text-neutral-content/75'
                  : 'text-base-content/45'"
                aria-hidden="true"
              />
            </div>
            <div class="min-w-0">
              <h3
                class="card-title text-base"
                :class="{ 'text-neutral-content': recommendationCoverUrl(entry.id) }"
              >
                {{ entry.label }}
              </h3>
              <div class="mt-2 flex flex-wrap gap-1">
                <span
                  v-for="provider in entry.providers"
                  :key="provider.pluginId"
                  class="badge badge-sm"
                  :class="{ 'border-neutral-content/25 bg-neutral/50 text-neutral-content': recommendationCoverUrl(entry.id) }"
                >
                  {{ provider.label }}
                </span>
              </div>
            </div>
          </div>
        </button>
      </div>

      <section data-online-playlists class="mt-6 min-w-0 border-t border-base-300 pt-4">
        <div class="mb-3 flex min-w-0 flex-wrap items-center gap-2">
          <ListMusic :size="17" aria-hidden="true" />
          <h2 class="text-sm font-semibold">Playlists</h2>
          <span v-if="playlistLibraryItems.length" class="text-xs tabular-nums text-base-content/50">
            {{ playlistLibraryItems.length }} loaded
          </span>
          <span
            v-if="playlistLibraryFailures.length && playlistLibraryItems.length"
            class="badge badge-warning badge-sm ml-auto"
          >
            Partial
          </span>
          <button
            class="btn btn-square btn-ghost btn-xs"
            :class="{ 'ml-auto': !playlistLibraryFailures.length || !playlistLibraryItems.length }"
            type="button"
            :disabled="playlistLibraryLoading"
            aria-label="Refresh playlists"
            title="Refresh playlists"
            @click="loadPlaylistLibrary(true)"
          >
            <RefreshCw
              :class="{ 'animate-spin': playlistLibraryLoading }"
              :size="14"
              aria-hidden="true"
            />
          </button>
        </div>

        <div v-if="playlistLibraryLoading && !playlistLibraryItems.length" class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <div v-for="index in 6" :key="index" class="card card-border card-sm h-24">
            <div class="card-body flex-row items-center gap-3">
              <div class="skeleton size-14 shrink-0"></div>
              <div class="min-w-0 flex-1 space-y-2">
                <div class="skeleton h-4 w-3/4"></div>
                <div class="skeleton h-3 w-1/2"></div>
              </div>
            </div>
          </div>
        </div>

        <div
          v-else-if="playlistLibraryError && !playlistLibraryItems.length"
          role="alert"
          class="alert alert-error py-2"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-sm">{{ playlistLibraryError }}</span>
          <button class="btn btn-sm" type="button" @click="loadPlaylistLibrary(true)">
            <RefreshCw :size="14" aria-hidden="true" />
            Retry
          </button>
        </div>

        <div
          v-else-if="playlistLibraryFailures.length && !playlistLibraryItems.length"
          role="status"
          class="alert py-2"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-sm">
            Connect NetEase or KuGou to load your playlists.
          </span>
          <div class="flex shrink-0 flex-wrap gap-1">
            <button
              v-for="provider in failedPlaylistProviders"
              :key="provider.pluginId"
              class="btn btn-sm"
              type="button"
              @click="emit('openPlugin', provider.pluginId)"
            >
              Open {{ provider.label }}
            </button>
          </div>
        </div>

        <div
          v-else-if="playlistLibraryResult?.supportedChannels === 0"
          role="status"
          class="alert py-2"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-sm">
            Enable NetEase or KuGou to load your playlists.
          </span>
          <div class="flex shrink-0 flex-wrap gap-1">
            <button
              v-for="provider in playlistProviders"
              :key="provider.pluginId"
              class="btn btn-sm"
              type="button"
              @click="emit('openPlugin', provider.pluginId)"
            >
              Open {{ provider.label }}
            </button>
          </div>
        </div>

        <div
          v-else-if="!playlistLibraryItems.length"
          class="flex min-h-24 items-center text-sm text-base-content/50"
        >
          No playlists found
        </div>

        <div v-else class="space-y-5">
          <section
            v-for="provider in playlistProviderSections"
            :key="provider.pluginId"
            :data-online-playlist-provider="provider.pluginId"
            class="min-w-0"
          >
            <div class="mb-2 flex items-center gap-2 border-b border-base-300 pb-2">
              <h3 class="text-sm font-medium">{{ provider.label }}</h3>
              <span class="text-xs tabular-nums text-base-content/50">
                {{ provider.items.length }}
                {{ provider.items.length === 1 ? "playlist" : "playlists" }}
              </span>
            </div>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <button
                v-for="playlist in provider.items"
                :key="playlist.key"
                class="card card-border card-sm min-h-24 w-full bg-base-100 text-left transition-colors hover:bg-base-200/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                type="button"
                :aria-label="`Open playlist ${playlist.name}`"
                @click="openDetail({ kind: 'playlist', entity: playlist })"
              >
                <div class="card-body flex-row items-center gap-3">
                  <div class="flex size-14 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
                    <img
                      v-if="playlist.coverUrl"
                      :src="playlist.coverUrl"
                      class="size-full object-cover"
                      alt=""
                      decoding="async"
                    />
                    <ListMusic v-else :size="22" aria-hidden="true" />
                  </div>
                  <div class="min-w-0 flex-1">
                    <h4 class="truncate text-sm font-medium">{{ playlist.name }}</h4>
                    <p v-if="playlist.ownerName" class="mt-1 truncate text-xs text-base-content/55">
                      {{ playlist.ownerName }}
                    </p>
                    <p
                      v-if="playlist.trackCount !== null"
                      class="mt-2 truncate text-xs tabular-nums text-base-content/45"
                    >
                      {{ playlist.trackCount }} tracks
                    </p>
                  </div>
                  <ChevronRight :size="17" class="shrink-0 text-base-content/40" aria-hidden="true" />
                </div>
              </button>
            </div>
          </section>
        </div>

        <div
          v-if="playlistLibraryError && playlistLibraryItems.length"
          role="alert"
          class="alert alert-error mt-3 py-2 text-sm"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1">{{ playlistLibraryError }}</span>
          <button class="btn btn-sm" type="button" @click="loadPlaylistLibrary(true)">
            <RefreshCw :size="14" aria-hidden="true" />
            Retry
          </button>
        </div>

        <div
          v-if="playlistLibraryFailures.length && playlistLibraryItems.length"
          role="status"
          class="alert alert-warning mt-3 py-2 text-sm"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1">
            {{ playlistLibraryFailures.map((failure) => failure.channelName).join(', ') }} unavailable
          </span>
          <button
            v-for="provider in failedPlaylistProviders"
            :key="provider.pluginId"
            class="btn btn-sm"
            type="button"
            @click="emit('openPlugin', provider.pluginId)"
          >
            Open {{ provider.label }}
          </button>
        </div>
      </section>
    </div>

    <div v-else data-online-results class="flex flex-col gap-5">

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
          :track-action-id="trackActionId"
          :supports-library-actions="supportsLibraryActions"
          :supports-playlist-selection="supportsPlaylistSelection"
          :is-favorite="isTrackFavorite"
          @play="requestTrackPlayback($event, sectionItems<OnlineTrack>('songs'))"
          @download="downloadTrack"
          @download-selection="downloadTracks"
          @favorite="addToFavorites"
          @add-to-playlist="openPlaylistPicker"
          @add-selection-to-playlist="openPlaylistPicker"
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

    <Teleport to="body" :disabled="isActive">
      <dialog
        v-if="pendingPlaylistTracks.length"
        open
        tabindex="0"
        class="modal"
        aria-labelledby="online-playlist-picker-title"
        @cancel.prevent="closePlaylistPicker"
      >
        <form class="modal-box max-w-lg" data-online-playlist-picker @submit.prevent="confirmPlaylistAdd">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">
              <h2 id="online-playlist-picker-title" class="text-base font-semibold">Add to Playlist</h2>
              <p class="mt-1 truncate text-sm text-base-content/65">
                {{ pendingPlaylistTracks.length === 1
                  ? `${pendingPlaylistTracks[0].title} · ${pendingPlaylistTracks[0].artist}`
                  : `${pendingPlaylistTracks.length} selected tracks` }}
              </p>
            </div>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              :disabled="trackActionId === playlistPickerActionId"
              aria-label="Close playlist picker"
              @click="closePlaylistPicker"
            >
              <X :size="17" aria-hidden="true" />
            </button>
          </div>

          <ul class="menu mt-4 max-h-72 w-full overflow-y-auto rounded border border-base-300 p-1">
            <li v-for="target in pendingPlaylistTargets" :key="target.playlist.key">
              <button
                type="button"
                :class="{ 'menu-active': selectedPlaylistTargetKey === target.playlist.key }"
                :aria-pressed="selectedPlaylistTargetKey === target.playlist.key"
                @click="selectedPlaylistTargetKey = target.playlist.key"
              >
                <span class="min-w-0 flex-1 text-left">
                  <span class="block truncate text-sm">{{ target.playlist.name }}</span>
                  <span class="block truncate text-xs opacity-60">
                    {{ target.playlist.ownerName || target.playlist.channelName }}
                  </span>
                </span>
                <span class="badge badge-ghost badge-sm shrink-0">{{ target.playlist.channelName }}</span>
              </button>
            </li>
          </ul>

          <div class="modal-action">
            <button
              class="btn btn-ghost btn-sm"
              type="button"
              :disabled="trackActionId === playlistPickerActionId"
              @click="closePlaylistPicker"
            >
              Cancel
            </button>
            <button
              class="btn btn-primary btn-sm"
              type="submit"
              :disabled="!selectedPlaylistTarget || trackActionId === playlistPickerActionId"
            >
              <RefreshCw v-if="trackActionId === playlistPickerActionId" class="animate-spin" :size="15" aria-hidden="true" />
              <ListPlus v-else :size="15" aria-hidden="true" />
              Add
            </button>
          </div>
        </form>
        <form method="dialog" class="modal-backdrop" @submit.prevent="closePlaylistPicker">
          <button type="submit" :disabled="trackActionId === playlistPickerActionId">Close</button>
        </form>
      </dialog>
    </Teleport>
  </div>
</template>
