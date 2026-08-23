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
  History,
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
import OnlineCommentsDialog from "./OnlineCommentsDialog.vue";
import OnlineTrackTable from "./OnlineTrackTable.vue";
import {
  centerElementInScrollViewport,
  useTrackListFollow,
  type TrackListScrollBehavior,
} from "../composables/use-track-list-follow";
import { cancelSourceRequest } from "../lib/plugin-api";
import type { RemoteTrack } from "../lib/plugin-api";
import { normalizeError } from "../lib/errors";
import {
  cancelOnlineDownloadTask,
  createOnlineDownloadTask,
  getOnlineAlbumTracks,
  getOnlineArtistAlbums,
  getOnlineArtistBiography,
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
  listenOnlineDownloadProgress,
  listenOnlineDownloadTasks,
  listenOnlineMusicSearch,
  onlinePlaylistDetailError,
  onlineTracksMatch,
  pauseOnlineDownloadTask,
  refreshOnlineDownloadItemCandidates,
  retryOnlineDownloadItem,
  selectOnlineDownloadDirectory,
  splitOnlineArtistNames,
  startOnlineDownloadTask,
  startOnlineMusicSearch,
  updateOnlineMusicSettings,
  type OnlineAlbum,
  type OnlineAlbumPage,
  type OnlineArtist,
  type OnlineArtistBiography,
  type OnlineMusicSettings,
  type OnlineDownloadProgressEvent,
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
import { onlineTrackSupportsComments } from "../lib/online-comment-api";
import { formatNumber, t } from "../i18n";
import { shortcutAriaKeys } from "../lib/keyboard-shortcuts";

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

type ArtistDetailTab = "topTracks" | "albums" | "biography";

type ArtistDetailHistory = {
  detail: Extract<DetailState, { kind: "artist" }>;
  tracks: OnlineTrack[];
  tracksLoading: boolean;
  page: number;
  hasMore: boolean;
  retryAvailable: boolean;
  error: string | null;
  loginRequiredPluginId: string | null;
  tab: ArtistDetailTab;
  albums: OnlineAlbum[];
  albumsLoaded: boolean;
  albumsError: string | null;
  albumsPage: number;
  albumsHasMore: boolean;
  biography: OnlineArtistBiography | null;
  biographyLoaded: boolean;
  biographyError: string | null;
  scrollPosition: number;
};

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

type DownloadFlightOrigin = {
  artwork: HTMLElement;
  bounds: Pick<DOMRect, "height" | "left" | "top" | "width">;
  trackKey: string;
};

type DownloadFlightAnimation = {
  animation: Animation;
  artwork: HTMLElement;
};

const FAVORITE_TRACK_PAGE_SIZE = 200;
const MAX_FAVORITE_TRACK_PAGES = 100;
const SEARCH_SUMMARY_SIZE = 5;

const props = defineProps<{
  isActive: boolean;
  audioSources: AudioSourceRecord[];
  selectedAudioSourceId: string;
  activeOnlineTrack: OnlineTrack | null;
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
  queueTracks: [tracks: OnlineTrack[], placement: "next" | "last"];
  openAudioSources: [];
  downloadCompleted: [destination: string];
  togglePlayback: [];
  openPlugin: [pluginId: string];
  addToCollection: [tracks: OnlineTrack[]];
  createCollection: [tracks: OnlineTrack[]];
}>();

function handleQueueTracks(tracks: OnlineTrack[], placement: "next" | "last") {
  emit("queueTracks", tracks, placement);
}

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
const searchInput = ref<HTMLInputElement | null>(null);
const workspaceRoot = ref<HTMLElement | null>(null);
const searchArea = ref<HTMLElement | null>(null);
const mainScrollViewport = ref<HTMLElement | null>(null);
const downloadsTab = ref<HTMLButtonElement | null>(null);
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
const artistDetailTab = ref<ArtistDetailTab>("topTracks");
const artistAlbums = ref<OnlineAlbum[]>([]);
const artistAlbumsLoading = ref(false);
const artistAlbumsLoadingMore = ref(false);
const artistAlbumsLoaded = ref(false);
const artistAlbumsError = ref<string | null>(null);
const artistAlbumsPage = ref(1);
const artistAlbumsHasMore = ref(false);
const artistBiography = ref<OnlineArtistBiography | null>(null);
const artistBiographyLoading = ref(false);
const artistBiographyLoaded = ref(false);
const artistBiographyError = ref<string | null>(null);
const artistDetailHistory = ref<ArtistDetailHistory | null>(null);
const downloadTasks = ref<OnlineDownloadTask[]>([]);
const downloadActionId = ref<string | null>(null);
const settings = ref<OnlineMusicSettings | null>(null);
const sectionStates = ref<Record<OnlineSearchSection, SectionState>>(newSectionStates());
const resultScrollPosition = ref(0);
const summaryScrollPosition = ref(0);
const activeRecommendation = ref<MusicRecommendationKind | null>(null);
const recommendationTracks = ref<OnlineTrack[]>([]);
const selectedRecommendationProviderId = ref<string | null>(null);
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
const entityActionId = ref<string | null>(null);
const trackActionMessage = ref<string | null>(null);
const favoriteTrackKeys = ref<Set<string>>(new Set());
const favoriteTrackIdentities = ref<Set<string>>(new Set());
const optimisticFavoriteTrackIdentities = ref<Set<string>>(new Set());
const commentTrack = ref<OnlineTrack | null>(null);

let unlistenSearch: (() => void) | null = null;
let unlistenDownloads: (() => void) | null = null;
let unlistenDownloadProgress: (() => void) | null = null;
let unlistenDownloadCompletions: (() => void) | null = null;
let searchGeneration = 0;
let sectionLoadGeneration = 0;
let suggestionGeneration = 0;
let detailRequestGeneration = 0;
let suggestionTimer: number | null = null;
let suggestionRequestId: string | null = null;
let detailRequestId: string | null = null;
let artistAlbumsRequestId: string | null = null;
let artistBiographyRequestId: string | null = null;
let recommendationGeneration = 0;
let privateRoamingBatchGeneration = 0;
let privateRoamingBatchRequestId: string | null = null;
let pendingPrivateRoamingBatch: Promise<OnlineRecommendationsResult | null> | null = null;
let playlistLibraryGeneration = 0;
let playlistLibraryRequestId: string | null = null;
let pendingPlaylistLibraryLoad: Promise<OnlinePlaylistsResult | null> | null = null;
let favoriteTrackLoadGeneration = 0;
let favoriteTrackLoadSignature: string | null = null;
let pendingFavoriteTrackLoadSignature: string | null = null;
let pendingFavoriteTrackLoad: Promise<void> | null = null;
const favoriteTrackRequestIds = new Set<string>();
let entityResolveRequestId: string | null = null;
let entityResolveGeneration = 0;
let trackActionMessageTimer: number | null = null;
let pendingSearchEvents: OnlineSearchSectionEvent[] = [];
let listEntryGeneration = 0;
let pendingListEntryGeneration: number | null = null;
const pendingRecommendationLoads = new Map<
  MusicRecommendationKind,
  Promise<OnlineRecommendationsResult | null>
>();
const downloadFlightAnimations = new Set<DownloadFlightAnimation>();

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
const visibleDetailSubtitle = computed(() => {
  if (!detail.value) return "";
  if (detail.value.kind === "artist") {
    const label = {
      topTracks: "Top tracks",
      albums: "All albums",
      biography: "Artist bio",
    }[artistDetailTab.value];
    return t(label);
  }
  if (detail.value.kind === "album") return detail.value.entity.artist;
  return detail.value.entity.ownerName || detail.value.entity.channelName;
});
const detailBackLabel = computed(() => {
  if (detail.value?.kind === "album" && artistDetailHistory.value) {
    return t("Back to {name}", { name: artistDetailHistory.value.detail.entity.name });
  }
  return t("Back to search results");
});
const isArtistTopTracksTab = computed(() =>
  detail.value?.kind !== "artist" || artistDetailTab.value === "topTracks"
);
const activeRecommendationEntry = computed(() =>
  recommendationEntries.find((entry) => entry.id === activeRecommendation.value) ?? null,
);
const selectedRecommendationProvider = computed(() =>
  activeRecommendationEntry.value?.providers.find(
    (provider) => provider.pluginId === selectedRecommendationProviderId.value,
  ) ?? null,
);
const visibleRecommendationTracks = computed(() => {
  const pluginId = selectedRecommendationProvider.value?.pluginId;
  if (!pluginId) return recommendationTracks.value;
  return recommendationTracks.value
    .filter((track) => track.candidates.some((candidate) => candidate.pluginId === pluginId))
    .sort((left, right) =>
      recommendationProviderRank(left, pluginId) - recommendationProviderRank(right, pluginId)
    );
});
const visibleRecommendationFailures = computed(() => {
  const provider = selectedRecommendationProvider.value;
  if (!provider) return recommendationFailures.value;
  return recommendationFailures.value.filter((failure) =>
    recommendationFailureMatchesProvider(failure, provider)
  );
});
const visibleRecommendationError = computed(() => {
  if (!selectedRecommendationProvider.value) return recommendationError.value;
  if (visibleRecommendationTracks.value.length) return null;
  const result = activeRecommendation.value
    ? recommendationPreviews.value[activeRecommendation.value].result
    : null;
  if (result?.supportedChannels === 0) return recommendationError.value;
  return visibleRecommendationFailures.value[0]?.message ?? null;
});
const visibleRecommendationProviders = computed(() =>
  selectedRecommendationProvider.value
    ? [selectedRecommendationProvider.value]
    : activeRecommendationEntry.value?.providers ?? [],
);
const visibleSongListKey = computed(() => {
  if (activeTab.value !== "search") return null;
  if (detail.value) {
    return isArtistTopTracksTab.value
      ? `detail:${detail.value.kind}:${detail.value.entity.key}`
      : null;
  }
  if (activeRecommendation.value) {
    return `recommendation:${activeRecommendation.value}:${selectedRecommendationProviderId.value ?? "all"}`;
  }
  if (!hasSubmittedSearch.value || (expandedSection.value && expandedSection.value !== "songs")) {
    return null;
  }
  return `search:${submittedQuery.value}:${expandedSection.value ?? "summary"}`;
});
const visibleSongTracks = computed(() => {
  if (detail.value) return isArtistTopTracksTab.value ? detailTracks.value : [];
  if (activeRecommendation.value) return visibleRecommendationTracks.value;
  if (hasSubmittedSearch.value && (!expandedSection.value || expandedSection.value === "songs")) {
    return sectionItems<OnlineTrack>("songs");
  }
  return [];
});
const visibleSongListLoading = computed(() => {
  if (detail.value) return isArtistTopTracksTab.value && detailLoading.value;
  if (activeRecommendation.value) return recommendationLoading.value;
  if (hasSubmittedSearch.value && (!expandedSection.value || expandedSection.value === "songs")) {
    const state = sectionStates.value.songs;
    return state.loading || state.loadingMore;
  }
  return false;
});
const songListFollow = useTrackListFollow({
  viewport: mainScrollViewport,
  locate: locateVisiblePlayingTrack,
  isActive: () => props.isActive && visibleSongListKey.value !== null,
});
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
  mainScrollViewport.value = workspaceRoot.value?.closest("main") ?? null;
  window.addEventListener("pointerdown", handleWindowPointerDown);
  [
    unlistenSearch,
    unlistenDownloads,
    unlistenDownloadProgress,
    unlistenDownloadCompletions,
  ] = await Promise.all([
    listenOnlineMusicSearch(onSearchSection),
    listenOnlineDownloadTasks(upsertDownloadTask),
    listenOnlineDownloadProgress(applyDownloadProgress),
    listenOnlineDownloadCompletions((task) => {
      completionMessage.value = t("{title}: {count} downloaded", {
        title: task.title,
        count: task.completedItems,
      });
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
      mainScrollViewport.value = workspaceRoot.value?.closest("main") ?? null;
      if (visibleSongListKey.value === null) void preloadForYou(true);
      else void loadPlaylistLibrary();
      const entryGeneration = beginVisibleSongListEntry();
      void finishVisibleSongListEntry(entryGeneration);
    } else {
      commentTrack.value = null;
      if (playlistLibraryRequestId) cancelPlaylistLibraryLoad();
      cancelFavoriteTrackLoad();
      cancelTrackEntityResolution();
      songListFollow.cancelPending();
    }
  },
);

watch(visibleSongListKey, (listKey) => {
  if (listKey !== null && props.isActive) void loadPlaylistLibrary();
});

watch(
  () => props.activeOnlineTrack,
  (track, previousTrack) => {
    if (
      !track
      || (previousTrack && onlineTracksMatch(track, previousTrack))
      || !props.isActive
      || visibleSongListKey.value === null
    ) {
      return;
    }
    void songListFollow.followTrackChange();
  },
  { flush: "post" },
);

watch(visibleSongListLoading, (loading) => {
  if (!loading && pendingListEntryGeneration !== null) {
    void finishVisibleSongListEntry(pendingListEntryGeneration);
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", handleWindowPointerDown);
  unlistenSearch?.();
  unlistenDownloads?.();
  unlistenDownloadProgress?.();
  unlistenDownloadCompletions?.();
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  for (const flight of downloadFlightAnimations) {
    flight.animation.cancel();
    flight.artwork.remove();
  }
  downloadFlightAnimations.clear();
  cancelTrackEntityResolution();
  cancelArtistDetailRequests();
  cancelRecommendationLoads();
  cancelPlaylistLibraryLoad();
  cancelFavoriteTrackLoad();
  cancelPrivateRoamingBatchLoad();
  if (searchId.value) void cancelSourceRequest(searchId.value);
  if (trackActionMessageTimer !== null) window.clearTimeout(trackActionMessageTimer);
});

function beginVisibleSongListEntry() {
  if (visibleSongListKey.value === null) {
    pendingListEntryGeneration = null;
    songListFollow.cancelPending();
    return null;
  }
  const generation = ++listEntryGeneration;
  pendingListEntryGeneration = generation;
  songListFollow.beginEntry();
  return generation;
}

async function finishVisibleSongListEntry(generation: number | null) {
  if (generation === null || generation !== pendingListEntryGeneration) {
    return;
  }
  await nextTick();
  if (
    generation !== pendingListEntryGeneration
    || !props.isActive
    || visibleSongListKey.value === null
    || visibleSongListLoading.value
  ) {
    return;
  }
  pendingListEntryGeneration = null;
  await songListFollow.locateEntry();
}

async function locateVisiblePlayingTrack(
  behavior: TrackListScrollBehavior,
  isCurrent: () => boolean,
) {
  const activeTrack = props.activeOnlineTrack;
  const viewport = mainScrollViewport.value;
  const root = workspaceRoot.value;
  if (!activeTrack || !viewport || !root) {
    return false;
  }
  const tracks = visibleSongTracks.value;
  const target = tracks.find((track) => track.key === activeTrack.key)
    ?? tracks.find((track) => onlineTracksMatch(track, activeTrack));
  if (!target || !isCurrent()) {
    return false;
  }
  await nextTick();
  if (!isCurrent()) {
    return false;
  }
  const row = [...root.querySelectorAll<HTMLElement>("[data-online-track-key]")]
    .find((element) => element.dataset.onlineTrackKey === target.key);
  if (!row) {
    return false;
  }
  centerElementInScrollViewport(viewport, row, behavior);
  return true;
}

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
  const keyword = query.value.trim();
  if (keyword.length !== 1) {
    requestSuggestions(keyword, keyword ? 300 : 0);
    return;
  }

  suggestionsOpen.value = false;
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  suggestionTimer = null;
  suggestionRequestId = null;
  suggestionGeneration += 1;
  suggestions.value = [];
  suggestionLoading.value = false;
}

function openSuggestions() {
  const keyword = query.value.trim();
  if (!keyword) {
    suggestionsOpen.value = true;
    if (!suggestionLoading.value && !suggestions.value.length) {
      requestSuggestions(keyword);
    }
    return;
  }
  suggestionsOpen.value = keyword.length >= 2;
}

function focusSearch() {
  searchInput.value?.focus();
  searchInput.value?.select();
  openSuggestions();
}

function handleWindowPointerDown(event: PointerEvent) {
  if (event.target instanceof Node && searchArea.value?.contains(event.target)) return;
  suggestionsOpen.value = false;
}

function requestSuggestions(keyword: string, delayMs = 0) {
  if (suggestionTimer !== null) window.clearTimeout(suggestionTimer);
  if (suggestionRequestId) void cancelSourceRequest(suggestionRequestId);
  suggestionTimer = null;
  suggestionRequestId = null;
  const generation = ++suggestionGeneration;
  suggestionsOpen.value = true;
  suggestionLoading.value = true;

  const load = async () => {
    suggestionTimer = null;
    const requestId = keyword
      ? `online-suggest-${Date.now()}-${generation}`
      : undefined;
    suggestionRequestId = requestId ?? null;
    try {
      const result = await getOnlineMusicSuggestions(keyword, requestId);
      if (generation === suggestionGeneration) suggestions.value = result.suggestions;
    } catch {
      if (generation === suggestionGeneration) suggestions.value = [];
    } finally {
      if (generation === suggestionGeneration) suggestionLoading.value = false;
      if (requestId && suggestionRequestId === requestId) suggestionRequestId = null;
    }
  };

  if (delayMs) {
    suggestionTimer = window.setTimeout(() => void load(), delayMs);
  } else {
    void load();
  }
}

function recommendationCoverUrl(kind: MusicRecommendationKind) {
  return recommendationPreviews.value[kind].result?.items[0]?.coverUrl ?? null;
}

function recommendationResultError(result: OnlineRecommendationsResult) {
  if (result.supportedChannels === 0) {
    return t("No enabled channel supports this recommendation.");
  }
  if (!result.items.length) {
    return result.failures[0]?.message ?? t("No recommendations available.");
  }
  return null;
}

function applyRecommendationResult(result: OnlineRecommendationsResult) {
  recommendationTracks.value = result.items;
  recommendationFailures.value = result.failures;
  recommendationError.value = recommendationResultError(result);
}

function recommendationProviderRank(track: OnlineTrack, pluginId: string) {
  return Math.min(
    ...track.candidates
      .filter((candidate) => candidate.pluginId === pluginId)
      .map((candidate) => candidate.rank),
  );
}

function recommendationFailureMatchesProvider(
  failure: OnlineRecommendationsResult["failures"][number],
  provider: RecommendationEntry["providers"][number],
) {
  return failure.channelId === provider.pluginId
    || failure.channelId.startsWith(`${provider.pluginId}:`)
    || failure.channelName.toLocaleLowerCase().includes(provider.label.toLocaleLowerCase());
}

async function toggleRecommendationProvider(pluginId: string) {
  selectedRecommendationProviderId.value = selectedRecommendationProviderId.value === pluginId
    ? null
    : pluginId;
  const listEntry = beginVisibleSongListEntry();
  await nextTick();
  await finishVisibleSongListEntry(listEntry);
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
  if (!force && playlistLibraryResult.value) {
    void loadFavoriteTrackIdentities(playlistLibraryResult.value);
    return playlistLibraryResult.value;
  }

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
      void loadFavoriteTrackIdentities(result, force);
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

function favoritePlaylistSignature(playlists: OnlinePlaylist[]) {
  return playlists
    .map((playlist) => `${playlist.key}:${playlist.trackCount ?? "unknown"}`)
    .sort()
    .join("|");
}

async function loadFavoritePlaylistIdentities(
  playlist: OnlinePlaylist,
  playlistIndex: number,
  generation: number,
) {
  const identities = new Set<string>();
  if (playlist.trackCount === 0) return identities;

  for (let page = 1; page <= MAX_FAVORITE_TRACK_PAGES; page += 1) {
    const requestId = `online-favorites-${Date.now()}-${generation}-${playlistIndex}-${page}`;
    favoriteTrackRequestIds.add(requestId);
    try {
      const result = await getOnlinePlaylistTracks(
        playlist,
        page,
        FAVORITE_TRACK_PAGE_SIZE,
        requestId,
      );
      if (generation !== favoriteTrackLoadGeneration) return identities;
      for (const track of result.items) {
        for (const candidate of track.candidates) {
          identities.add(trackIdentity(candidate));
        }
      }
      if (!result.hasMore) return identities;
      if (!result.items.length) {
        throw new Error(t("Favorite playlist pagination did not advance."));
      }
    } finally {
      favoriteTrackRequestIds.delete(requestId);
    }
  }

  throw new Error(t("Favorite playlist exceeded the supported page limit."));
}

function loadFavoriteTrackIdentities(result: OnlinePlaylistsResult, force = false) {
  const playlists = result.items.filter((playlist) => playlist.isFavorite);
  const signature = favoritePlaylistSignature(playlists);
  if (!force && favoriteTrackLoadSignature === signature) return Promise.resolve();
  if (
    !force
    && pendingFavoriteTrackLoad
    && pendingFavoriteTrackLoadSignature === signature
  ) {
    return pendingFavoriteTrackLoad;
  }

  cancelFavoriteTrackLoad();
  const generation = favoriteTrackLoadGeneration;
  pendingFavoriteTrackLoadSignature = signature;
  const load = (async () => {
    if (!playlists.length) {
      if (!result.failures.length) favoriteTrackIdentities.value = new Set();
      favoriteTrackLoadSignature = signature;
      return;
    }

    const outcomes = await Promise.allSettled(
      playlists.map((playlist, index) =>
        loadFavoritePlaylistIdentities(playlist, index, generation)
      ),
    );
    if (generation !== favoriteTrackLoadGeneration) return;

    const complete = !result.failures.length
      && outcomes.every((outcome) => outcome.status === "fulfilled");
    const identities = complete
      ? new Set<string>()
      : new Set(favoriteTrackIdentities.value);
    for (const outcome of outcomes) {
      if (outcome.status === "fulfilled") {
        for (const identity of outcome.value) identities.add(identity);
      }
    }
    favoriteTrackIdentities.value = identities;
    favoriteTrackLoadSignature = signature;
  })();
  pendingFavoriteTrackLoad = load;
  void load.finally(() => {
    if (pendingFavoriteTrackLoad === load) {
      pendingFavoriteTrackLoad = null;
      pendingFavoriteTrackLoadSignature = null;
    }
  });
  return load;
}

function cancelFavoriteTrackLoad() {
  favoriteTrackLoadGeneration += 1;
  for (const requestId of favoriteTrackRequestIds) void cancelSourceRequest(requestId);
  favoriteTrackRequestIds.clear();
  pendingFavoriteTrackLoad = null;
  pendingFavoriteTrackLoadSignature = null;
}

function supportsLibraryActions(track: OnlineTrack) {
  return track.candidates.some((candidate) =>
    candidate.pluginId === NETEASE_PLUGIN_ID || candidate.pluginId === KUGOU_PLUGIN_ID
  );
}

function openTrackComments(track: OnlineTrack) {
  if (onlineTrackSupportsComments(track)) commentTrack.value = track;
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
    || optimisticFavoriteTrackIdentities.value.has(trackIdentity(candidate))
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
  throw new Error(t("The selected playlist does not support adding tracks."));
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
    globalError.value = t("This track is not available on NetEase or KuGou.");
    return;
  }
  globalError.value = null;
  const library = await loadPlaylistLibrary();
  if (!library) {
    globalError.value ??= t("Could not load your playlists.");
    return;
  }
  const targets = playlistTargetsForTrack(track, true);
  if (!targets.length) {
    globalError.value = t("No matching My Favorite Music playlist is available.");
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
    optimisticFavoriteTrackIdentities.value = new Set([
      ...optimisticFavoriteTrackIdentities.value,
      ...targets.flatMap((target, index) =>
        outcomes[index].status === "fulfilled"
          ? [trackIdentity(target.candidate)]
          : []
      ),
    ]);
    showTrackActionMessage(
      succeeded === 1
        ? t("Added to My Favorite Music.")
        : t("Added to {count} favorite playlists.", { count: succeeded }),
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
    globalError.value = t("One or more selected tracks are not available on NetEase or KuGou.");
    return;
  }
  globalError.value = null;
  const library = await loadPlaylistLibrary();
  if (!library) {
    globalError.value ??= t("Could not load your playlists.");
    return;
  }
  const targets = playlistTargetsForTracks(tracks);
  if (!targets.length) {
    globalError.value = tracks.length === 1
      ? t("No matching writable playlist is available.")
      : t("No writable playlist supports every selected track.");
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
        ? t("Added to {name}.", { name: target.playlist.name })
        : t("Added {added} of {total} tracks to {name}.", {
            added: succeeded,
            total: tracks.length,
            name: target.playlist.name,
          }),
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
  cancelTrackEntityResolution();
  abandonRecommendationRequest();
  if (activeRecommendation.value !== kind) selectedRecommendationProviderId.value = null;
  activeRecommendation.value = kind;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  recommendationLoading.value = true;
  activeTab.value = "search";
  globalError.value = null;
  const listEntry = beginVisibleSongListEntry();
  const generation = recommendationGeneration;
  const result = await loadRecommendation(kind, force);
  if (generation !== recommendationGeneration || activeRecommendation.value !== kind) return;
  if (result) applyRecommendationResult(result);
  else recommendationError.value = recommendationPreviews.value[kind].error;
  recommendationLoading.value = false;
  await finishVisibleSongListEntry(listEntry);
}

function abandonRecommendationRequest() {
  recommendationGeneration += 1;
  if (activeRecommendation.value) cancelRecommendationLoad(activeRecommendation.value);
  recommendationLoading.value = false;
}

function closeRecommendation() {
  cancelTrackEntityResolution();
  abandonRecommendationRequest();
  activeRecommendation.value = null;
  selectedRecommendationProviderId.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  beginVisibleSongListEntry();
  void preloadForYou();
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = 0;
  });
}

async function submitSearch(suggestion?: string) {
  const keyword = (suggestion ?? query.value).trim();
  if (!keyword) return;
  cancelTrackEntityResolution();
  query.value = keyword;
  suggestionsOpen.value = false;
  abandonRecommendationRequest();
  activeRecommendation.value = null;
  selectedRecommendationProviderId.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  cancelArtistDetailRequests();
  resetArtistDetailState();
  detail.value = null;
  artistDetailHistory.value = null;
  sectionLoadGeneration += 1;
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
  const listEntry = beginVisibleSongListEntry();
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
    await finishVisibleSongListEntry(listEntry);
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
    ? t("No enabled channel supports this search type.")
    : event.result.completedChannels === event.result.failures.length
      ? t("This section could not be loaded.")
      : null;
  if (event.result.section === "songs" && pendingListEntryGeneration !== null) {
    void finishVisibleSongListEntry(pendingListEntryGeneration);
  }
}

function sectionItems<T>(section: OnlineSearchSection): T[] {
  const data = sectionStates.value[section].result?.data;
  return data?.section === section ? (data.items as T[]) : [];
}

function collapseSectionResult(state: SectionState) {
  const result = state.result;
  if (result && result.data.items.length > SEARCH_SUMMARY_SIZE) {
    result.data.items.splice(SEARCH_SUMMARY_SIZE);
    result.hasMore = true;
  }
  state.page = 1;
  state.loadingMore = false;
}

async function loadMore(
  section: OnlineSearchSection,
  generation = sectionLoadGeneration,
) {
  const state = sectionStates.value[section];
  if (state.loadingMore || !submittedQuery.value) return;
  state.loadingMore = true;
  const expandSummary = state.page === 1
    && (state.result?.data.items.length ?? 0) <= SEARCH_SUMMARY_SIZE;
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
    if (generation !== sectionLoadGeneration || expandedSection.value !== section) return;
    if (expandSummary) {
      state.result = next;
    } else {
      appendSectionData(state, next);
    }
    state.page = nextPage;
  } catch (error) {
    if (generation === sectionLoadGeneration && expandedSection.value === section) {
      state.error = normalizeError(error);
    }
  } finally {
    if (generation === sectionLoadGeneration) state.loadingMore = false;
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
  const listEntry = section === "songs" ? beginVisibleSongListEntry() : null;
  try {
    state.result = await getOnlineMusicSearchPage(submittedQuery.value, section, 1, 20);
    state.page = 1;
  } catch (error) {
    state.error = normalizeError(error);
  } finally {
    state.loading = false;
    await finishVisibleSongListEntry(listEntry);
  }
}

function cancelArtistDetailRequests() {
  if (artistAlbumsRequestId) void cancelSourceRequest(artistAlbumsRequestId);
  if (artistBiographyRequestId) void cancelSourceRequest(artistBiographyRequestId);
  artistAlbumsRequestId = null;
  artistBiographyRequestId = null;
}

function cancelTrackEntityResolution() {
  entityResolveGeneration += 1;
  entityActionId.value = null;
  if (entityResolveRequestId) void cancelSourceRequest(entityResolveRequestId);
  entityResolveRequestId = null;
}

function normalizedEntityText(value: string) {
  return value.normalize("NFKC").toLowerCase().trim().split(/\s+/u).join(" ");
}

function artistIdentity(value: string) {
  return [...new Set(
    splitOnlineArtistNames(value)
      .map(normalizedEntityText)
      .filter(Boolean),
  )].sort().join("\u001f");
}

function artistNamesMatch(left: string, right: string) {
  return artistIdentity(left) === artistIdentity(right);
}

function artistNameIncluded(artists: string, artist: string) {
  const target = normalizedEntityText(artist);
  return splitOnlineArtistNames(artists)
    .some((candidate) => normalizedEntityText(candidate) === target);
}

function resolveTrackArtist(result: OnlineSearchSectionResult, artistName: string) {
  if (result.data.section !== "artists") return null;
  const artists = result.data.items as OnlineArtist[];
  const exact = artists.find((item) =>
    normalizedEntityText(item.name) === normalizedEntityText(artistName)
  );
  if (exact) return exact;
  return artists.find((item) => artistNamesMatch(item.name, artistName)) ?? null;
}

function resolveTrackAlbum(result: OnlineSearchSectionResult, track: OnlineTrack) {
  if (result.data.section !== "albums" || !track.album) return null;
  const albums = result.data.items as OnlineAlbum[];
  const title = normalizedEntityText(track.album);
  const titleMatches = albums.filter((item) => normalizedEntityText(item.title) === title);
  return titleMatches.find((item) => artistNamesMatch(item.artist, track.artist))
    ?? (titleMatches.length === 1 ? titleMatches[0] : null);
}

async function searchTrackEntity(
  track: OnlineTrack,
  kind: "artist" | "album",
  label: string,
) {
  cancelTrackEntityResolution();
  const generation = entityResolveGeneration;
  const requestId = `online-track-${kind}-${Date.now()}-${generation}`;
  entityResolveRequestId = requestId;
  entityActionId.value = kind === "artist"
    ? `artist:${track.key}:${label}`
    : `album:${track.key}`;
  globalError.value = null;
  try {
    const result = await getOnlineMusicSearchPage(
      label,
      kind === "artist" ? "artists" : "albums",
      1,
      20,
      requestId,
    );
    return generation === entityResolveGeneration ? result : null;
  } catch (error) {
    if (generation === entityResolveGeneration) globalError.value = normalizeError(error);
    return null;
  } finally {
    if (generation === entityResolveGeneration) {
      entityResolveRequestId = null;
      entityActionId.value = null;
    }
  }
}

async function openTrackArtist(track: OnlineTrack, artistName: string) {
  if (detail.value?.kind === "artist"
    && artistNamesMatch(detail.value.entity.name, artistName)) {
    return;
  }
  if (detail.value?.kind === "album" && artistDetailHistory.value
    && artistNamesMatch(artistDetailHistory.value.detail.entity.name, artistName)) {
    await restoreArtistDetail();
    return;
  }

  const result = await searchTrackEntity(track, "artist", artistName);
  if (!result) return;
  const artist = resolveTrackArtist(result, artistName);
  if (!artist) {
    globalError.value = t('Could not find the artist page for "{name}".', { name: artistName });
    return;
  }
  await openDetail({ kind: "artist", entity: artist });
}

async function openTrackAlbum(track: OnlineTrack) {
  if (!track.album) return;
  if (detail.value?.kind === "album"
    && normalizedEntityText(detail.value.entity.title) === normalizedEntityText(track.album)
    && artistNamesMatch(detail.value.entity.artist, track.artist)) {
    return;
  }

  const result = await searchTrackEntity(track, "album", track.album);
  if (!result) return;
  const album = resolveTrackAlbum(result, track);
  if (!album) {
    globalError.value = t('Could not find the album page for "{name}".', { name: track.album });
    return;
  }
  if (detail.value?.kind === "artist"
    && artistNameIncluded(album.artist, detail.value.entity.name)) {
    await openArtistAlbum(album);
    return;
  }
  await openDetail({ kind: "album", entity: album });
}

function resetArtistDetailState() {
  artistDetailTab.value = "topTracks";
  artistAlbums.value = [];
  artistAlbumsLoading.value = false;
  artistAlbumsLoadingMore.value = false;
  artistAlbumsLoaded.value = false;
  artistAlbumsError.value = null;
  artistAlbumsPage.value = 1;
  artistAlbumsHasMore.value = false;
  artistBiography.value = null;
  artistBiographyLoading.value = false;
  artistBiographyLoaded.value = false;
  artistBiographyError.value = null;
}

async function openDetail(
  next: DetailState,
  rememberScroll = true,
  preserveArtistHistory = false,
) {
  cancelTrackEntityResolution();
  if (!preserveArtistHistory) artistDetailHistory.value = null;
  if (rememberScroll) {
    resultScrollPosition.value = document.querySelector("main")?.scrollTop ?? 0;
  }
  detailAppendGeneration.value += 1;
  cancelArtistDetailRequests();
  resetArtistDetailState();
  detail.value = next;
  detailTracks.value = [];
  detailPage.value = 1;
  detailHasMore.value = false;
  detailLoading.value = true;
  globalError.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  const listEntry = beginVisibleSongListEntry();
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
      globalError.value = t("{name} requires login to read this playlist.", {
        name: playlistError.channelName,
      });
    } else {
      globalError.value = playlistError?.message ?? normalizeError(error);
    }
  } finally {
    if (detailRequestId === requestId) {
      detailLoading.value = false;
      detailRequestId = null;
      await finishVisibleSongListEntry(listEntry);
    }
  }
}

async function openArtistAlbum(album: OnlineAlbum) {
  if (detail.value?.kind !== "artist") return;
  artistDetailHistory.value = {
    detail: detail.value,
    tracks: [...detailTracks.value],
    tracksLoading: detailLoading.value,
    page: detailPage.value,
    hasMore: detailHasMore.value,
    retryAvailable: detailRetryAvailable.value,
    error: globalError.value,
    loginRequiredPluginId: loginRequiredPluginId.value,
    tab: artistDetailTab.value,
    albums: [...artistAlbums.value],
    albumsLoaded: artistAlbumsLoaded.value,
    albumsError: artistAlbumsError.value,
    albumsPage: artistAlbumsPage.value,
    albumsHasMore: artistAlbumsHasMore.value,
    biography: artistBiography.value,
    biographyLoaded: artistBiographyLoaded.value,
    biographyError: artistBiographyError.value,
    scrollPosition: document.querySelector("main")?.scrollTop ?? 0,
  };
  await openDetail({ kind: "album", entity: album }, false, true);
}

async function selectArtistDetailTab(tab: ArtistDetailTab) {
  if (detail.value?.kind !== "artist") return;
  artistDetailTab.value = tab;
  const listEntry = beginVisibleSongListEntry();
  if (tab === "albums" && !artistAlbumsLoaded.value && !artistAlbumsLoading.value) {
    await loadArtistAlbums();
  }
  if (tab === "biography" && !artistBiographyLoaded.value && !artistBiographyLoading.value) {
    await loadArtistBiography();
  }
  await nextTick();
  await finishVisibleSongListEntry(listEntry);
}

async function loadArtistAlbums(loadMore = false) {
  if (detail.value?.kind !== "artist") return;
  if (loadMore ? artistAlbumsLoadingMore.value : artistAlbumsLoading.value) return;
  const page = loadMore ? artistAlbumsPage.value + 1 : 1;
  const requestId = `online-artist-albums-${Date.now()}-${page}`;
  artistAlbumsRequestId = requestId;
  artistAlbumsError.value = null;
  if (loadMore) artistAlbumsLoadingMore.value = true;
  else {
    artistAlbumsLoading.value = true;
  }
  try {
    const result: OnlineAlbumPage = await getOnlineArtistAlbums(
      detail.value.entity,
      page,
      50,
      requestId,
    );
    if (artistAlbumsRequestId !== requestId) return;
    artistAlbums.value = loadMore
      ? [...artistAlbums.value, ...result.items.filter((album) =>
        !artistAlbums.value.some((existing) => existing.key === album.key)
      )]
      : result.items;
    artistAlbumsPage.value = page;
    artistAlbumsHasMore.value = result.hasMore;
    artistAlbumsLoaded.value = true;
  } catch (error) {
    if (artistAlbumsRequestId !== requestId) return;
    artistAlbumsError.value = normalizeError(error);
  } finally {
    if (artistAlbumsRequestId === requestId) {
      artistAlbumsRequestId = null;
      artistAlbumsLoading.value = false;
      artistAlbumsLoadingMore.value = false;
    }
  }
}

async function loadArtistBiography() {
  if (detail.value?.kind !== "artist" || artistBiographyLoading.value) return;
  const requestId = `online-artist-biography-${Date.now()}`;
  artistBiographyRequestId = requestId;
  artistBiographyLoading.value = true;
  artistBiographyError.value = null;
  try {
    const result = await getOnlineArtistBiography(detail.value.entity, requestId);
    if (artistBiographyRequestId !== requestId) return;
    artistBiography.value = result;
    artistBiographyLoaded.value = true;
  } catch (error) {
    if (artistBiographyRequestId !== requestId) return;
    artistBiographyError.value = normalizeError(error);
  } finally {
    if (artistBiographyRequestId === requestId) {
      artistBiographyRequestId = null;
      artistBiographyLoading.value = false;
    }
  }
}

async function retryDetail() {
  if (detail.value) await openDetail(detail.value, false, true);
}

async function loadDetailPage(next: DetailState, page: number, requestId?: string) {
  if (next.kind === "artist") return getOnlineArtistTracks(next.entity, requestId);
  if (next.kind === "album") return getOnlineAlbumTracks(next.entity, page, 100, requestId);
  return getOnlinePlaylistTracks(next.entity, page, 100, requestId);
}

async function loadMoreDetail() {
  if (!detail.value || detailLoadingMore.value || !detailHasMore.value) return;
  const generation = detailAppendGeneration.value;
  const detailKey = detail.value.entity.key;
  detailLoadingMore.value = true;
  const page = detailPage.value + 1;
  const requestId = `online-detail-page-${Date.now()}`;
  try {
    const result = await loadDetailPage(detail.value, page, requestId);
    if (generation !== detailAppendGeneration.value || detail.value?.entity.key !== detailKey) {
      return;
    }
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
    globalError.value = t("Playback is unavailable from the configured Audio Sources.");
    return;
  }
  if (!currentTrack.candidates.length) {
    globalError.value = t("Playback is unavailable from the configured Audio Sources.");
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
    props.activeOnlineTrack !== null
    && onlineTracksMatch(props.activeOnlineTrack, track)
    && props.resolvingOnlineTrackKey !== track.key
  ) {
    emit("togglePlayback");
    return;
  }
  const privateRoaming = activeRecommendation.value === "roaming";
  void playTrack(track, queue, privateRoaming);
}

async function openSection(section: OnlineSearchSection) {
  const generation = ++sectionLoadGeneration;
  summaryScrollPosition.value = document.querySelector("main")?.scrollTop ?? 0;
  expandedSection.value = section;
  const listEntry = beginVisibleSongListEntry();
  await nextTick();
  const state = sectionStates.value[section];
  if (
    (state.result?.data.items.length ?? 0) <= SEARCH_SUMMARY_SIZE
    && state.result?.hasMore
  ) {
    await loadMore(section, generation);
  }
  if (generation !== sectionLoadGeneration || expandedSection.value !== section) return;
  const main = document.querySelector("main");
  if (main) main.scrollTop = 0;
  await finishVisibleSongListEntry(listEntry);
}

async function closeSection() {
  cancelTrackEntityResolution();
  sectionLoadGeneration += 1;
  const section = expandedSection.value;
  if (section) collapseSectionResult(sectionStates.value[section]);
  expandedSection.value = null;
  const listEntry = beginVisibleSongListEntry();
  await nextTick();
  const main = document.querySelector("main");
  if (main) main.scrollTop = summaryScrollPosition.value;
  await finishVisibleSongListEntry(listEntry);
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
  if (!visibleRecommendationTracks.value.length) return;
  const queue = activeRecommendation.value === "roaming"
    ? visibleRecommendationTracks.value
    : [...visibleRecommendationTracks.value];
  await playTrack(queue[0], queue, true);
}

async function downloadAllRecommendation() {
  if (!visibleRecommendationTracks.value.length || !activeRecommendationEntry.value) return;
  await createDownload(
    "recommendation",
    t(activeRecommendationEntry.value.label),
    [...visibleRecommendationTracks.value],
  );
}

async function downloadTrack(track: OnlineTrack, artwork: HTMLElement | null = null) {
  await createDownload(
    "track",
    track.title,
    [track],
    captureDownloadFlightOrigin(track, artwork),
  );
}

async function downloadTracks(tracks: OnlineTrack[]) {
  if (!tracks.length) return;
  await createDownload(
    tracks.length === 1 ? "track" : "selection",
    tracks.length === 1
      ? tracks[0].title
      : t("{count} selected tracks", { count: formatNumber(tracks.length) }),
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

async function createDownload(
  kind: string,
  title: string,
  tracks: OnlineTrack[],
  flightOrigin: DownloadFlightOrigin | null = null,
) {
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
    playDownloadFlight(flightOrigin);
    upsertDownloadTask(await startOnlineDownloadTask(task.taskId));
  } catch (error) {
    globalError.value = normalizeError(error);
  } finally {
    downloadActionId.value = null;
  }
}

function captureDownloadFlightOrigin(
  track: OnlineTrack,
  artwork: HTMLElement | null,
): DownloadFlightOrigin | null {
  if (!artwork) return null;
  const bounds = artwork.getBoundingClientRect();
  if (bounds.width <= 0 || bounds.height <= 0) return null;
  return {
    artwork: artwork.cloneNode(true) as HTMLElement,
    bounds: {
      height: bounds.height,
      left: bounds.left,
      top: bounds.top,
      width: bounds.width,
    },
    trackKey: track.key,
  };
}

function playDownloadFlight(origin: DownloadFlightOrigin | null) {
  const target = downloadsTab.value;
  if (!origin || !target || prefersReducedMotion()) return;
  const targetBounds = target.getBoundingClientRect();
  if (targetBounds.width <= 0 || targetBounds.height <= 0) return;

  const artwork = origin.artwork;
  artwork.dataset.onlineDownloadFlight = origin.trackKey;
  artwork.setAttribute("aria-hidden", "true");
  Object.assign(artwork.style, {
    borderRadius: "0.25rem",
    height: `${origin.bounds.height}px`,
    left: `${origin.bounds.left}px`,
    margin: "0",
    overflow: "hidden",
    pointerEvents: "none",
    position: "fixed",
    top: `${origin.bounds.top}px`,
    transformOrigin: "center",
    width: `${origin.bounds.width}px`,
    willChange: "transform, opacity, filter",
    zIndex: "100",
  });
  document.body.append(artwork);

  if (typeof artwork.animate !== "function") {
    artwork.remove();
    return;
  }

  const destinationX = targetBounds.left + targetBounds.width / 2
    - origin.bounds.left - origin.bounds.width / 2;
  const destinationY = targetBounds.top + targetBounds.height / 2
    - origin.bounds.top - origin.bounds.height / 2;
  const arcHeight = Math.min(56, Math.max(24, Math.abs(destinationX) * 0.08));
  const destinationScale = Math.min(
    0.5,
    Math.max(0.28, (targetBounds.height / origin.bounds.height) * 0.42),
  );

  let animation: Animation;
  try {
    animation = artwork.animate(
      [
        {
          filter: "drop-shadow(0 5px 10px color-mix(in oklab, var(--color-base-content) 24%, transparent))",
          opacity: 1,
          transform: "translate3d(0, 0, 0) scale(1) rotate(0deg)",
        },
        {
          filter: "drop-shadow(0 8px 14px color-mix(in oklab, var(--color-base-content) 18%, transparent))",
          offset: 0.55,
          opacity: 0.92,
          transform: `translate3d(${destinationX * 0.58}px, ${destinationY * 0.46 - arcHeight}px, 0) scale(0.72) rotate(5deg)`,
        },
        {
          filter: "drop-shadow(0 0 0 transparent)",
          opacity: 0.08,
          transform: `translate3d(${destinationX}px, ${destinationY}px, 0) scale(${destinationScale}) rotate(0deg)`,
        },
      ],
      {
        duration: 680,
        easing: "cubic-bezier(0.22, 0.72, 0.18, 1)",
        fill: "forwards",
      },
    );
  } catch {
    artwork.remove();
    return;
  }

  const flight = { animation, artwork };
  downloadFlightAnimations.add(flight);
  void animation.finished
    .catch(() => undefined)
    .then(() => {
      downloadFlightAnimations.delete(flight);
      artwork.remove();
    });
}

function prefersReducedMotion() {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
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

function applyDownloadProgress(progress: OnlineDownloadProgressEvent) {
  const taskIndex = downloadTasks.value.findIndex((task) => task.taskId === progress.taskId);
  if (taskIndex < 0) return;
  const task = downloadTasks.value[taskIndex];
  if (task.state !== "running") return;
  const itemIndex = task.items.findIndex((item) => item.itemId === progress.itemId);
  if (itemIndex < 0) return;
  const item = task.items[itemIndex];
  if (["paused", "completed", "skipped", "failed", "cancelled"].includes(item.state)) return;
  if (item.state === "downloading" && progress.state === "resolving") return;

  const items = [...task.items];
  items[itemIndex] = {
    ...item,
    state: progress.state,
    bytesDownloaded: Math.max(0, progress.bytesDownloaded),
    totalBytes: progress.totalBytes === null ? null : Math.max(0, progress.totalBytes),
  };
  downloadTasks.value[taskIndex] = { ...task, items };
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
  const terminalItems = task.completedItems + task.skippedItems + task.failedItems;
  const activeProgress = task.items.reduce((total, item) => {
    if (item.state !== "downloading" || !item.totalBytes) return total;
    return total + Math.min(1, Math.max(0, item.bytesDownloaded / item.totalBytes));
  }, 0);
  return Math.round(Math.min(1, (terminalItems + activeProgress) / task.totalItems) * 100);
}

function taskProgressValue(task: OnlineDownloadTask) {
  const indeterminate = task.items.some((item) =>
    (item.state === "resolving")
    || (item.state === "downloading" && !item.totalBytes)
  );
  const progress = taskProgress(task);
  return indeterminate && progress === 0 ? undefined : progress;
}

function taskProgressText(task: OnlineDownloadTask) {
  const progress = taskProgress(task);
  const isResolving = task.items.some((item) => item.state === "resolving");
  const hasUnknownActiveSize = task.items.some((item) =>
    item.state === "downloading" && !item.totalBytes
  );
  if (progress === 0 && isResolving) return t("Resolving");
  if (progress === 0 && hasUnknownActiveSize) return t("Downloading");
  return `${progress}%${isResolving || hasUnknownActiveSize ? "+" : ""}`;
}

function activeDownloadBytes(task: OnlineDownloadTask) {
  const active = task.items.filter((item) => item.state === "downloading");
  if (!active.length) return null;
  const downloaded = active.reduce((total, item) => total + item.bytesDownloaded, 0);
  const allSizesKnown = active.every((item) => item.totalBytes !== null && item.totalBytes > 0);
  if (!allSizesKnown) {
    return downloaded > 0
      ? t("{size} downloaded", { size: formatBytes(downloaded) })
      : t("Downloading");
  }
  const total = active.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0);
  return t("{downloaded} of {total} active", {
    downloaded: formatBytes(downloaded),
    total: formatBytes(total),
  });
}

function itemProgressText(item: OnlineDownloadTask["items"][number]) {
  if (item.totalBytes) {
    const percentage = Math.round(
      Math.min(1, Math.max(0, item.bytesDownloaded / item.totalBytes)) * 100,
    );
    return `${percentage}% · ${formatBytes(item.bytesDownloaded)} / ${formatBytes(item.totalBytes)}`;
  }
  return item.bytesDownloaded > 0 ? formatBytes(item.bytesDownloaded) : null;
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / (1024 ** exponent);
  return `${value >= 10 || Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`;
}

function downloadTaskStateLabel(state: OnlineDownloadTask["state"]) {
  return t({
    queued: "Queued",
    running: "Downloading",
    paused: "Paused",
    completed: "Complete",
    completedWithErrors: "Finished with errors",
    cancelled: "Cancelled",
  }[state]);
}

function downloadItemStateLabel(state: OnlineDownloadTask["items"][number]["state"]) {
  return t({
    queued: "Waiting",
    resolving: "Resolving",
    downloading: "Downloading",
    paused: "Paused",
    completed: "Complete",
    skipped: "Skipped",
    failed: "Failed",
    cancelled: "Cancelled",
  }[state]);
}

async function backToResults() {
  cancelTrackEntityResolution();
  detailAppendGeneration.value += 1;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  cancelArtistDetailRequests();
  detailRequestId = null;
  detailLoading.value = false;
  detailLoadingMore.value = false;
  detail.value = null;
  artistDetailHistory.value = null;
  resetArtistDetailState();
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  const listEntry = beginVisibleSongListEntry();
  await nextTick();
  const main = document.querySelector("main");
  if (main) main.scrollTop = resultScrollPosition.value;
  await finishVisibleSongListEntry(listEntry);
}

async function restoreArtistDetail() {
  cancelTrackEntityResolution();
  const history = artistDetailHistory.value;
  if (!history || detail.value?.kind !== "album") {
    await backToResults();
    return;
  }

  detailAppendGeneration.value += 1;
  detailRequestGeneration += 1;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  cancelArtistDetailRequests();
  detailRequestId = null;
  artistDetailHistory.value = null;
  detail.value = history.detail;
  detailTracks.value = history.tracks;
  detailPage.value = history.page;
  detailHasMore.value = history.hasMore;
  detailLoading.value = history.tracksLoading;
  detailLoadingMore.value = false;
  detailRetryAvailable.value = history.retryAvailable;
  globalError.value = history.error;
  loginRequiredPluginId.value = history.loginRequiredPluginId;
  artistDetailTab.value = history.tab;
  artistAlbums.value = history.albums;
  artistAlbumsLoading.value = false;
  artistAlbumsLoadingMore.value = false;
  artistAlbumsLoaded.value = history.albumsLoaded;
  artistAlbumsError.value = history.albumsError;
  artistAlbumsPage.value = history.albumsPage;
  artistAlbumsHasMore.value = history.albumsHasMore;
  artistBiography.value = history.biography;
  artistBiographyLoading.value = false;
  artistBiographyLoaded.value = history.biographyLoaded;
  artistBiographyError.value = history.biographyError;

  const listEntry = beginVisibleSongListEntry();
  await nextTick();
  const main = document.querySelector("main");
  if (main) main.scrollTop = history.scrollPosition;
  if (history.tracksLoading) {
    await reloadRestoredArtistTracks(history.detail);
  } else {
    await finishVisibleSongListEntry(listEntry);
  }
}

async function reloadRestoredArtistTracks(artist: Extract<DetailState, { kind: "artist" }>) {
  detailLoading.value = true;
  detailRetryAvailable.value = false;
  globalError.value = null;
  const listEntry = beginVisibleSongListEntry();
  const requestId = `online-detail-${Date.now()}-${++detailRequestGeneration}`;
  detailRequestId = requestId;
  try {
    const page = await loadDetailPage(artist, 1, requestId);
    if (detailRequestId !== requestId || detail.value?.entity.key !== artist.entity.key) return;
    detailTracks.value = page.items;
    detailPage.value = 1;
    detailHasMore.value = page.hasMore;
  } catch (error) {
    if (detailRequestId !== requestId) return;
    detailRetryAvailable.value = true;
    globalError.value = normalizeError(error);
  } finally {
    if (detailRequestId === requestId) {
      detailLoading.value = false;
      detailRequestId = null;
      await finishVisibleSongListEntry(listEntry);
    }
  }
}

async function backFromDetail() {
  if (detail.value?.kind === "album" && artistDetailHistory.value) {
    await restoreArtistDetail();
    return;
  }
  await backToResults();
}

function selectTab(tab: "search" | "downloads") {
  if (activeTab.value === tab) {
    return;
  }
  cancelTrackEntityResolution();
  activeTab.value = tab;
  const listEntry = beginVisibleSongListEntry();
  void finishVisibleSongListEntry(listEntry);
}

function dismissGlobalError() {
  globalError.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
}

function showHome() {
  cancelTrackEntityResolution();
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
  sectionLoadGeneration += 1;
  expandedSection.value = null;
  sectionStates.value = newSectionStates();

  detailAppendGeneration.value += 1;
  detailRequestGeneration += 1;
  if (detailRequestId) void cancelSourceRequest(detailRequestId);
  cancelArtistDetailRequests();
  detailRequestId = null;
  detail.value = null;
  artistDetailHistory.value = null;
  resetArtistDetailState();
  detailTracks.value = [];
  detailLoading.value = false;
  detailLoadingMore.value = false;
  detailHasMore.value = false;

  abandonRecommendationRequest();
  activeRecommendation.value = null;
  selectedRecommendationProviderId.value = null;
  recommendationTracks.value = [];
  recommendationFailures.value = [];
  recommendationError.value = null;
  globalError.value = null;
  pendingPlaylistTracks.value = [];
  selectedPlaylistTargetKey.value = "";
  trackActionId.value = null;
  loginRequiredPluginId.value = null;
  detailRetryAvailable.value = false;
  beginVisibleSongListEntry();
  void preloadForYou();
  void nextTick(() => {
    const main = document.querySelector("main");
    if (main) main.scrollTop = 0;
  });
}

defineExpose({
  addToFavorites,
  downloadTrack,
  focusSearch,
  isDownloadActionPending,
  isTrackActionPending,
  isTrackFavorite,
  openPlaylistPicker,
  showHome,
});

</script>

<template>
  <div ref="workspaceRoot" class="flex w-full min-w-0 flex-col gap-4">
    <div class="flex flex-col gap-3 border-b border-base-300 pb-4 sm:flex-row sm:items-center">
      <div ref="searchArea" class="relative min-w-0 flex-1">
        <form class="join flex w-full" role="search" @submit.prevent="submitSearch()">
          <label class="input input-sm join-item flex min-w-0 flex-1 items-center gap-2">
            <Search :size="16" aria-hidden="true" />
            <input
              ref="searchInput"
              v-model="query"
              class="min-w-0 grow"
              type="search"
              autocomplete="off"
              :placeholder="t('Search songs, artists, albums, and playlists')"
              :aria-label="t('Search Online Music')"
              :aria-keyshortcuts="shortcutAriaKeys('focusSearch')"
              @input="onQueryInput"
              @focus="openSuggestions"
              @click="openSuggestions"
            />
          </label>
          <button class="btn btn-primary btn-sm join-item" type="submit" :disabled="!query.trim()">
            <Search :size="16" aria-hidden="true" />
            {{ t("Search") }}
          </button>
        </form>

        <div
          v-if="suggestionsOpen && (suggestionLoading || suggestions.length)"
          class="absolute inset-x-0 top-full z-30 mt-1 overflow-hidden rounded border border-base-300 bg-base-100 shadow-lg"
        >
          <div v-if="suggestionLoading" class="flex h-10 items-center gap-2 px-3 text-xs text-muted">
            <span class="loading loading-spinner loading-xs"></span>
            {{ query.trim() ? t("Loading suggestions") : t("Loading search history") }}
          </div>
          <ul
            v-else
            class="menu menu-sm w-full p-1"
            :aria-label="query.trim() ? t('Search suggestions') : t('Search history')"
          >
            <li v-for="suggestion in suggestions" :key="suggestion">
              <button type="button" @mousedown.prevent="submitSearch(suggestion)">
              <History v-if="!query.trim()" :size="16" aria-hidden="true" />
              <Search v-else :size="16" aria-hidden="true" />
                <span class="truncate">{{ suggestion }}</span>
              </button>
            </li>
          </ul>
        </div>
      </div>

      <div role="tablist" class="tabs tabs-border shrink-0">
        <button
          role="tab"
          class="tab"
          :class="activeTab === 'search' ? 'tab-active text-base-content' : 'text-muted'"
          :aria-selected="activeTab === 'search'"
          @click="selectTab('search')"
        >
          {{ t("Search") }}
        </button>
        <button
          ref="downloadsTab"
          role="tab"
          data-online-download-tab
          class="tab"
          :class="activeTab === 'downloads' ? 'tab-active text-base-content' : 'text-muted'"
          :aria-selected="activeTab === 'downloads'"
          @click="selectTab('downloads')"
        >
          {{ t("Downloads") }}
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
          {{ t("Open channel") }}
        </button>
        <button v-if="detailRetryAvailable" class="btn btn-sm" type="button" @click="retryDetail">
          <RefreshCw :size="16" aria-hidden="true" />
          {{ t("Retry") }}
        </button>
      </div>
      <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Dismiss error')" @click="dismissGlobalError">
        <X :size="16" aria-hidden="true" />
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
        <h2 class="text-sm font-semibold">{{ t("Download tasks") }}</h2>
        <span class="text-xs text-muted">{{ t(downloadTasks.length === 1 ? "{count} task" : "{count} tasks", { count: downloadTasks.length }) }}</span>
      </div>
      <div v-if="!downloadTasks.length" class="flex min-h-48 flex-col items-center justify-center gap-2 text-muted">
        <Download :size="24" aria-hidden="true" />
        <span class="text-sm">{{ t("No download tasks") }}</span>
      </div>
      <div v-else class="divide-y divide-base-300">
        <section v-for="task in downloadTasks" :key="task.taskId" class="py-3">
          <div class="flex min-w-0 items-start gap-3">
            <Download :size="17" class="mt-1 shrink-0" aria-hidden="true" />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="min-w-0 flex-1 truncate text-sm font-medium">{{ task.title }}</h3>
                <span class="badge badge-sm" :class="{ 'badge-error': task.state === 'completedWithErrors', 'badge-success': task.state === 'completed' }">
                  {{ downloadTaskStateLabel(task.state) }}
                </span>
              </div>
              <div class="mt-1 flex min-w-0 items-center gap-2 text-xs text-muted">
                <span>{{ t("{count} complete", { count: task.completedItems }) }}</span>
                <span>{{ t("{count} skipped", { count: task.skippedItems }) }}</span>
                <span v-if="task.failedItems">{{ t("{count} failed", { count: task.failedItems }) }}</span>
                <span v-if="activeDownloadBytes(task)" class="hidden min-w-0 truncate sm:inline">
                  {{ activeDownloadBytes(task) }}
                </span>
                <span class="ml-auto shrink-0 tabular-nums">{{ taskProgressText(task) }}</span>
              </div>
              <progress
                class="progress progress-primary mt-1 h-1.5 w-full"
                :value="taskProgressValue(task)"
                max="100"
                :aria-label="t('{title} progress', { title: task.title })"
              ></progress>
            </div>
            <div class="flex shrink-0 gap-1">
              <button
                v-if="task.state === 'paused' || task.state === 'queued'"
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="downloadActionId === task.taskId"
                :aria-label="t('Resume download task')"
                :title="t('Resume')"
                @click="runDownloadAction(task, 'start')"
              >
                <RefreshCw v-if="downloadActionId === task.taskId" class="animate-spin" :size="16" aria-hidden="true" />
                <Play v-else :size="16" aria-hidden="true" />
              </button>
              <button
                v-if="task.state === 'running'"
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="downloadActionId === task.taskId"
                :aria-label="t('Pause download task')"
                :title="t('Pause')"
                @click="runDownloadAction(task, 'pause')"
              >
                <Pause :size="16" aria-hidden="true" />
              </button>
              <button
                v-if="!['completed', 'cancelled'].includes(task.state)"
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="downloadActionId === task.taskId"
                :aria-label="t('Cancel download task')"
                :title="t('Cancel')"
                @click="runDownloadAction(task, 'cancel')"
              >
                <Ban :size="16" aria-hidden="true" />
              </button>
            </div>
          </div>
          <ul class="mt-2 divide-y divide-base-300/70 pl-7">
            <li
              v-for="item in task.items"
              :key="item.itemId"
              class="grid min-w-0 grid-cols-[6rem_minmax(0,1fr)] items-center gap-x-2 gap-y-1 py-1.5 text-xs sm:flex"
            >
              <span class="w-24 shrink-0 text-muted">{{ downloadItemStateLabel(item.state) }}</span>
              <span class="min-w-0 flex-1 truncate">{{ item.track.artist }} - {{ item.track.title }}</span>
              <div class="col-start-2 flex min-w-0 items-center justify-end gap-1 sm:contents">
                <span v-if="itemProgressText(item)" class="shrink-0 tabular-nums text-muted">
                  {{ itemProgressText(item) }}
                </span>
                <span v-if="item.message" class="hidden max-w-56 truncate text-error lg:block" :title="item.message">{{ item.message }}</span>
                <button
                  v-if="item.state === 'failed' && task.state !== 'running'"
                  class="btn btn-square btn-ghost btn-sm"
                  type="button"
                  :disabled="downloadActionId === item.itemId"
                  :aria-label="t('Refresh candidates for {title}', { title: item.track.title })"
                  :title="t('Refresh candidates')"
                  @click="refreshAndRetryDownloadItem(task, item.itemId)"
                >
                  <Search :class="{ 'animate-pulse': downloadActionId === item.itemId }" :size="16" aria-hidden="true" />
                </button>
                <button
                  v-if="(item.state === 'failed' || item.state === 'cancelled') && task.state !== 'running'"
                  class="btn btn-square btn-ghost btn-sm"
                  type="button"
                  :disabled="downloadActionId === item.itemId"
                  :aria-label="t('Retry {title}', { title: item.track.title })"
                  :title="t('Retry')"
                  @click="retryDownloadItem(task, item.itemId)"
                >
                  <RefreshCw :class="{ 'animate-spin': downloadActionId === item.itemId }" :size="16" aria-hidden="true" />
                </button>
              </div>
            </li>
          </ul>
        </section>
      </div>
    </div>

    <div v-else-if="detail" class="min-w-0">
      <div class="mb-3 flex min-w-0 items-center gap-3 border-b border-base-300 pb-3">
        <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="detailBackLabel" :title="t('Back')" @click="backFromDetail">
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <div class="flex size-12 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
          <img v-if="detail.entity.coverUrl" :src="detail.entity.coverUrl" class="size-full object-cover" alt="" />
          <component :is="detail.kind === 'artist' ? UserRound : detail.kind === 'album' ? Disc3 : ListMusic" v-else :size="22" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ visibleDetailTitle }}</h2>
          <p class="truncate text-xs text-muted">
            {{ visibleDetailSubtitle }}
          </p>
        </div>
        <div v-if="detailTracks.length && isArtistTopTracksTab" class="flex shrink-0 gap-1">
          <button class="btn btn-sm" type="button" @click="playAllDetail">
            <Play :size="16" aria-hidden="true" />
            {{ t("Play all") }}
          </button>
          <button class="btn btn-square btn-sm" type="button" :aria-label="t('Download all loaded tracks')" :title="t('Download all')" @click="downloadAll">
            <Download :size="16" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div v-if="detail.kind === 'artist'" role="tablist" class="tabs tabs-border mb-3" :aria-label="t('Artist details')">
        <button
          role="tab"
          class="tab"
          :class="artistDetailTab === 'topTracks' ? 'tab-active text-base-content' : 'text-muted'"
          :aria-selected="artistDetailTab === 'topTracks'"
          data-online-artist-tab="top-tracks"
          type="button"
          @click="selectArtistDetailTab('topTracks')"
        >
          {{ t("Top tracks") }}
        </button>
        <button
          role="tab"
          class="tab"
          :class="artistDetailTab === 'albums' ? 'tab-active text-base-content' : 'text-muted'"
          :aria-selected="artistDetailTab === 'albums'"
          data-online-artist-tab="albums"
          type="button"
          @click="selectArtistDetailTab('albums')"
        >
          {{ t("All albums") }}
        </button>
        <button
          role="tab"
          class="tab"
          :class="artistDetailTab === 'biography' ? 'tab-active text-base-content' : 'text-muted'"
          :aria-selected="artistDetailTab === 'biography'"
          data-online-artist-tab="biography"
          type="button"
          @click="selectArtistDetailTab('biography')"
        >
          {{ t("Artist bio") }}
        </button>
      </div>

      <template v-if="isArtistTopTracksTab">
        <div v-if="detailLoading" class="space-y-2">
          <div v-for="index in 7" :key="index" class="skeleton h-11 w-full"></div>
        </div>
        <OnlineTrackTable
          v-else
          :tracks="detailTracks"
          :active-track="activeOnlineTrack"
          :is-playing="isPlaying"
          :track-action-id="trackActionId"
          :entity-action-id="entityActionId"
          :supports-library-actions="supportsLibraryActions"
          :supports-playlist-selection="supportsPlaylistSelection"
          :supports-comments="onlineTrackSupportsComments"
          :is-favorite="isTrackFavorite"
          @play="playTrack($event, detailTracks)"
          @queue-tracks="handleQueueTracks"
          @download="downloadTrack"
          @download-selection="downloadTracks"
          @favorite="addToFavorites"
          @add-to-playlist="openPlaylistPicker"
          @add-selection-to-playlist="openPlaylistPicker"
          @add-to-collection="emit('addToCollection', $event)"
          @create-collection="emit('createCollection', $event)"
          @view-comments="openTrackComments"
          @open-artist="openTrackArtist"
          @open-album="openTrackAlbum"
        />
        <div v-if="detailHasMore" class="flex justify-center py-4">
          <button class="btn btn-sm" type="button" :disabled="detailLoadingMore" @click="loadMoreDetail">
            <span v-if="detailLoadingMore" class="loading loading-spinner loading-xs"></span>
            {{ t("Load more") }}
          </button>
        </div>
      </template>

      <div v-else-if="artistDetailTab === 'albums'" data-online-artist-albums>
        <div v-if="artistAlbumsLoading" class="space-y-2">
          <div v-for="index in 6" :key="index" class="skeleton h-14 w-full"></div>
        </div>
        <div
          v-else-if="artistAlbumsError && !artistAlbums.length"
          class="flex min-h-24 items-center justify-between gap-3 text-sm text-muted"
        >
          <span>{{ artistAlbumsError }}</span>
          <button class="btn btn-sm" type="button" @click="loadArtistAlbums()">
            <RefreshCw :size="16" aria-hidden="true" />
            {{ t("Retry") }}
          </button>
        </div>
        <div v-else>
          <div v-if="!artistAlbums.length" class="flex min-h-20 items-center text-sm text-muted">
            {{ t("No albums found") }}
          </div>
          <ul v-else class="list divide-y divide-base-300">
            <li v-for="album in artistAlbums" :key="album.key">
              <button
                class="list-row w-full px-0 py-2 text-left hover:bg-base-200/60"
                type="button"
                :aria-label="t('Open album {album}', { album: album.title })"
                :data-online-artist-album="album.key"
                @click="openArtistAlbum(album)"
              >
                <div class="flex size-11 items-center justify-center overflow-hidden rounded bg-base-200">
                  <img v-if="album.coverUrl" :src="album.coverUrl" class="size-full object-cover" alt="" />
                  <Disc3 v-else :size="19" aria-hidden="true" />
                </div>
                <div class="min-w-0">
                  <div class="truncate text-sm font-medium">{{ album.title }}</div>
                  <div class="truncate text-xs text-muted">
                    {{ [album.artist, album.releaseYear].filter(Boolean).join(' · ') }}
                  </div>
                </div>
                <span v-if="album.trackCount !== null" class="text-xs tabular-nums text-muted">
                  {{ album.trackCount }}
                </span>
                <ChevronRight :size="16" class="text-muted" aria-hidden="true" />
              </button>
            </li>
          </ul>
          <div v-if="artistAlbumsError" role="alert" class="alert alert-error mt-3 py-2">
            <AlertCircle :size="17" aria-hidden="true" />
            <span class="min-w-0 flex-1 text-sm">{{ artistAlbumsError }}</span>
            <button class="btn btn-sm" type="button" @click="loadArtistAlbums(true)">
              <RefreshCw :size="16" aria-hidden="true" />
              {{ t("Retry") }}
            </button>
          </div>
          <div v-if="artistAlbumsHasMore" class="flex justify-center py-4">
            <button class="btn btn-sm" type="button" :disabled="artistAlbumsLoadingMore" @click="loadArtistAlbums(true)">
              <span v-if="artistAlbumsLoadingMore" class="loading loading-spinner loading-xs"></span>
              {{ t("Load more") }}
            </button>
          </div>
        </div>
      </div>

      <div v-else data-online-artist-biography>
        <div v-if="artistBiographyLoading" class="space-y-3">
          <div class="skeleton h-5 w-2/5"></div>
          <div v-for="index in 4" :key="index" class="skeleton h-4 w-full"></div>
        </div>
        <div
          v-else-if="artistBiographyError"
          class="flex min-h-24 items-center justify-between gap-3 text-sm text-muted"
        >
          <span>{{ artistBiographyError }}</span>
          <button class="btn btn-sm" type="button" @click="loadArtistBiography()">
            <RefreshCw :size="16" aria-hidden="true" />
            {{ t("Retry") }}
          </button>
        </div>
        <div
          v-else-if="!artistBiography?.summary && !artistBiography?.sections.length"
          class="flex min-h-20 items-center text-sm text-muted"
        >
          {{ t("No artist biography found") }}
        </div>
        <div v-else class="space-y-5">
          <p v-if="artistBiography?.summary" class="whitespace-pre-line text-sm leading-6 text-muted">
            {{ artistBiography.summary }}
          </p>
          <section v-for="section in artistBiography?.sections" :key="`${section.title}:${section.text}`" class="space-y-1">
            <h3 class="text-sm font-semibold">{{ section.title }}</h3>
            <p class="whitespace-pre-line text-sm leading-6 text-muted">{{ section.text }}</p>
          </section>
          <p class="text-xs text-muted">{{ artistBiography?.sourceName }}</p>
        </div>
      </div>
    </div>

    <div v-else-if="activeRecommendationEntry" data-online-recommendation class="min-w-0">
      <div class="mb-3 flex min-w-0 flex-wrap items-center gap-3 border-b border-base-300 pb-3">
        <button
          class="btn btn-square btn-ghost btn-sm"
          type="button"
          :aria-label="t('Back to Online Music home')"
          :title="t('Back')"
          @click="closeRecommendation"
        >
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <div class="flex size-12 shrink-0 items-center justify-center rounded bg-base-200">
          <component :is="activeRecommendationEntry.icon" :size="22" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{{ t(activeRecommendationEntry.label) }}</h2>
          <div
            v-if="activeRecommendationEntry.providers.length > 1"
            class="mt-1 flex flex-wrap items-center gap-1"
          >
            <div
              class="join"
              role="group"
              :aria-label="t('Recommendation channels')"
            >
              <button
                v-for="provider in activeRecommendationEntry.providers"
                :key="provider.pluginId"
                class="btn btn-ghost btn-xs join-item"
                :class="{ 'btn-active': selectedRecommendationProviderId === provider.pluginId }"
                type="button"
                :data-recommendation-provider="provider.pluginId"
                :aria-label="t('Filter by {name}', { name: provider.label })"
                :aria-pressed="selectedRecommendationProviderId === provider.pluginId"
                :title="t('Filter by {name}', { name: provider.label })"
                @click="toggleRecommendationProvider(provider.pluginId)"
              >
                {{ provider.label }}
              </button>
            </div>
            <span v-if="visibleRecommendationFailures.length && visibleRecommendationTracks.length" class="badge badge-warning badge-sm">
              {{ t("Partial") }}
            </span>
          </div>
          <div v-else class="mt-1 flex flex-wrap gap-1">
            <span class="badge badge-sm">
              {{ activeRecommendationEntry.providers[0]?.label }}
            </span>
            <span v-if="visibleRecommendationFailures.length && visibleRecommendationTracks.length" class="badge badge-warning badge-sm">
              {{ t("Partial") }}
            </span>
          </div>
        </div>
        <div class="ml-auto flex basis-full justify-end gap-1 sm:basis-auto">
          <button
            v-if="activeRecommendationEntry.id !== 'roaming'"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :disabled="recommendationLoading"
            :aria-label="t('Refresh recommendations')"
            :title="t('Refresh recommendations')"
            @click="openRecommendation(activeRecommendationEntry.id, true)"
          >
            <RefreshCw
              :class="{ 'animate-spin': recommendationLoading }"
              :size="16"
              aria-hidden="true"
            />
          </button>
          <button
            v-if="visibleRecommendationTracks.length"
            class="btn btn-sm"
            type="button"
            @click="playAllRecommendation"
          >
            <Play :size="16" aria-hidden="true" />
            {{ t("Play all") }}
          </button>
          <button
            v-if="visibleRecommendationTracks.length"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :aria-label="t('Download all recommendations')"
            :title="t('Download all')"
            @click="downloadAllRecommendation"
          >
            <Download :size="16" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div v-if="visibleRecommendationError" role="alert" class="alert alert-error mb-3 py-2">
        <AlertCircle :size="17" aria-hidden="true" />
        <span class="min-w-0 flex-1 text-sm">{{ visibleRecommendationError }}</span>
        <div v-if="!visibleRecommendationTracks.length" class="flex shrink-0 flex-wrap gap-1">
          <button
            v-for="provider in visibleRecommendationProviders"
            :key="provider.pluginId"
            class="btn btn-sm"
            type="button"
            @click="emit('openPlugin', provider.pluginId)"
          >
            {{ t("Open {name}", { name: provider.label }) }}
          </button>
        </div>
      </div>
      <div
        v-else-if="visibleRecommendationFailures.length && visibleRecommendationTracks.length"
        role="status"
        class="alert alert-warning mb-3 py-2 text-sm"
      >
        <AlertCircle :size="17" aria-hidden="true" />
        {{ t("{names} unavailable", { names: visibleRecommendationFailures.map((failure) => failure.channelName).join(', ') }) }}
      </div>

      <div v-if="recommendationLoading" class="space-y-2">
        <div v-for="index in 8" :key="index" class="skeleton h-11 w-full"></div>
      </div>
      <OnlineTrackTable
        v-else-if="visibleRecommendationTracks.length"
        :tracks="visibleRecommendationTracks"
        :active-track="activeOnlineTrack"
        :is-playing="isPlaying"
        :track-action-id="trackActionId"
        :entity-action-id="entityActionId"
        :supports-library-actions="supportsLibraryActions"
        :supports-playlist-selection="supportsPlaylistSelection"
        :supports-comments="onlineTrackSupportsComments"
        :is-favorite="isTrackFavorite"
        @play="requestTrackPlayback($event, visibleRecommendationTracks)"
        @queue-tracks="handleQueueTracks"
        @download="downloadTrack"
        @download-selection="downloadTracks"
        @favorite="addToFavorites"
        @add-to-playlist="openPlaylistPicker"
        @add-selection-to-playlist="openPlaylistPicker"
        @add-to-collection="emit('addToCollection', $event)"
        @create-collection="emit('createCollection', $event)"
        @view-comments="openTrackComments"
        @open-artist="openTrackArtist"
        @open-album="openTrackAlbum"
      />
      <div
        v-if="activeRecommendationEntry.id === 'roaming' && visibleRecommendationTracks.length"
        class="flex justify-center border-t border-base-300 py-4"
      >
        <button
          data-private-roaming-next
          class="btn btn-sm"
          type="button"
          :disabled="privateRoamingBatchLoading"
          :aria-label="t('Load next private roaming batch')"
          @click="loadNextPrivateRoamingBatch()"
        >
          <span
            v-if="privateRoamingBatchLoading"
            class="loading loading-spinner loading-xs"
            aria-hidden="true"
          ></span>
          <ListPlus v-else :size="16" aria-hidden="true" />
          {{ t("Load next songs") }}
        </button>
      </div>
      <div v-else-if="!visibleRecommendationError" class="flex min-h-48 items-center justify-center text-sm text-muted">
        {{ t("No recommendations available") }}
      </div>
    </div>

    <div v-else-if="!hasSubmittedSearch" data-online-home class="min-h-64 py-2">
      <div class="mb-3 flex items-center gap-2">
        <House :size="17" aria-hidden="true" />
        <h2 class="text-sm font-semibold">{{ t("For you") }}</h2>
      </div>
      <div class="grid grid-cols-1 gap-3 md:grid-cols-3">
        <button
          v-for="entry in recommendationEntries"
          :key="entry.id"
          data-online-recommendation-entry
          class="card card-border card-sm group relative isolate h-36 w-full overflow-hidden bg-base-100 text-left transition-colors hover:bg-base-200/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          type="button"
          :aria-label="t(entry.label)"
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
                  ? 'text-neutral-content'
                  : 'text-muted'"
                aria-hidden="true"
              />
            </div>
            <div class="min-w-0">
              <h3
                class="card-title text-base"
                :class="{ 'text-neutral-content': recommendationCoverUrl(entry.id) }"
              >
                {{ t(entry.label) }}
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
          <h2 class="text-sm font-semibold">{{ t("Playlists") }}</h2>
          <span v-if="playlistLibraryItems.length" class="text-xs tabular-nums text-muted">
            {{ t("{count} loaded", { count: playlistLibraryItems.length }) }}
          </span>
          <span
            v-if="playlistLibraryFailures.length && playlistLibraryItems.length"
            class="badge badge-warning badge-sm ml-auto"
          >
            {{ t("Partial") }}
          </span>
          <button
            class="btn btn-square btn-ghost btn-sm"
            :class="{ 'ml-auto': !playlistLibraryFailures.length || !playlistLibraryItems.length }"
            type="button"
            :disabled="playlistLibraryLoading"
            :aria-label="t('Refresh playlists')"
            :title="t('Refresh playlists')"
            @click="loadPlaylistLibrary(true)"
          >
            <RefreshCw
              :class="{ 'animate-spin': playlistLibraryLoading }"
              :size="16"
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
              <RefreshCw :size="16" aria-hidden="true" />
            {{ t("Retry") }}
          </button>
        </div>

        <div
          v-else-if="playlistLibraryFailures.length && !playlistLibraryItems.length"
          role="status"
          class="alert py-2"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-sm">
            {{ t("Connect NetEase or KuGou to load your playlists.") }}
          </span>
          <div class="flex shrink-0 flex-wrap gap-1">
            <button
              v-for="provider in failedPlaylistProviders"
              :key="provider.pluginId"
              class="btn btn-sm"
              type="button"
              @click="emit('openPlugin', provider.pluginId)"
            >
              {{ t("Open {name}", { name: provider.label }) }}
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
            {{ t("Enable NetEase or KuGou to load your playlists.") }}
          </span>
          <div class="flex shrink-0 flex-wrap gap-1">
            <button
              v-for="provider in playlistProviders"
              :key="provider.pluginId"
              class="btn btn-sm"
              type="button"
              @click="emit('openPlugin', provider.pluginId)"
            >
              {{ t("Open {name}", { name: provider.label }) }}
            </button>
          </div>
        </div>

        <div
          v-else-if="!playlistLibraryItems.length"
          class="flex min-h-24 items-center text-sm text-muted"
        >
          {{ t("No playlists found") }}
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
              <span class="text-xs tabular-nums text-muted">
                {{ provider.items.length }}
                {{ provider.items.length === 1 ? t("playlist") : t("playlists") }}
              </span>
            </div>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <button
                v-for="playlist in provider.items"
                :key="playlist.key"
                class="card card-border card-sm min-h-24 w-full bg-base-100 text-left transition-colors hover:bg-base-200/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                type="button"
                :aria-label="t('Open playlist {name}', { name: playlist.name })"
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
                    <p v-if="playlist.ownerName" class="mt-1 truncate text-xs text-muted">
                      {{ playlist.ownerName }}
                    </p>
                    <p
                      v-if="playlist.trackCount !== null"
                      class="mt-2 truncate text-xs tabular-nums text-muted"
                    >
                      {{ t("{count} tracks", { count: playlist.trackCount }) }}
                    </p>
                  </div>
                  <ChevronRight :size="17" class="shrink-0 text-muted" aria-hidden="true" />
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
            <RefreshCw :size="16" aria-hidden="true" />
            {{ t("Retry") }}
          </button>
        </div>

        <div
          v-if="playlistLibraryFailures.length && playlistLibraryItems.length"
          role="status"
          class="alert alert-warning mt-3 py-2 text-sm"
        >
          <AlertCircle :size="17" aria-hidden="true" />
          <span class="min-w-0 flex-1">
            {{ t("{names} unavailable", { names: playlistLibraryFailures.map((failure) => failure.channelName).join(', ') }) }}
          </span>
          <button
            v-for="provider in failedPlaylistProviders"
            :key="provider.pluginId"
            class="btn btn-sm"
            type="button"
            @click="emit('openPlugin', provider.pluginId)"
          >
            {{ t("Open {name}", { name: provider.label }) }}
          </button>
        </div>
      </section>
    </div>

    <div v-else data-online-results class="flex flex-col gap-5">

      <div v-if="expandedSection" class="flex items-center gap-2 border-b border-base-300 pb-3">
        <button class="btn btn-square btn-ghost btn-sm" type="button" :aria-label="t('Back to search summary')" :title="t('Back')" @click="closeSection">
          <ArrowLeft :size="17" aria-hidden="true" />
        </button>
        <span class="text-sm font-medium">{{ t("All {section}", { section: t(sections.find((section) => section.id === expandedSection)?.label || '') }) }}</span>
      </div>

      <section v-for="section in visibleSections" :key="section.id" class="min-w-0">
        <div class="mb-2 flex items-center gap-2 border-b border-base-300 pb-2">
          <component :is="section.icon" :size="17" aria-hidden="true" />
          <h2 class="text-sm font-semibold">{{ t(section.label) }}</h2>
          <span v-if="sectionStates[section.id].result" class="text-xs tabular-nums text-muted">
            {{ t("{count} loaded", { count: sectionStates[section.id].result?.data.items.length ?? 0 }) }}
          </span>
          <span v-if="sectionStates[section.id].result?.failures.length" class="badge badge-warning badge-sm ml-auto">
            {{ t("Partial") }}
          </span>
          <button
            v-if="sectionStates[section.id].result?.failures.length"
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            :aria-label="t('Retry unavailable {section} channels', { section: t(section.label) })"
            :title="t('Retry unavailable channels')"
            @click="retrySection(section.id)"
          >
            <RefreshCw :size="16" aria-hidden="true" />
          </button>
        </div>

        <div v-if="sectionStates[section.id].loading" class="space-y-2">
          <div v-for="index in 5" :key="index" class="skeleton h-11 w-full"></div>
        </div>
        <div v-else-if="sectionStates[section.id].error" class="flex min-h-24 items-center justify-between gap-3 text-sm text-muted">
          <span>{{ sectionStates[section.id].error }}</span>
          <button class="btn btn-sm" type="button" @click="retrySection(section.id)">
          <RefreshCw :size="16" aria-hidden="true" />
            {{ t("Retry") }}
          </button>
        </div>
        <div v-else-if="!sectionStates[section.id].result?.data.items.length" class="flex min-h-20 items-center text-sm text-muted">
          {{ t("No {section} found", { section: t(section.label).toLocaleLowerCase() }) }}
        </div>

        <OnlineTrackTable
          v-else-if="section.id === 'songs'"
          :tracks="sectionItems<OnlineTrack>('songs')"
          :active-track="activeOnlineTrack"
          :is-playing="isPlaying"
          :track-action-id="trackActionId"
          :entity-action-id="entityActionId"
          :supports-library-actions="supportsLibraryActions"
          :supports-playlist-selection="supportsPlaylistSelection"
          :supports-comments="onlineTrackSupportsComments"
          :is-favorite="isTrackFavorite"
          @play="requestTrackPlayback($event, sectionItems<OnlineTrack>('songs'))"
          @queue-tracks="handleQueueTracks"
          @download="downloadTrack"
          @download-selection="downloadTracks"
          @favorite="addToFavorites"
          @add-to-playlist="openPlaylistPicker"
          @add-selection-to-playlist="openPlaylistPicker"
          @add-to-collection="emit('addToCollection', $event)"
          @create-collection="emit('createCollection', $event)"
          @view-comments="openTrackComments"
          @open-artist="openTrackArtist"
          @open-album="openTrackAlbum"
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
              <div class="truncate text-xs text-muted">
                {{ item.artist ?? item.ownerName ?? item.candidates?.map((candidate: any) => candidate.channelName).join(' / ') ?? item.channelName }}
              </div>
            </div>
            <span class="text-xs tabular-nums text-muted">{{ item.trackCount ?? '' }}</span>
          </li>
        </ul>

        <div v-if="sectionStates[section.id].result?.hasMore" class="flex justify-center pt-3">
          <button class="btn btn-sm" type="button" :disabled="sectionStates[section.id].loadingMore" @click="expandedSection === section.id ? loadMore(section.id) : openSection(section.id)">
            <span v-if="sectionStates[section.id].loadingMore" class="loading loading-spinner loading-xs"></span>
            {{ expandedSection === section.id ? t('Load more') : t('More {section}', { section: t(section.label) }) }}
          </button>
        </div>
      </section>
    </div>

    <OnlineCommentsDialog
      v-if="commentTrack"
      :key="commentTrack.key"
      :track="commentTrack"
      @close="commentTrack = null"
    />

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
              <h2 id="online-playlist-picker-title" class="text-base font-semibold">{{ t("Add to Playlist") }}</h2>
              <p class="mt-1 truncate text-sm text-muted">
                {{ pendingPlaylistTracks.length === 1
                  ? `${pendingPlaylistTracks[0].title} · ${pendingPlaylistTracks[0].artist}`
                  : t("{count} selected tracks", { count: pendingPlaylistTracks.length }) }}
              </p>
            </div>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              :disabled="trackActionId === playlistPickerActionId"
              :aria-label="t('Close playlist picker')"
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
              <span class="block truncate text-xs text-muted">
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
              {{ t("Cancel") }}
            </button>
            <button
              class="btn btn-primary btn-sm"
              type="submit"
              :disabled="!selectedPlaylistTarget || trackActionId === playlistPickerActionId"
            >
          <RefreshCw v-if="trackActionId === playlistPickerActionId" class="animate-spin" :size="16" aria-hidden="true" />
          <ListPlus v-else :size="16" aria-hidden="true" />
              {{ t("Add") }}
            </button>
          </div>
        </form>
        <form method="dialog" class="modal-backdrop" @submit.prevent="closePlaylistPicker">
          <button type="submit" :disabled="trackActionId === playlistPickerActionId">{{ t("Close") }}</button>
        </form>
      </dialog>
    </Teleport>
  </div>
</template>
