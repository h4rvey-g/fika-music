<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertCircle,
  AudioLines,
  Captions,
  Check,
  ChevronRight,
  Download,
  Ellipsis,
  FolderOpen,
  FolderPlus,
  Gauge,
  Headphones,
  Heart,
  Library,
  ListMusic,
  ListPlus,
  ListOrdered,
  Menu,
  Music2,
  Pause,
  Palette,
  Pencil,
  Play,
  Plus,
  Plug,
  Radio,
  RefreshCw,
  Repeat2,
  RotateCcw,
  Settings,
  Shuffle,
  SkipBack,
  SkipForward,
  Trash2,
  Volume2,
  X,
} from "@lucide/vue";
import LibraryBrowser from "./components/LibraryBrowser.vue";
import CollectionBrowser from "./components/CollectionBrowser.vue";
import AudioSourceManager from "./components/AudioSourceManager.vue";
import KugouSource from "./components/KugouSource.vue";
import PluginManager from "./components/PluginManager.vue";
import PluginWorkspace from "./components/PluginWorkspace.vue";
import NeteaseSource from "./components/NeteaseSource.vue";
import NowPlayingPanel from "./components/NowPlayingPanel.vue";
import NowPlayingLyricsSettings from "./components/NowPlayingLyricsSettings.vue";
import DesktopLyricsSettings from "./components/DesktopLyricsSettings.vue";
import OnlineMusic from "./components/OnlineMusic.vue";
import OnlineMusicSettingsPanel from "./components/OnlineMusicSettings.vue";
import fikaLogoUrl from "../src-tauri/icons/fika.svg";
import { useLibraryScan } from "./composables/use-library-scan";
import { useOnlineMusicConfig } from "./composables/use-online-music-config";
import { NETEASE_PLUGIN_ID } from "./lib/netease-api";
import { KUGOU_PLUGIN_ID } from "./lib/kugou-api";
import {
  buildAudioSourceOptions,
  listAudioSources,
  type AudioSourceRecord,
} from "./lib/audio-source-api";
import { listPlugins, type PluginRecord } from "./lib/plugin-api";
import { PlayCountTracker } from "./lib/play-count-tracker";
import { normalizeError } from "./lib/errors";
import { ExpiringCache } from "./lib/expiring-cache";
import {
  clearPreloadedMedia,
  clearOnlinePlaybackFailures,
  invalidateOnlinePlaybackCaches,
  preloadMediaUrl,
  playbackAttemptKey,
  reportOnlinePlaybackFailure,
  resolveOnlineTrack,
  type OnlinePlayback,
  type OnlineTrack,
} from "./lib/online-music-api";
import { TAURI_COMMANDS } from "./generated/bindings";
import type {
  AudioSourceSelectionMode,
  LibraryChangedEvent,
  LibraryPlaybackQueue,
  LibraryQueueTrack,
  LocalTrack,
  LocalTrackPlaybackDetails,
  MediaSource,
  OnlineMusicSettings,
  ResolvedLyrics,
  TrackLyricsQuery,
} from "./generated/bindings";
import {
  DEFAULT_UI_PREFERENCES,
  THEME_GROUPS,
  THEME_MODE_OPTIONS,
  loadUiPreferences,
  saveUiPreferences,
  type PlaybackMode,
  type StreamQuality,
  type ThemePreference,
} from "./lib/ui-preferences";
import {
  applyCoverTheme,
  clearCoverTheme,
  extractCoverTheme,
} from "./lib/dynamic-theme";
import {
  DEFAULT_DESKTOP_LYRICS_PREFERENCES,
  DESKTOP_LYRICS_HIDE_EVENT,
  DESKTOP_LYRICS_READY_EVENT,
  DESKTOP_LYRICS_UPDATE_EVENT,
  desktopLyricsMessage,
  loadDesktopLyricsPreferences,
  parseDesktopLyricsPreferences,
  resolveDesktopLyricLines,
  saveDesktopLyricsPreferences,
  type DesktopLyricsPreferences,
  type DesktopLyricsState,
} from "./lib/desktop-lyrics";
import {
  broadcastDesktopLyricsState,
  syncDesktopLyricsWindow,
  syncMenuBarLyrics,
} from "./lib/desktop-lyrics-window";
import {
  DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
  NOW_PLAYING_LYRICS_SETTINGS_ID,
  loadNowPlayingLyricsPreferences,
  parseNowPlayingLyricsPreferences,
  saveNowPlayingLyricsPreferences,
  type NowPlayingLyricsPreferences,
} from "./lib/now-playing-lyrics";
import {
  COLLECTION_DRAG_TYPE,
  addMusicCollectionItemsToMusicCollection,
  addLocalSelectionToMusicCollection,
  addOnlineTracksToMusicCollection,
  createMusicCollection,
  deleteMusicCollection,
  listMusicCollections,
  readCollectionDragPayload,
  renameMusicCollection,
  type CollectionSeed,
  type CollectionItemSelection,
  type LocalCollectionSelection,
  type MusicCollectionItem,
  type MusicCollectionMutation,
  type MusicCollectionSummary,
} from "./lib/collection-api";

type LibraryBrowserInstance = {
  refresh: () => Promise<void>;
  startFirstTrack: () => Promise<void>;
  updatePlayCount: (trackId: number, playCount: number) => void;
};

type CollectionBrowserInstance = {
  refresh: () => Promise<void>;
  startCollection: (collectionId?: string) => Promise<void>;
  startFirstTrack: (collectionId?: string) => Promise<void>;
  updatePlayCount: (trackId: number, playCount: number) => void;
};

type OnlineMusicInstance = {
  addToFavorites: (track: OnlineTrack) => Promise<void>;
  downloadTrack: (track: OnlineTrack) => Promise<void>;
  isDownloadActionPending: () => boolean;
  isTrackActionPending: (track: OnlineTrack, action: "favorite" | "playlist") => boolean;
  isTrackFavorite: (track: OnlineTrack) => boolean;
  openPlaylistPicker: (track: OnlineTrack) => Promise<void>;
  showHome: () => void;
};

type OnlineQueueLoadMore = () => Promise<OnlineTrack[]>;

type PreloadedOnlinePlayback = {
  index: number;
  playback: OnlinePlayback;
  preparedAt: number;
};

const mainSections = [
  {
    id: "local",
    label: "Local Music",
    description: "Browse and index music stored on this device",
    icon: Library,
  },
  {
    id: "online",
    label: "Online Music",
    description: "Search enabled music channels",
    icon: Radio,
  },
  {
    id: "sources",
    label: "Audio Sources",
    description: "Import sources and manage their permissions",
    icon: AudioLines,
  },
  {
    id: "plugins",
    label: "Plugins",
    description: "Manage installed packages and diagnostics",
    icon: Plug,
  },
] as const;

const settingsSection = {
  id: "settings",
  label: "Settings",
  description: "Manage appearance and playback defaults",
  icon: Settings,
} as const;

const STREAM_QUALITY_OPTIONS: ReadonlyArray<{ value: StreamQuality; label: string }> = [
  { value: "128k", label: "128 kbps" },
  { value: "320k", label: "320 kbps" },
  { value: "flac", label: "FLAC" },
  { value: "flac24bit", label: "FLAC 24-bit" },
];

const sections = [...mainSections, settingsSection];
type AppSection = (typeof sections)[number]["id"];
type ActiveSection = AppSection | "plugin" | "collection";
type AppLocation =
  | { section: AppSection; pluginId: null; collectionId: null }
  | { section: "plugin"; pluginId: string; collectionId: null }
  | { section: "collection"; pluginId: null; collectionId: string };

type PositionedMenu = { x: number; y: number };
type CollectionNameDialog =
  | { intent: "create"; seed: CollectionSeed }
  | { intent: "rename"; collectionId: string };

const MOUSE_BACK_BUTTON = 3;
const MOUSE_FORWARD_BUTTON = 4;

const savedUiPreferences = loadUiPreferences();
const savedDesktopLyricsPreferences = loadDesktopLyricsPreferences();
const savedNowPlayingLyricsPreferences = loadNowPlayingLyricsPreferences();
const appError = ref<string | null>(null);
const libraryScan = useLibraryScan((message) => {
  appError.value = message;
});
const {
  scanStatus,
  selectedFolder,
  scanMessage,
  isChoosingFolder,
  chooseFolder,
} = libraryScan;
const activeSection = ref<ActiveSection>("local");
const activePluginId = ref<string | null>(null);
const activeCollectionId = ref<string | null>(null);
const pluginRecords = ref<PluginRecord[]>([]);
const audioSourceRecords = ref<AudioSourceRecord[]>([]);
const musicCollections = ref<MusicCollectionSummary[]>([]);
const collectionsExpanded = ref(true);
const collectionRefreshKey = ref(0);
const collectionDropTargetId = ref<string | null>(null);
const localMusicContextMenu = ref<PositionedMenu | null>(null);
const collectionContextMenu = ref<(PositionedMenu & { collectionId: string }) | null>(null);
const collectionPickerSeed = ref<CollectionSeed | null>(null);
const selectedCollectionId = ref("");
const collectionNameDialog = ref<CollectionNameDialog | null>(null);
const collectionName = ref("");
const collectionNameInput = ref<HTMLInputElement | null>(null);
const collectionActionError = ref<string | null>(null);
const collectionActionBusy = ref(false);
const collectionDeleteTarget = ref<MusicCollectionSummary | null>(null);
const collectionNotice = ref<string | null>(null);
const sidebarOpen = ref(false);
const activeTrack = ref<LocalTrack | null>(null);
const activeRemoteTitle = ref<string | null>(null);
const activeOnlineTrack = ref<OnlineTrack | null>(null);
const activeRemoteQuality = ref<string | null>(null);
const activeOnlineAudioSourceId = ref<string | null>(null);
const activeOnlineChannelId = ref<string | null>(null);
const activeOnlineAttemptKey = ref<string | null>(null);
const activeOnlineUrl = ref<string | null>(null);
const sourceChangeMessage = ref<string | null>(null);
const audioUrl = ref<string | null>(null);
const isPlaying = ref(false);
const isPlaybackWaiting = ref(false);
const playbackPosition = ref(0);
const playbackDuration = ref(0);
const volume = ref(savedUiPreferences.volume);
const playbackMode = ref<PlaybackMode>(savedUiPreferences.playbackMode);
const desktopLyricsPreferences = ref(savedDesktopLyricsPreferences);
const nowPlayingLyricsPreferences = ref(savedNowPlayingLyricsPreferences);
const themePreference = ref(savedUiPreferences.theme);
const layoutDensity = ref(savedUiPreferences.density);
const nowPlayingCoverUrl = ref<string | null>(null);
const dynamicThemeStatus = ref<"idle" | "waiting" | "loading" | "active" | "unavailable">("idle");
const activeLyrics = ref<ResolvedLyrics | null>(null);
const activeRemoteLyricsQuery = ref<TrackLyricsQuery | null>(null);
const isLoadingLyrics = ref(false);
const lyricsError = ref<string | null>(null);
const isPreparingPlayback = ref(false);
const audioElement = ref<HTMLAudioElement | null>(null);
const playbackOptionsMenu = ref<HTMLDetailsElement | null>(null);
const libraryBrowser = ref<LibraryBrowserInstance | null>(null);
const collectionBrowser = ref<CollectionBrowserInstance | null>(null);
const onlineMusic = ref<OnlineMusicInstance | null>(null);
const libraryTrackCount = ref(0);
const filteredLibraryTrackCount = ref(0);
const localQueueId = ref<string | null>(null);
const localQueueTotal = ref(0);
const localQueueIndex = ref(-1);
const queuedLocalTrack = ref<LocalTrack | null>(null);
const localQueueActive = ref(false);
const remoteQueue = ref<OnlineTrack[]>([]);
const remoteQueueIndex = ref(-1);
const remoteQueueActive = ref(false);
const remoteQueueLoadMore = ref<OnlineQueueLoadMore | null>(null);
const collectionQueue = ref<MusicCollectionItem[]>([]);
const collectionQueueIndex = ref(-1);
const collectionQueueActive = ref(false);
const resolvingOnlineTrackKey = ref<string | null>(null);
const playbackAudioSourceId = ref(savedUiPreferences.audioSourceId);
const remoteQuality = ref(savedUiPreferences.streamQuality);
const audioSourceSelectionMode = ref<AudioSourceSelectionMode>("automatic");
const onlineMusicConfig = useOnlineMusicConfig();

const navigationHistory: AppLocation[] = [
  { section: "local", pluginId: null, collectionId: null },
];
let navigationHistoryIndex = 0;
let playbackDetailsGeneration = 0;
let onlinePlaybackController: AbortController | null = null;
let onlinePreloadController: AbortController | null = null;
let onlinePreloadTimer: ReturnType<typeof setTimeout> | null = null;
let preloadedOnlinePlayback: PreloadedOnlinePlayback | null = null;
let pendingRemoteQueueLoad: Promise<OnlineTrack[]> | null = null;
let remoteQueueGeneration = 0;
let audioSourceSelectionModeGeneration = 0;
let sourceChangeMessageTimer: ReturnType<typeof setTimeout> | null = null;
let collectionNoticeTimer: ReturnType<typeof setTimeout> | null = null;
let sidebarCollectionPlaybackGeneration = 0;
let dynamicThemeGeneration = 0;
const desktopLyricsUnlisteners: UnlistenFn[] = [];
const collectionUnlisteners: UnlistenFn[] = [];
const failedOnlineAttempts = new ExpiringCache<string, true>(5 * 60_000, 256);
const failedOnlineUrls = new ExpiringCache<string, true>(5 * 60_000, 256);
const playCountTracker = new PlayCountTracker();

const enabledPlugins = computed(() => pluginRecords.value.filter((plugin) => plugin.enabled));
const availableAudioSources = computed(() => buildAudioSourceOptions(audioSourceRecords.value));
const activePlugin = computed(
  () => enabledPlugins.value.find((plugin) => plugin.id === activePluginId.value) ?? null,
);
const activeCollection = computed(
  () => musicCollections.value.find((collection) => collection.id === activeCollectionId.value) ?? null,
);
const currentSection = computed(() => {
  if (activeSection.value === "collection" && activeCollection.value) {
    return {
      label: activeCollection.value.name,
      description: `${activeCollection.value.itemCount.toLocaleString()} tracks in this Collection`,
    };
  }
  if (activeSection.value === "plugin" && activePlugin.value) {
    return {
      label: activePlugin.value.name,
      description:
        activePlugin.value.description || "Inspect this plugin's registered source providers",
    };
  }

  return sections.find((section) => section.id === activeSection.value) ?? mainSections[0];
});
const nowPlayingTitle = computed(() => activeTrack.value?.title || activeRemoteTitle.value || "Nothing playing");
const nowPlayingSubtitle = computed(() => {
  if (activeTrack.value) {
    return trackSubtitle(activeTrack.value);
  }

  if (!activeRemoteTitle.value) return "Select a local or remote track";
  return activeOnlineTrack.value
    ? [activeOnlineTrack.value.artist, activeOnlineTrack.value.album].filter(Boolean).join(" - ")
    : "Remote track";
});
const activeOnlineTrackSupportsLibraryActions = computed(() => {
  const track = activeOnlineTrack.value;
  return Boolean(track?.candidates.some((candidate) =>
    candidate.pluginId === NETEASE_PLUGIN_ID || candidate.pluginId === KUGOU_PLUGIN_ID
  ));
});
const activeOnlineTrackIsFavorite = computed(() => {
  const track = activeOnlineTrack.value;
  return Boolean(track && onlineMusic.value?.isTrackFavorite(track));
});
const activeOnlineFavoritePending = computed(() => {
  const track = activeOnlineTrack.value;
  return Boolean(track && onlineMusic.value?.isTrackActionPending(track, "favorite"));
});
const activeOnlinePlaylistPending = computed(() => {
  const track = activeOnlineTrack.value;
  return Boolean(track && onlineMusic.value?.isTrackActionPending(track, "playlist"));
});
const activeOnlineDownloadPending = computed(() => onlineMusic.value?.isDownloadActionPending() ?? false);
const volumePercent = computed(() => Math.round(volume.value * 100));
const currentPlaybackAudioSourceId = computed(() =>
  activeOnlineTrack.value
    ? activeOnlineAudioSourceId.value
    : playbackAudioSourceId.value,
);
const automaticAudioSourceSelection = computed(
  () => audioSourceSelectionMode.value === "automatic",
);
const currentPlaybackQuality = computed(() =>
  activeOnlineTrack.value && activeRemoteQuality.value
    ? activeRemoteQuality.value
    : remoteQuality.value,
);
const canGoPrevious = computed(() => {
  if (collectionQueueActive.value && collectionQueueIndex.value >= 0) {
    return playbackMode.value !== "sequential" || collectionQueueIndex.value > 0;
  }
  if (remoteQueueActive.value && remoteQueueIndex.value >= 0) {
    return playbackMode.value !== "sequential" || remoteQueueIndex.value > 0;
  }
  if (
    !activeTrack.value ||
    !localQueueActive.value ||
    !localQueueId.value ||
    localQueueIndex.value < 0
  ) {
    return false;
  }
  return playbackMode.value !== "sequential" || localQueueIndex.value > 0;
});
const canGoNext = computed(() => {
  if (collectionQueueActive.value && collectionQueueIndex.value >= 0) {
    return playbackMode.value !== "sequential"
      || collectionQueueIndex.value < collectionQueue.value.length - 1;
  }
  if (remoteQueueActive.value && remoteQueueIndex.value >= 0) {
    return (
      Boolean(remoteQueueLoadMore.value) ||
      playbackMode.value !== "sequential" ||
      remoteQueueIndex.value < remoteQueue.value.length - 1
    );
  }
  if (
    !activeTrack.value ||
    !localQueueActive.value ||
    !localQueueId.value ||
    localQueueIndex.value < 0
  ) {
    return false;
  }
  return playbackMode.value !== "sequential" || localQueueIndex.value < localQueueTotal.value - 1;
});
const playbackModeLabel = computed(() => {
  switch (playbackMode.value) {
    case "shuffle":
      return "Shuffle";
    case "repeat":
      return "Repeat all";
    default:
      return "Sequential";
  }
});
const nextPlaybackModeLabel = computed(() => {
  switch (playbackMode.value) {
    case "sequential":
      return "Shuffle";
    case "shuffle":
      return "Repeat all";
    default:
      return "Sequential";
  }
});
const desktopLyricLines = computed(() => {
  if (!activeTrack.value && !activeRemoteTitle.value) {
    return desktopLyricsMessage("Nothing playing");
  }
  if (isLoadingLyrics.value) {
    return desktopLyricsMessage("Loading lyrics");
  }
  if (lyricsError.value) {
    return desktopLyricsMessage("Lyrics unavailable");
  }
  return resolveDesktopLyricLines(
    activeLyrics.value,
    playbackPosition.value,
    playbackDuration.value,
  );
});
const desktopLyricsState = computed<DesktopLyricsState>(() => ({
  title: nowPlayingTitle.value,
  subtitle: nowPlayingSubtitle.value,
  ...desktopLyricLines.value,
  isPlaying: isPlaying.value,
  clockRunning: isPlaying.value && !isPlaybackWaiting.value,
  playbackRate: audioElement.value?.playbackRate || 1,
  playbackPositionMs: playbackPosition.value * 1_000,
  preferences: { ...desktopLyricsPreferences.value },
}));

watch(
  [themePreference, nowPlayingCoverUrl],
  ([theme, coverUrl]) => void applyTheme(theme, coverUrl),
  { immediate: true },
);
watch(volume, updateVolume);
watch(audioUrl, () => {
  playbackPosition.value = 0;
  playbackDuration.value = 0;
});
watch(playbackMode, (mode) => {
  cancelOnlinePreload();
  if (mode === "sequential") scheduleNextOnlinePreload();
});
watch([remoteQuality, playbackAudioSourceId], cancelOnlinePreload);
watch(
  [
    themePreference,
    layoutDensity,
    remoteQuality,
    playbackAudioSourceId,
    volume,
    playbackMode,
  ],
  () => {
    saveUiPreferences({
      theme: themePreference.value,
      density: layoutDensity.value,
      streamQuality: remoteQuality.value,
      audioSourceId: playbackAudioSourceId.value,
      volume: volume.value,
      playbackMode: playbackMode.value,
    });
  },
);
watch(
  desktopLyricsPreferences,
  (preferences) => {
    saveDesktopLyricsPreferences(preferences);
    void syncDesktopLyricsWindow(desktopLyricsState.value);
    void syncMenuBarLyrics(desktopLyricsState.value);
  },
  { deep: true },
);
watch(
  nowPlayingLyricsPreferences,
  (preferences) => saveNowPlayingLyricsPreferences(preferences),
  { deep: true },
);
watch(
  [
    nowPlayingTitle,
    nowPlayingSubtitle,
    desktopLyricLines,
    playbackPosition,
    isPlaying,
    isPlaybackWaiting,
  ],
  () => void broadcastDesktopLyricsState(desktopLyricsState.value),
);
watch(
  [
    nowPlayingTitle,
    nowPlayingSubtitle,
    () => desktopLyricLines.value.currentLine,
    () => desktopLyricLines.value.currentLineKey,
  ],
  () => void syncMenuBarLyrics(desktopLyricsState.value),
);

onMounted(async () => {
  document.addEventListener("click", handlePlaybackOptionsOutsideClick);
  window.addEventListener("mousedown", suppressMouseNavigationDefault);
  window.addEventListener("mouseup", handleMouseNavigation);
  window.addEventListener("auxclick", suppressMouseNavigationDefault);
  await setupDesktopLyricsEvents();
  await setupCollectionEvents();
  await Promise.all([
    libraryScan.initialize(),
    loadPluginNavigation(),
    loadAudioSourceNavigation(),
    loadCollectionNavigation(),
  ]);
  const modeGeneration = audioSourceSelectionModeGeneration;
  void onlineMusicConfig.load().then(({ settings }) => {
    if (modeGeneration === audioSourceSelectionModeGeneration) {
      audioSourceSelectionMode.value = settings.audioSourceSelectionMode;
    }
  }).catch(() => undefined);
  await syncDesktopLyricsWindow(desktopLyricsState.value);
  await syncMenuBarLyrics(desktopLyricsState.value);
});

onBeforeUnmount(() => {
  dynamicThemeGeneration += 1;
  clearCoverTheme(document.documentElement);
  document.removeEventListener("click", handlePlaybackOptionsOutsideClick);
  window.removeEventListener("mousedown", suppressMouseNavigationDefault);
  window.removeEventListener("mouseup", handleMouseNavigation);
  window.removeEventListener("auxclick", suppressMouseNavigationDefault);
  libraryScan.dispose();
  if (sourceChangeMessageTimer) clearTimeout(sourceChangeMessageTimer);
  if (collectionNoticeTimer) clearTimeout(collectionNoticeTimer);
  playbackDetailsGeneration += 1;
  onlinePlaybackController?.abort();
  cancelOnlinePreload();
  for (const unlisten of desktopLyricsUnlisteners) unlisten();
  for (const unlisten of collectionUnlisteners) unlisten();
  sampleListeningTime();
});

async function setupDesktopLyricsEvents() {
  desktopLyricsUnlisteners.push(
    await listen(DESKTOP_LYRICS_READY_EVENT, () => {
      void broadcastDesktopLyricsState(desktopLyricsState.value);
    }),
    await listen(DESKTOP_LYRICS_HIDE_EVENT, () => {
      updateDesktopLyricsPreferences({ enabled: false });
    }),
    await listen<Partial<DesktopLyricsPreferences>>(DESKTOP_LYRICS_UPDATE_EVENT, (event) => {
      updateDesktopLyricsPreferences(event.payload);
    }),
  );
}

async function setupCollectionEvents() {
  collectionUnlisteners.push(
    await listen<LibraryChangedEvent>("library:changed", () => {
      collectionRefreshKey.value += 1;
      void loadCollectionNavigation();
    }),
  );
}

function selectSection(section: AppSection) {
  const resetOnlineHome = section === "online" && activeSection.value === "online";
  navigateTo({ section, pluginId: null, collectionId: null });
  if (resetOnlineHome) {
    void nextTick(() => onlineMusic.value?.showHome());
  }
}

async function openNowPlayingLyricsSettings() {
  selectSection(settingsSection.id);
  await nextTick();
  const settings = document.getElementById(NOW_PLAYING_LYRICS_SETTINGS_ID);
  settings?.focus({ preventScroll: true });
  settings?.scrollIntoView?.({ behavior: "smooth", block: "start" });
}

function selectPlugin(pluginId: string) {
  navigateTo({ section: "plugin", pluginId, collectionId: null });
}

function selectCollection(collectionId: string) {
  navigateTo({ section: "collection", pluginId: null, collectionId });
}

async function playCollectionFromSidebar(collection: MusicCollectionSummary) {
  selectCollection(collection.id);
  const generation = ++sidebarCollectionPlaybackGeneration;
  if (!collection.itemCount) {
    showCollectionNotice(`${collection.name} has no tracks to play.`);
    return;
  }
  await nextTick();
  if (
    generation !== sidebarCollectionPlaybackGeneration
    || activeSection.value !== "collection"
    || activeCollectionId.value !== collection.id
  ) {
    return;
  }
  await collectionBrowser.value?.startCollection(collection.id);
}

function locationsMatch(left: AppLocation, right: AppLocation) {
  return left.section === right.section
    && left.pluginId === right.pluginId
    && left.collectionId === right.collectionId;
}

function applyLocation(location: AppLocation) {
  activeSection.value = location.section;
  activePluginId.value = location.pluginId;
  activeCollectionId.value = location.collectionId;
  sidebarOpen.value = false;
}

function navigateTo(location: AppLocation) {
  const current = navigationHistory[navigationHistoryIndex];
  if (current && locationsMatch(current, location)) {
    applyLocation(location);
    return;
  }

  navigationHistory.splice(navigationHistoryIndex + 1);
  navigationHistory.push(location);
  navigationHistoryIndex = navigationHistory.length - 1;
  applyLocation(location);
}

function replaceCurrentLocation(location: AppLocation) {
  navigationHistory[navigationHistoryIndex] = location;
  applyLocation(location);
}

function isLocationAvailable(location: AppLocation) {
  if (location.section === "plugin") {
    return enabledPlugins.value.some((plugin) => plugin.id === location.pluginId);
  }
  if (location.section === "collection") {
    return musicCollections.value.some((collection) => collection.id === location.collectionId);
  }
  return true;
}

function moveThroughNavigationHistory(direction: -1 | 1) {
  let nextIndex = navigationHistoryIndex + direction;
  while (
    nextIndex >= 0
    && nextIndex < navigationHistory.length
    && !isLocationAvailable(navigationHistory[nextIndex])
  ) {
    nextIndex += direction;
  }
  if (nextIndex < 0 || nextIndex >= navigationHistory.length) return;

  navigationHistoryIndex = nextIndex;
  applyLocation(navigationHistory[navigationHistoryIndex]);
}

function isMouseNavigationButton(event: MouseEvent) {
  return event.button === MOUSE_BACK_BUTTON || event.button === MOUSE_FORWARD_BUTTON;
}

function suppressMouseNavigationDefault(event: MouseEvent) {
  if (isMouseNavigationButton(event)) event.preventDefault();
}

function handleMouseNavigation(event: MouseEvent) {
  if (!isMouseNavigationButton(event)) return;
  event.preventDefault();
  moveThroughNavigationHistory(event.button === MOUSE_BACK_BUTTON ? -1 : 1);
}

async function loadPluginNavigation() {
  try {
    updatePluginRecords(await listPlugins());
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

async function loadAudioSourceNavigation() {
  try {
    updateAudioSourceRecords(await listAudioSources());
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

async function loadCollectionNavigation() {
  try {
    const collections = await listMusicCollections();
    musicCollections.value = Array.isArray(collections) ? collections : [];
    if (
      activeSection.value === "collection"
      && !musicCollections.value.some((collection) => collection.id === activeCollectionId.value)
    ) {
      replaceCurrentLocation({ section: "local", pluginId: null, collectionId: null });
    }
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

function updateCollectionSummary(collection: MusicCollectionSummary) {
  const index = musicCollections.value.findIndex((candidate) => candidate.id === collection.id);
  if (index < 0) {
    musicCollections.value = [...musicCollections.value, collection];
  } else {
    const next = [...musicCollections.value];
    next[index] = collection;
    musicCollections.value = next;
  }
  collectionRefreshKey.value += 1;
}

function openCreateCollection(seed: CollectionSeed = { kind: "empty" }) {
  collectionPickerSeed.value = null;
  collectionNameDialog.value = { intent: "create", seed };
  collectionName.value = "";
  collectionActionError.value = null;
  closeSidebarContextMenus();
  void nextTick(() => collectionNameInput.value?.focus());
}

function openRenameCollection(collection: MusicCollectionSummary) {
  collectionNameDialog.value = { intent: "rename", collectionId: collection.id };
  collectionName.value = collection.name;
  collectionActionError.value = null;
  closeSidebarContextMenus();
  void nextTick(() => {
    collectionNameInput.value?.focus();
    collectionNameInput.value?.select();
  });
}

function openCollectionPicker(seed: Exclude<CollectionSeed, { kind: "empty" }>) {
  const options = seed.kind === "collection"
    ? musicCollections.value.filter((collection) => collection.id !== seed.sourceCollectionId)
    : musicCollections.value;
  if (!options.length) {
    openCreateCollection(seed);
    return;
  }
  collectionPickerSeed.value = seed;
  selectedCollectionId.value = options[0].id;
  collectionActionError.value = null;
}

function collectionPickerOptions() {
  const seed = collectionPickerSeed.value;
  return seed?.kind === "collection"
    ? musicCollections.value.filter((collection) => collection.id !== seed.sourceCollectionId)
    : musicCollections.value;
}

function addLocalSelectionToCollection(source: LocalCollectionSelection) {
  openCollectionPicker({ kind: "local", ...source });
}

function createCollectionFromLocalSelection(source: LocalCollectionSelection) {
  openCreateCollection({ kind: "local", ...source });
}

function addOnlineTracksToCollection(tracks: OnlineTrack[]) {
  if (tracks.length) openCollectionPicker({ kind: "online", tracks: [...tracks] });
}

function createCollectionFromOnlineTracks(tracks: OnlineTrack[]) {
  if (tracks.length) openCreateCollection({ kind: "online", tracks: [...tracks] });
}

function addCollectionItemsToCollection(source: CollectionItemSelection) {
  if (source.itemIds.length) openCollectionPicker({ kind: "collection", ...source });
}

function createCollectionFromCollectionItems(source: CollectionItemSelection) {
  if (source.itemIds.length) openCreateCollection({ kind: "collection", ...source });
}

function closeCollectionDialogs() {
  if (collectionActionBusy.value) return;
  collectionPickerSeed.value = null;
  collectionNameDialog.value = null;
  collectionDeleteTarget.value = null;
  collectionActionError.value = null;
}

function createCollectionFromPicker() {
  const seed = collectionPickerSeed.value;
  if (seed && seed.kind !== "empty") openCreateCollection(seed);
}

async function submitCollectionName() {
  const dialog = collectionNameDialog.value;
  if (!dialog || collectionActionBusy.value) return;
  collectionActionBusy.value = true;
  collectionActionError.value = null;

  let created: MusicCollectionSummary | null = null;
  try {
    if (dialog.intent === "rename") {
      const renamed = await renameMusicCollection(dialog.collectionId, collectionName.value);
      updateCollectionSummary(renamed);
      collectionNameDialog.value = null;
      showCollectionNotice(`Renamed Collection to ${renamed.name}.`);
      return;
    }

    created = await createMusicCollection(collectionName.value);
    const mutation = await addCollectionSeed(created.id, dialog.seed);
    updateCollectionSummary(mutation?.collection ?? created);
    collectionsExpanded.value = true;
    collectionNameDialog.value = null;
    selectCollection(created.id);
    showCollectionNotice(
      mutation
        ? collectionMutationMessage(mutation, created.name)
        : `Created ${created.name}.`,
    );
  } catch (error) {
    if (created) {
      await deleteMusicCollection(created.id).catch(() => undefined);
    }
    collectionActionError.value = normalizeError(error);
  } finally {
    collectionActionBusy.value = false;
  }
}

async function confirmCollectionAdd() {
  const seed = collectionPickerSeed.value;
  if (!seed || seed.kind === "empty" || !selectedCollectionId.value) return;
  collectionActionBusy.value = true;
  collectionActionError.value = null;
  try {
    const mutation = await addCollectionSeed(selectedCollectionId.value, seed);
    if (!mutation) return;
    updateCollectionSummary(mutation.collection);
    collectionPickerSeed.value = null;
    collectionsExpanded.value = true;
    showCollectionNotice(collectionMutationMessage(mutation, mutation.collection.name));
  } catch (error) {
    collectionActionError.value = normalizeError(error);
  } finally {
    collectionActionBusy.value = false;
  }
}

function addCollectionSeed(collectionId: string, seed: CollectionSeed) {
  if (seed.kind === "local") {
    return addLocalSelectionToMusicCollection(collectionId, seed);
  }
  if (seed.kind === "online") {
    return addOnlineTracksToMusicCollection(collectionId, seed.tracks);
  }
  if (seed.kind === "collection") {
    return addMusicCollectionItemsToMusicCollection(collectionId, seed);
  }
  return Promise.resolve<MusicCollectionMutation | null>(null);
}

function collectionSeedCount(seed: CollectionSeed | null) {
  if (!seed || seed.kind === "empty") return 0;
  if (seed.kind === "online") return seed.tracks.length;
  if (seed.kind === "collection") return seed.itemIds.length;
  if (seed.selection.selectAll) return null;
  return seed.selection.ranges.reduce(
    (total, range) => total + Math.abs(range.end - range.start) + 1,
    0,
  );
}

function collectionMutationMessage(
  mutation: MusicCollectionMutation,
  collectionNameValue: string,
) {
  if (!mutation.added) {
    return `The selected tracks are already in ${collectionNameValue}.`;
  }
  const duplicateSuffix = mutation.skipped
    ? ` ${mutation.skipped} duplicate${mutation.skipped === 1 ? " was" : "s were"} skipped.`
    : "";
  return `Added ${mutation.added} track${mutation.added === 1 ? "" : "s"} to ${collectionNameValue}.${duplicateSuffix}`;
}

function showCollectionNotice(message: string) {
  collectionNotice.value = message;
  if (collectionNoticeTimer) clearTimeout(collectionNoticeTimer);
  collectionNoticeTimer = setTimeout(() => {
    collectionNotice.value = null;
    collectionNoticeTimer = null;
  }, 4_000);
}

function openLocalMusicContextMenu(event: MouseEvent) {
  event.preventDefault();
  collectionContextMenu.value = null;
  localMusicContextMenu.value = appMenuPosition(event.clientX, event.clientY, 220, 96);
}

function openCollectionContextMenu(event: MouseEvent, collectionId: string) {
  event.preventDefault();
  localMusicContextMenu.value = null;
  collectionContextMenu.value = {
    ...appMenuPosition(event.clientX, event.clientY, 220, 128),
    collectionId,
  };
}

function closeSidebarContextMenus() {
  localMusicContextMenu.value = null;
  collectionContextMenu.value = null;
}

function appMenuPosition(x: number, y: number, width: number, height: number) {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  };
}

function collectionForContextMenu() {
  return musicCollections.value.find(
    (collection) => collection.id === collectionContextMenu.value?.collectionId,
  ) ?? null;
}

function renameContextCollection() {
  const collection = collectionForContextMenu();
  if (collection) openRenameCollection(collection);
}

function deleteContextCollection() {
  const collection = collectionForContextMenu();
  if (collection) requestCollectionDelete(collection);
}

function requestCollectionDelete(collection: MusicCollectionSummary) {
  collectionDeleteTarget.value = collection;
  closeSidebarContextMenus();
}

async function confirmCollectionDelete() {
  const collection = collectionDeleteTarget.value;
  if (!collection || collectionActionBusy.value) return;
  collectionActionBusy.value = true;
  collectionActionError.value = null;
  try {
    await deleteMusicCollection(collection.id);
    musicCollections.value = musicCollections.value.filter(
      (candidate) => candidate.id !== collection.id,
    );
    if (activeCollectionId.value === collection.id) {
      clearCollectionPlaybackQueue();
      replaceCurrentLocation({ section: "local", pluginId: null, collectionId: null });
    }
    collectionDeleteTarget.value = null;
    showCollectionNotice(`Deleted ${collection.name}.`);
  } catch (error) {
    collectionActionError.value = normalizeError(error);
  } finally {
    collectionActionBusy.value = false;
  }
}

function handleCollectionDragOver(event: DragEvent, collectionId: string) {
  const types = event.dataTransfer ? Array.from(event.dataTransfer.types) : [];
  if (!types.includes(COLLECTION_DRAG_TYPE)) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
  collectionDropTargetId.value = collectionId;
}

function handleCollectionDragLeave(event: DragEvent, collectionId: string) {
  const currentTarget = event.currentTarget;
  if (
    currentTarget instanceof Element
    && event.relatedTarget instanceof Node
    && currentTarget.contains(event.relatedTarget)
  ) {
    return;
  }
  if (collectionDropTargetId.value === collectionId) collectionDropTargetId.value = null;
}

async function handleCollectionDrop(event: DragEvent, collectionId: string) {
  event.preventDefault();
  collectionDropTargetId.value = null;
  const payload = readCollectionDragPayload(event.dataTransfer);
  if (!payload) return;
  try {
    const mutation = await addCollectionSeed(collectionId, payload);
    if (!mutation) return;
    updateCollectionSummary(mutation.collection);
    showCollectionNotice(collectionMutationMessage(mutation, mutation.collection.name));
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

function updatePluginRecords(records: PluginRecord[]) {
  pluginRecords.value = records;
  onlineMusicConfig.invalidateChannels();
  if (
    activeSection.value === "plugin" &&
    !records.some((plugin) => plugin.id === activePluginId.value && plugin.enabled)
  ) {
    replaceCurrentLocation({ section: "plugins", pluginId: null, collectionId: null });
  }
}

function updateAudioSourceRecords(records: AudioSourceRecord[]) {
  audioSourceRecords.value = records;
  invalidateOnlinePlaybackCaches();
  cancelOnlinePreload();
  const sourceOptions = buildAudioSourceOptions(records);
  if (!sourceOptions.some((source) => source.value === playbackAudioSourceId.value)) {
    playbackAudioSourceId.value = sourceOptions[0]?.value ?? "";
  }
}

async function applyTheme(theme: ThemePreference, coverUrl: string | null) {
  if (typeof document === "undefined") {
    return;
  }

  const generation = ++dynamicThemeGeneration;
  const root = document.documentElement;
  clearCoverTheme(root);
  dynamicThemeStatus.value = theme === "dynamic"
    ? coverUrl ? "loading" : "waiting"
    : "idle";

  if (theme === "system" || theme === "dynamic") {
    document.documentElement.removeAttribute("data-theme");
  } else {
    document.documentElement.dataset.theme = theme;
  }

  if (theme !== "dynamic" || !coverUrl) {
    return;
  }

  const coverTheme = await extractCoverTheme(coverUrl);
  if (
    generation !== dynamicThemeGeneration
    || themePreference.value !== "dynamic"
    || nowPlayingCoverUrl.value !== coverUrl
  ) {
    return;
  }
  if (coverTheme) {
    applyCoverTheme(root, coverTheme);
    dynamicThemeStatus.value = "active";
  } else {
    dynamicThemeStatus.value = "unavailable";
  }
}

function resetUiPreferences() {
  themePreference.value = DEFAULT_UI_PREFERENCES.theme;
  layoutDensity.value = DEFAULT_UI_PREFERENCES.density;
  remoteQuality.value = DEFAULT_UI_PREFERENCES.streamQuality;
  playbackAudioSourceId.value = DEFAULT_UI_PREFERENCES.audioSourceId;
  volume.value = DEFAULT_UI_PREFERENCES.volume;
  playbackMode.value = DEFAULT_UI_PREFERENCES.playbackMode;
  resetNowPlayingLyricsPreferences();
  resetDesktopLyricsPreferences();
}

function updateNowPlayingLyricsPreferences(
  patch: Partial<NowPlayingLyricsPreferences>,
) {
  nowPlayingLyricsPreferences.value = parseNowPlayingLyricsPreferences({
    ...nowPlayingLyricsPreferences.value,
    ...patch,
  });
}

function resetNowPlayingLyricsPreferences() {
  nowPlayingLyricsPreferences.value = {
    ...DEFAULT_NOW_PLAYING_LYRICS_PREFERENCES,
  };
}

function updateDesktopLyricsPreferences(patch: Partial<DesktopLyricsPreferences>) {
  desktopLyricsPreferences.value = parseDesktopLyricsPreferences({
    ...desktopLyricsPreferences.value,
    ...patch,
  });
}

function resetDesktopLyricsPreferences() {
  desktopLyricsPreferences.value = { ...DEFAULT_DESKTOP_LYRICS_PREFERENCES };
}

function toggleDesktopLyrics() {
  updateDesktopLyricsPreferences({ enabled: !desktopLyricsPreferences.value.enabled });
}

function resetPlaybackDetails(
  coverUrl: string | null,
  remoteQuery: TrackLyricsQuery | null = null,
) {
  playbackDetailsGeneration += 1;
  nowPlayingCoverUrl.value = coverUrl;
  activeLyrics.value = null;
  activeRemoteLyricsQuery.value = remoteQuery;
  lyricsError.value = null;
  isLoadingLyrics.value = false;
  return playbackDetailsGeneration;
}

async function loadLocalTrackPlaybackDetails(track: LocalTrack, reset = true) {
  const generation = reset
    ? resetPlaybackDetails(null)
    : ++playbackDetailsGeneration;
  isLoadingLyrics.value = true;
  lyricsError.value = null;

  try {
    const details = await invoke<LocalTrackPlaybackDetails>(
      TAURI_COMMANDS.localTrackPlaybackDetails,
      { trackId: track.id },
    );
    if (generation !== playbackDetailsGeneration || activeTrack.value?.id !== track.id) {
      return;
    }
    nowPlayingCoverUrl.value = details.coverDataUrl;
    activeLyrics.value = details.lyrics;
    lyricsError.value = details.lyricsError;
  } catch (error) {
    if (generation === playbackDetailsGeneration && activeTrack.value?.id === track.id) {
      lyricsError.value = normalizeError(error);
    }
  } finally {
    if (generation === playbackDetailsGeneration) {
      isLoadingLyrics.value = false;
    }
  }
}

async function loadRemoteTrackLyrics(
  query: TrackLyricsQuery,
  coverUrl: string | null,
  reset = true,
) {
  const generation = reset
    ? resetPlaybackDetails(coverUrl, query)
    : ++playbackDetailsGeneration;
  activeRemoteLyricsQuery.value = query;
  isLoadingLyrics.value = true;
  lyricsError.value = null;

  try {
    const lyrics = await invoke<ResolvedLyrics | null>(TAURI_COMMANDS.resolveRemoteTrackLyrics, {
      query,
    });
    if (generation !== playbackDetailsGeneration || activeTrack.value) {
      return;
    }
    activeLyrics.value = lyrics;
  } catch (error) {
    if (generation === playbackDetailsGeneration && !activeTrack.value) {
      lyricsError.value = normalizeError(error);
    }
  } finally {
    if (generation === playbackDetailsGeneration) {
      isLoadingLyrics.value = false;
    }
  }
}

function retryLyrics() {
  if (activeTrack.value) {
    void loadLocalTrackPlaybackDetails(activeTrack.value, false);
  } else if (activeRemoteLyricsQuery.value) {
    void loadRemoteTrackLyrics(
      activeRemoteLyricsQuery.value,
      nowPlayingCoverUrl.value,
      false,
    );
  }
}

async function playTrack(track: LocalTrack, preserveCollectionQueue = false) {
  sampleListeningTime();
  if (!preserveCollectionQueue) clearCollectionPlaybackQueue();
  clearRemotePlaybackQueue();
  isPreparingPlayback.value = true;
  appError.value = null;

  try {
    const source = await invoke<MediaSource>(TAURI_COMMANDS.localTrackMediaSource, {
      trackId: track.id,
    });

    activeTrack.value = track;
    activeRemoteTitle.value = null;
    activeRemoteQuality.value = null;
    audioUrl.value = convertFileSrc(source.filePath);
    queuedLocalTrack.value = track;
    resetListeningSession();
    void loadLocalTrackPlaybackDetails(track);

    await nextTick();
    if (audioElement.value) {
      audioElement.value.volume = volume.value;
      await audioElement.value.play();
      isPlaying.value = true;
    }
  } catch (error) {
    appError.value = normalizeError(error);
    isPlaying.value = false;
  } finally {
    isPreparingPlayback.value = false;
  }
}

async function handleLibraryPlaybackQueue(queue: LibraryPlaybackQueue, autoplay: boolean) {
  clearCollectionPlaybackQueue();
  localQueueId.value = queue.queueId;
  localQueueTotal.value = queue.total;
  localQueueIndex.value = queue.currentIndex;
  queuedLocalTrack.value = queue.track;
  localQueueActive.value = autoplay;
  if (autoplay) {
    await playTrack(queue.track);
  }
}

function clearLocalPlaybackQueue() {
  localQueueId.value = null;
  localQueueTotal.value = 0;
  localQueueIndex.value = -1;
  queuedLocalTrack.value = null;
  localQueueActive.value = false;
}

function clearCollectionPlaybackQueue() {
  collectionQueue.value = [];
  collectionQueueIndex.value = -1;
  collectionQueueActive.value = false;
}

function clearRemotePlaybackQueue() {
  remoteQueueGeneration += 1;
  onlinePlaybackController?.abort();
  onlinePlaybackController = null;
  cancelOnlinePreload();
  remoteQueue.value = [];
  remoteQueueIndex.value = -1;
  remoteQueueActive.value = false;
  remoteQueueLoadMore.value = null;
  pendingRemoteQueueLoad = null;
  resolvingOnlineTrackKey.value = null;
  activeOnlineTrack.value = null;
  activeRemoteQuality.value = null;
  activeOnlineAudioSourceId.value = null;
  activeOnlineChannelId.value = null;
  activeOnlineAttemptKey.value = null;
  activeOnlineUrl.value = null;
  sourceChangeMessage.value = null;
  if (sourceChangeMessageTimer) {
    clearTimeout(sourceChangeMessageTimer);
    sourceChangeMessageTimer = null;
  }
}

function updateLibrarySummary(summary: { libraryTotal: number; filteredTotal: number }) {
  libraryTrackCount.value = summary.libraryTotal;
  filteredLibraryTrackCount.value = summary.filteredTotal;
}

function showLibraryError(message: string) {
  appError.value = message;
}

function favoriteActiveOnlineTrack() {
  const track = activeOnlineTrack.value;
  if (track) void onlineMusic.value?.addToFavorites(track);
}

function addActiveOnlineTrackToPlaylist() {
  closePlaybackOptionsMenu();
  const track = activeOnlineTrack.value;
  if (track) void onlineMusic.value?.openPlaylistPicker(track);
}

function downloadActiveOnlineTrack() {
  closePlaybackOptionsMenu();
  const track = activeOnlineTrack.value;
  if (track) void onlineMusic.value?.downloadTrack(track);
}

function closePlaybackOptionsMenu() {
  playbackOptionsMenu.value?.removeAttribute("open");
}

function handlePlaybackOptionsOutsideClick(event: MouseEvent) {
  const menu = playbackOptionsMenu.value;
  if (menu?.open && event.target instanceof Node && !menu.contains(event.target)) {
    closePlaybackOptionsMenu();
  }
}

function changePlaybackAudioSource(audioSourceId: string) {
  const track = activeOnlineTrack.value;
  const shouldReload = Boolean(
    track && audioSourceId !== activeOnlineAudioSourceId.value,
  );
  playbackAudioSourceId.value = audioSourceId;
  cancelOnlinePreload();
  closePlaybackOptionsMenu();
  if (track && shouldReload) {
    clearFailedOnlinePlayback(track.key);
    clearOnlinePlaybackFailures(track.key);
    void reloadActiveOnlinePlayback("Changing audio source");
  }
}

function changePlaybackQuality(quality: StreamQuality) {
  const track = activeOnlineTrack.value;
  const shouldReload = Boolean(
    track && quality !== activeRemoteQuality.value,
  );
  remoteQuality.value = quality;
  cancelOnlinePreload();
  closePlaybackOptionsMenu();
  if (track && shouldReload) {
    clearFailedOnlinePlayback(track.key);
    clearOnlinePlaybackFailures(track.key);
    void reloadActiveOnlinePlayback("Changing audio quality");
  }
}

async function handleOnlinePlayRequest(
  track: OnlineTrack,
  queue: OnlineTrack[],
  index: number,
  appendable: boolean,
  loadMore?: OnlineQueueLoadMore,
) {
  clearCollectionPlaybackQueue();
  remoteQueueGeneration += 1;
  cancelOnlinePreload();
  clearFailedOnlinePlayback(track.key);
  clearOnlinePlaybackFailures(track.key);
  const targetIndex = index >= 0 ? index : queue.findIndex((item) => item.key === track.key);
  remoteQueue.value = appendable ? queue : [...queue];
  remoteQueueActive.value = true;
  remoteQueueLoadMore.value = loadMore ?? null;
  pendingRemoteQueueLoad = null;
  await playOnlineQueueTrack(Math.max(0, targetIndex));
}

async function handleCollectionPlayback(
  items: MusicCollectionItem[],
  index: number,
  autoplay = true,
) {
  if (!items[index]) return;
  clearLocalPlaybackQueue();
  clearRemotePlaybackQueue();
  collectionQueue.value = [...items];
  collectionQueueIndex.value = index;
  collectionQueueActive.value = autoplay;
  if (autoplay) await playCollectionQueueTrack(index);
}

async function playCollectionQueueTrack(index: number) {
  const item = collectionQueue.value[index];
  if (!item) return;
  collectionQueueIndex.value = index;
  if (item.localTrack) {
    await playTrack(item.localTrack, true);
    return;
  }
  if (item.onlineTrack) {
    await playCollectionOnlineTrack(item.onlineTrack);
  }
}

async function playCollectionOnlineTrack(track: OnlineTrack) {
  clearRemotePlaybackQueue();
  const controller = new AbortController();
  onlinePlaybackController = controller;
  resolvingOnlineTrackKey.value = track.key;
  isPreparingPlayback.value = true;
  appError.value = null;
  clearFailedOnlinePlayback(track.key);
  clearOnlinePlaybackFailures(track.key);

  try {
    const playback = await resolveConfiguredOnlinePlayback(track, controller.signal, true);
    if (controller.signal.aborted || onlinePlaybackController !== controller) return;
    await applyOnlinePlayback(playback);
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      appError.value = normalizeError(error);
    }
  } finally {
    if (onlinePlaybackController === controller) {
      resolvingOnlineTrackKey.value = null;
      isPreparingPlayback.value = false;
    }
  }
}

async function playOnlineQueueTrack(index: number) {
  const snapshotTrack = remoteQueue.value[index];
  if (!snapshotTrack) return;

  const prepared = takePreloadedOnlinePlayback(index, snapshotTrack);
  if (prepared) {
    onlinePlaybackController?.abort();
    resolvingOnlineTrackKey.value = snapshotTrack.key;
    isPreparingPlayback.value = true;
    appError.value = null;
    try {
      remoteQueueIndex.value = index;
      await applyOnlinePlayback(prepared);
      if (index === remoteQueue.value.length - 1) void loadMoreRemoteQueue();
    } catch (error) {
      appError.value = normalizeError(error);
    } finally {
      clearPreloadedMedia();
      resolvingOnlineTrackKey.value = null;
      isPreparingPlayback.value = false;
    }
    return;
  }

  cancelOnlinePreload();
  onlinePlaybackController?.abort();
  const controller = new AbortController();
  onlinePlaybackController = controller;
  resolvingOnlineTrackKey.value = snapshotTrack.key;
  isPreparingPlayback.value = true;
  appError.value = null;

  try {
    const playback = await resolveConfiguredOnlinePlayback(
      snapshotTrack,
      controller.signal,
      true,
    );
    if (controller.signal.aborted || onlinePlaybackController !== controller) return;
    remoteQueueIndex.value = index;
    await applyOnlinePlayback(playback);
    if (index === remoteQueue.value.length - 1) void loadMoreRemoteQueue();
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      appError.value = normalizeError(error);
    }
  } finally {
    if (onlinePlaybackController === controller) {
      resolvingOnlineTrackKey.value = null;
      isPreparingPlayback.value = false;
    }
  }
}

async function resolveConfiguredOnlinePlayback(
  track: OnlineTrack,
  signal: AbortSignal,
  requireEnabledChannel: boolean,
  preload = false,
  bypassResolvedCache = false,
) {
  const { settings, channels } = await onlineMusicConfig.load();
  audioSourceSelectionModeGeneration += 1;
  audioSourceSelectionMode.value = settings.audioSourceSelectionMode;
  const enabledChannels = new Set(channels.map((channel) => channel.id));
  const candidates = requireEnabledChannel
    ? track.candidates.filter((candidate) => enabledChannels.has(candidate.channelId))
    : track.candidates;
  if (!candidates.length) {
    throw new Error("Playback is unavailable from the configured Audio Sources.");
  }
  const resolvedTrack = candidates === track.candidates ? track : { ...track, candidates };
  return resolveOnlineTrack({
    track: resolvedTrack,
    audioSources: audioSourceRecords.value,
    settings,
    selectedAudioSourceId: playbackAudioSourceId.value,
    quality: remoteQuality.value,
    signal,
    excludedAttempts: activeFailedAttempts(track.key),
    excludedUrls: activeFailedUrls(track.key),
    probe: preload ? preloadMediaUrl : undefined,
    cacheFailures: !preload,
    bypassResolvedCache,
  });
}

function handleOnlineMusicSettingsChanged(
  settings: OnlineMusicSettings,
) {
  audioSourceSelectionModeGeneration += 1;
  audioSourceSelectionMode.value = settings.audioSourceSelectionMode;
  cancelOnlinePreload();
  onlineMusicConfig.updateSettings(settings);
}

function nextRemoteQueueIndexForPreload() {
  if (playbackMode.value === "shuffle") return -1;
  const nextIndex = remoteQueueIndex.value + 1;
  if (nextIndex < remoteQueue.value.length) return nextIndex;
  if (remoteQueueLoadMore.value) return -1;
  return playbackMode.value === "repeat" && remoteQueue.value.length ? 0 : -1;
}

function scheduleNextOnlinePreload(delayMs = 750) {
  if (!remoteQueueActive.value || !isPlaying.value) return;
  const index = nextRemoteQueueIndexForPreload();
  if (index < 0) return;
  if (preloadedOnlinePlayback?.index === index) return;
  if (onlinePreloadTimer) clearTimeout(onlinePreloadTimer);
  onlinePreloadTimer = window.setTimeout(() => {
    onlinePreloadTimer = null;
    void preloadOnlineQueueTrack(index);
  }, delayMs);
}

async function preloadOnlineQueueTrack(index: number, refresh = false) {
  const track = remoteQueue.value[index];
  if (!track || index !== nextRemoteQueueIndexForPreload()) return;
  onlinePreloadController?.abort();
  const controller = new AbortController();
  onlinePreloadController = controller;
  try {
    const playback = await resolveConfiguredOnlinePlayback(
      track,
      controller.signal,
      true,
      true,
      refresh,
    );
    if (
      controller.signal.aborted
      || onlinePreloadController !== controller
      || index !== nextRemoteQueueIndexForPreload()
      || remoteQueue.value[index]?.key !== track.key
    ) {
      clearPreloadedMedia();
      return;
    }
    clearPreloadedMedia(playback.url);
    preloadedOnlinePlayback = { index, playback, preparedAt: Date.now() };
  } catch (error) {
    clearPreloadedMedia();
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      preloadedOnlinePlayback = null;
    }
  } finally {
    if (onlinePreloadController === controller) onlinePreloadController = null;
  }
}

function takePreloadedOnlinePlayback(index: number, track: OnlineTrack) {
  const prepared = preloadedOnlinePlayback;
  if (!prepared || prepared.index !== index || prepared.playback.track.key !== track.key) {
    return null;
  }
  preloadedOnlinePlayback = null;
  onlinePreloadController?.abort();
  onlinePreloadController = null;
  return prepared.playback;
}

function cancelOnlinePreload() {
  if (onlinePreloadTimer) clearTimeout(onlinePreloadTimer);
  onlinePreloadTimer = null;
  onlinePreloadController?.abort();
  onlinePreloadController = null;
  preloadedOnlinePlayback = null;
  clearPreloadedMedia();
}

async function playStandaloneOnlineTrack(track: OnlineTrack) {
  onlinePlaybackController?.abort();
  const controller = new AbortController();
  onlinePlaybackController = controller;
  resolvingOnlineTrackKey.value = track.key;
  isPreparingPlayback.value = true;
  appError.value = null;

  try {
    const playback = await resolveConfiguredOnlinePlayback(track, controller.signal, false);
    if (controller.signal.aborted || onlinePlaybackController !== controller) return;
    await applyOnlinePlayback(playback);
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      appError.value = normalizeError(error);
    }
  } finally {
    if (onlinePlaybackController === controller) {
      resolvingOnlineTrackKey.value = null;
      isPreparingPlayback.value = false;
    }
  }
}

async function applyOnlinePlayback(playback: OnlinePlayback) {
  sampleListeningTime();
  clearLocalPlaybackQueue();
  activeTrack.value = null;
  activeOnlineTrack.value = playback.track;
  activeRemoteTitle.value = playback.track.title;
  activeRemoteQuality.value = playback.quality;
  activeOnlineAudioSourceId.value = playback.audioSourceId;
  activeOnlineChannelId.value = playback.candidate.channelId;
  activeOnlineAttemptKey.value = activeOnlineChannelId.value
    ? playbackAttemptKey(playback.audioSourceId, activeOnlineChannelId.value, playback.quality)
    : null;
  activeOnlineUrl.value = playback.url;
  audioUrl.value = playback.url;
  void loadRemoteTrackLyrics(
    {
      title: playback.track.title,
      artist: playback.track.artist || null,
      album: playback.track.album,
      durationSeconds: playback.track.durationSeconds,
      source: playback.candidate.sourceId,
      trackId: playback.candidate.id,
    },
    playback.track.coverUrl,
  );

  await nextTick();
  if (audioElement.value) {
    audioElement.value.volume = volume.value;
    await audioElement.value.play();
    isPlaying.value = true;
  }
}

async function togglePlayback() {
  if (
    !collectionQueueActive.value
    && collectionQueue.value.length
    && collectionQueueIndex.value >= 0
    && (!audioElement.value || audioElement.value.ended)
  ) {
    collectionQueueActive.value = true;
    await playCollectionQueueTrack(collectionQueueIndex.value);
    return;
  }
  if (
    collectionQueueActive.value
    && collectionQueueIndex.value >= 0
    && (!audioElement.value || audioElement.value.ended)
  ) {
    await playCollectionQueueTrack(collectionQueueIndex.value);
    return;
  }
  if (
    !localQueueActive.value &&
    queuedLocalTrack.value &&
    (!audioElement.value || audioElement.value.ended)
  ) {
    localQueueActive.value = true;
    await playTrack(queuedLocalTrack.value);
    return;
  }
  if (!audioElement.value) {
    if (activeTrack.value) {
      await playTrack(activeTrack.value);
      return;
    }

    if (queuedLocalTrack.value) {
      localQueueActive.value = true;
      await playTrack(queuedLocalTrack.value);
      return;
    }
    if (activeSection.value === "collection") {
      await collectionBrowser.value?.startFirstTrack();
    } else {
      await libraryBrowser.value?.startFirstTrack();
    }
    return;
  }

  if (audioElement.value.paused) {
    await audioElement.value.play();
    isPlaying.value = true;
  } else {
    audioElement.value.pause();
    isPlaying.value = false;
  }
}

function cyclePlaybackMode() {
  switch (playbackMode.value) {
    case "sequential":
      playbackMode.value = "shuffle";
      break;
    case "shuffle":
      playbackMode.value = "repeat";
      break;
    default:
      playbackMode.value = "sequential";
  }
}

async function playPreviousTrack() {
  if (collectionQueueActive.value) {
    const index = collectionQueueNavigationIndex("previous");
    if (index >= 0) await playCollectionQueueTrack(index);
    return;
  }
  if (remoteQueueActive.value) {
    const index = remoteQueueNavigationIndex("previous");
    if (index >= 0) await playOnlineQueueTrack(index);
    return;
  }
  if (
    !activeTrack.value ||
    !localQueueActive.value ||
    !localQueueId.value ||
    localQueueIndex.value < 0 ||
    localQueueTotal.value === 0
  ) {
    return;
  }

  let previousIndex: number;
  if (playbackMode.value === "shuffle") {
    previousIndex = randomQueueIndex(localQueueIndex.value, localQueueTotal.value);
  } else if (localQueueIndex.value > 0) {
    previousIndex = localQueueIndex.value - 1;
  } else if (playbackMode.value === "repeat") {
    previousIndex = localQueueTotal.value - 1;
  } else {
    return;
  }

  await playLocalQueueTrack(previousIndex);
}

async function playNextTrack() {
  if (collectionQueueActive.value) {
    const index = collectionQueueNavigationIndex("next");
    if (index >= 0) await playCollectionQueueTrack(index);
    return;
  }
  if (remoteQueueActive.value) {
    if (
      remoteQueueLoadMore.value &&
      remoteQueueIndex.value === remoteQueue.value.length - 1
    ) {
      await loadMoreRemoteQueue();
    }
    const index = remoteQueueNavigationIndex("next");
    if (index >= 0) await playOnlineQueueTrack(index);
    return;
  }
  if (
    !activeTrack.value ||
    !localQueueActive.value ||
    !localQueueId.value ||
    localQueueIndex.value < 0 ||
    localQueueTotal.value === 0
  ) {
    return;
  }

  let nextIndex: number;
  if (playbackMode.value === "shuffle") {
    nextIndex = randomQueueIndex(localQueueIndex.value, localQueueTotal.value);
  } else if (localQueueIndex.value < localQueueTotal.value - 1) {
    nextIndex = localQueueIndex.value + 1;
  } else if (playbackMode.value === "repeat") {
    nextIndex = 0;
  } else {
    return;
  }

  await playLocalQueueTrack(nextIndex);
}

function loadMoreRemoteQueue() {
  if (pendingRemoteQueueLoad) return pendingRemoteQueueLoad;
  const loadMore = remoteQueueLoadMore.value;
  if (!loadMore) return Promise.resolve([]);

  const generation = remoteQueueGeneration;
  const load = loadMore()
    .then((tracks) => {
      if (generation === remoteQueueGeneration && tracks.length) {
        scheduleNextOnlinePreload();
      }
      return tracks;
    })
    .catch(() => []);
  pendingRemoteQueueLoad = load;
  void load.finally(() => {
    if (generation === remoteQueueGeneration && pendingRemoteQueueLoad === load) {
      pendingRemoteQueueLoad = null;
    }
  });
  return load;
}

function remoteQueueNavigationIndex(direction: "previous" | "next") {
  const total = remoteQueue.value.length;
  const current = remoteQueueIndex.value;
  if (!total || current < 0) return -1;
  if (playbackMode.value === "shuffle") {
    if (total === 1) return 0;
    const candidate = Math.floor(Math.random() * (total - 1));
    return candidate >= current ? candidate + 1 : candidate;
  }
  const offset = direction === "previous" ? -1 : 1;
  const candidate = current + offset;
  if (candidate >= 0 && candidate < total) return candidate;
  if (playbackMode.value !== "repeat") return -1;
  return direction === "previous" ? total - 1 : 0;
}

async function playLocalQueueTrack(index: number) {
  const queueId = localQueueId.value;
  if (!queueId) {
    return;
  }
  try {
    const queuedTrack = await invoke<LibraryQueueTrack>(TAURI_COMMANDS.localLibraryQueueTrack, {
      queueId,
      index,
    });
    localQueueIndex.value = queuedTrack.index;
    queuedLocalTrack.value = queuedTrack.track;
    localQueueActive.value = true;
    await playTrack(queuedTrack.track);
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

function collectionQueueNavigationIndex(direction: "previous" | "next") {
  const total = collectionQueue.value.length;
  const current = collectionQueueIndex.value;
  if (!total || current < 0) return -1;
  if (playbackMode.value === "shuffle") return randomQueueIndex(current, total);
  const offset = direction === "previous" ? -1 : 1;
  const candidate = current + offset;
  if (candidate >= 0 && candidate < total) return candidate;
  if (playbackMode.value === "repeat") return direction === "previous" ? total - 1 : 0;
  return -1;
}

function randomQueueIndex(currentIndex: number, total: number) {
  if (total <= 1) {
    return 0;
  }

  const candidate = Math.floor(Math.random() * (total - 1));
  return candidate >= currentIndex ? candidate + 1 : candidate;
}

function updateVolume() {
  if (audioElement.value) {
    audioElement.value.volume = volume.value;
  }
}

async function onAudioEnded() {
  pauseListeningTime();
  isPlaying.value = false;
  isPlaybackWaiting.value = false;
  playbackPosition.value = playbackDuration.value;
  if (
    !collectionQueueActive.value
    && collectionQueue.value.length
    && collectionQueueIndex.value >= 0
  ) {
    collectionQueueActive.value = true;
    await playCollectionQueueTrack(collectionQueueIndex.value);
    return;
  }
  if (!localQueueActive.value && localQueueId.value && queuedLocalTrack.value) {
    localQueueActive.value = true;
    await playTrack(queuedLocalTrack.value);
    return;
  }
  await playNextTrack();
}

function onAudioPause() {
  pauseListeningTime();
  isPlaying.value = false;
  isPlaybackWaiting.value = false;
}

function onAudioPlay() {
  isPlaying.value = true;
  isPlaybackWaiting.value = false;
  if (activeTrack.value) {
    playCountTracker.start(performance.now());
  }
}

function onAudioWaiting() {
  pauseListeningTime();
  isPlaybackWaiting.value = true;
}

function onAudioPlaying() {
  isPlaybackWaiting.value = false;
  scheduleNextOnlinePreload();
  if (activeTrack.value) {
    playCountTracker.start(performance.now());
  }
}

function onAudioLoadedMetadata() {
  syncPlaybackTimeline();
}

function onAudioTimeUpdate() {
  sampleListeningTime();
  syncPlaybackTimeline();
  if (
    preloadedOnlinePlayback
    && playbackDuration.value > 0
    && playbackDuration.value - playbackPosition.value <= 30
    && Date.now() - preloadedOnlinePlayback.preparedAt > 90_000
  ) {
    const index = preloadedOnlinePlayback.index;
    cancelOnlinePreload();
    void preloadOnlineQueueTrack(index, true);
  }
}

function resetListeningSession() {
  playCountTracker.reset();
}

function sampleListeningTime() {
  const track = activeTrack.value;
  if (!track) {
    return;
  }
  const duration = playbackDuration.value || track.durationSeconds || 0;
  if (
    playCountTracker.sample(
      performance.now(),
      duration,
      audioElement.value?.playbackRate || 1,
    )
  ) {
    void recordPlayCount(track);
  }
}

function pauseListeningTime() {
  const track = activeTrack.value;
  if (!track) {
    return;
  }
  const duration = playbackDuration.value || track.durationSeconds || 0;
  if (
    playCountTracker.pause(
      performance.now(),
      duration,
      audioElement.value?.playbackRate || 1,
    )
  ) {
    void recordPlayCount(track);
  }
}

async function recordPlayCount(track: LocalTrack) {
  try {
    const playCount = await invoke<number>(TAURI_COMMANDS.incrementLocalTrackPlayCount, {
      trackId: track.id,
    });
    libraryBrowser.value?.updatePlayCount(track.id, playCount);
    collectionBrowser.value?.updatePlayCount(track.id, playCount);
    if (activeTrack.value?.id === track.id) {
      activeTrack.value = { ...activeTrack.value, playCount };
    }
    if (queuedLocalTrack.value?.id === track.id) {
      queuedLocalTrack.value = { ...queuedLocalTrack.value, playCount };
    }
  } catch (error) {
    appError.value = normalizeError(error);
  }
}

function syncPlaybackTimeline() {
  const audio = audioElement.value;
  if (!audio) {
    return;
  }

  playbackPosition.value = Number.isFinite(audio.currentTime) ? audio.currentTime : 0;
  playbackDuration.value = Number.isFinite(audio.duration) ? audio.duration : 0;
}

function seekPlaybackTo(position: number) {
  const audio = audioElement.value;
  if (!audio || playbackDuration.value <= 0 || !Number.isFinite(position)) {
    return;
  }

  const nextPosition = Math.min(playbackDuration.value, Math.max(0, position));
  audio.currentTime = nextPosition;
  playbackPosition.value = nextPosition;
}

function seekPlayback(event: Event) {
  seekPlaybackTo(Number((event.currentTarget as HTMLInputElement).value));
}

function onAudioError() {
  isPlaying.value = false;
  isPlaybackWaiting.value = false;
  if (
    activeOnlineTrack.value
    && (remoteQueueActive.value || collectionQueueActive.value)
  ) {
    void recoverOnlinePlayback();
    return;
  }
  appError.value = "Playback failed for the selected track.";
}

async function recoverOnlinePlayback() {
  const track = activeOnlineTrack.value;
  if (
    !track
    || (!remoteQueueActive.value && !collectionQueueActive.value)
    || (remoteQueueActive.value && remoteQueueIndex.value < 0)
  ) {
    return;
  }
  if (activeOnlineAttemptKey.value) {
    failedOnlineAttempts.set(
      `${track.key}::${activeOnlineAttemptKey.value}`,
      true,
    );
    reportOnlinePlaybackFailure(activeOnlineAttemptKey.value);
  }
  if (activeOnlineUrl.value) {
    failedOnlineUrls.set(`${track.key}::${activeOnlineUrl.value}`, true);
  }
  await reloadActiveOnlinePlayback("Changing playback source");
}

async function reloadActiveOnlinePlayback(message: string) {
  const track = activeOnlineTrack.value;
  if (!track) return;
  const priorPosition = playbackPosition.value;
  const priorDuration = playbackDuration.value || track.durationSeconds || 0;
  const wasPlaying = isPlaying.value;
  sourceChangeMessage.value = message;

  if (remoteQueueActive.value && remoteQueueIndex.value >= 0) {
    await playOnlineQueueTrack(remoteQueueIndex.value);
  } else {
    await playStandaloneOnlineTrack(track);
  }

  const nextDuration = playbackDuration.value || activeOnlineTrack.value?.durationSeconds || 0;
  if (audioElement.value && Math.abs(nextDuration - priorDuration) <= 3) {
    audioElement.value.currentTime = Math.min(priorPosition, Math.max(0, nextDuration - 0.25));
  }
  if (audioElement.value && !wasPlaying) {
    audioElement.value.pause();
    isPlaying.value = false;
  }
  if (sourceChangeMessageTimer) clearTimeout(sourceChangeMessageTimer);
  sourceChangeMessageTimer = window.setTimeout(() => {
    sourceChangeMessage.value = null;
    sourceChangeMessageTimer = null;
  }, 2_500);
}

function activeFailedAttempts(trackKey: string) {
  return new Set(
    failedOnlineAttempts
      .keysWhere((key) => key.startsWith(`${trackKey}::`))
      .map((key) => key.slice(trackKey.length + 2)),
  );
}

function activeFailedUrls(trackKey: string) {
  return new Set(
    failedOnlineUrls
      .keysWhere((key) => key.startsWith(`${trackKey}::`))
      .map((key) => key.slice(trackKey.length + 2)),
  );
}

function clearFailedOnlinePlayback(trackKey: string) {
  failedOnlineAttempts.deleteWhere((key) => key.startsWith(`${trackKey}::`));
  failedOnlineUrls.deleteWhere((key) => key.startsWith(`${trackKey}::`));
}

function formatPlaybackTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) {
    return "--:--";
  }

  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

function trackSubtitle(track: LocalTrack) {
  return [track.artist, track.album].filter(Boolean).join(" - ") || track.fileName;
}

</script>

<template>
  <div
    class="drawer h-screen overflow-hidden bg-base-200 text-base-content min-[1200px]:drawer-open"
    :data-density="layoutDensity"
  >
    <input id="app-sidebar" v-model="sidebarOpen" type="checkbox" class="drawer-toggle" />

    <div class="drawer-content flex h-screen min-h-0 min-w-0 flex-col">
      <header class="navbar z-30 min-h-16 shrink-0 border-b border-base-300 bg-base-100 px-3 sm:px-4 lg:px-6">
        <div class="navbar-start min-w-0 flex-1 gap-2 sm:gap-3">
          <label
            for="app-sidebar"
            class="btn btn-square btn-ghost btn-sm drawer-button min-[1200px]:hidden"
            role="button"
            tabindex="0"
            aria-label="Open navigation"
            title="Open navigation"
            @keydown.enter.prevent="sidebarOpen = true"
            @keydown.space.prevent="sidebarOpen = true"
          >
            <Menu :size="19" aria-hidden="true" />
          </label>
          <div class="min-w-0">
            <h1 class="truncate text-base font-semibold leading-tight sm:text-lg">
              {{ currentSection.label }}
            </h1>
            <p class="hidden truncate text-xs text-muted xl:block">
              {{ currentSection.description }}
            </p>
          </div>
        </div>

        <div v-if="activeSection === 'local'" class="navbar-end w-auto gap-2">
          <button
            class="btn btn-sm"
            type="button"
            :disabled="isChoosingFolder || scanStatus.isRunning"
            @click="chooseFolder"
          >
            <FolderOpen :size="16" aria-hidden="true" />
            Folder
          </button>
        </div>

        <div v-else-if="activeSection === 'collection' && activeCollection" class="navbar-end w-auto gap-1">
          <button
            class="btn btn-square btn-ghost btn-sm"
            type="button"
            aria-label="Rename Collection"
            title="Rename Collection"
            @click="openRenameCollection(activeCollection)"
          >
            <Pencil :size="16" aria-hidden="true" />
          </button>
          <button
            class="btn btn-square btn-ghost btn-sm text-error"
            type="button"
            aria-label="Delete Collection"
            title="Delete Collection"
            @click="requestCollectionDelete(activeCollection)"
          >
            <Trash2 :size="16" aria-hidden="true" />
          </button>
        </div>

        <div v-else-if="activeSection === 'settings'" class="navbar-end w-auto">
          <button class="btn btn-ghost btn-sm" type="button" @click="resetUiPreferences">
            <RotateCcw :size="16" aria-hidden="true" />
            Reset
          </button>
        </div>
      </header>

      <main
        class="min-h-0 flex-1"
        :class="activeSection === 'local' || activeSection === 'collection' ? 'overflow-hidden' : 'overflow-y-auto'"
      >
        <section
          v-if="activeSection === 'local'"
          class="mx-auto flex h-full min-h-0 w-full flex-col"
          :class="layoutDensity === 'compact' ? 'gap-3 px-3 py-3 lg:px-4' : 'gap-4 px-4 py-4 lg:px-6'"
        >
          <div v-if="appError" role="alert" class="alert alert-error">
            <AlertCircle :size="18" aria-hidden="true" />
            <span class="min-w-0 flex-1">{{ appError }}</span>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              aria-label="Dismiss error"
              title="Dismiss error"
              @click="appError = null"
            >
              <X :size="16" aria-hidden="true" />
            </button>
          </div>

          <div class="grid min-h-0 flex-1 grid-cols-1 gap-4 min-[1000px]:grid-cols-[minmax(0,1fr)_20rem]">
            <LibraryBrowser
              ref="libraryBrowser"
              :active-track-id="activeTrack?.id ?? null"
              :is-playing="isPlaying"
              :density="layoutDensity"
              :scan-status="scanStatus"
              :scan-message="scanMessage"
              @playback-queue="handleLibraryPlaybackQueue"
              @add-to-collection="addLocalSelectionToCollection"
              @create-collection="createCollectionFromLocalSelection"
              @summary="updateLibrarySummary"
              @error="showLibraryError"
            />

          <aside class="hidden min-h-0 flex-col min-[1000px]:flex">
          <NowPlayingPanel
            fill-height
            :title="nowPlayingTitle"
            :subtitle="nowPlayingSubtitle"
            :cover-url="nowPlayingCoverUrl"
            :lyrics="activeLyrics"
            :lyrics-loading="isLoadingLyrics"
            :lyrics-error="lyricsError"
            :playback-position="playbackPosition"
            :can-retry="Boolean(activeTrack || activeRemoteLyricsQuery)"
            :lyrics-preferences="nowPlayingLyricsPreferences"
            @retry-lyrics="retryLyrics"
            @seek-playback="seekPlaybackTo"
            @open-lyrics-settings="openNowPlayingLyricsSettings"
          />

        </aside>
      </div>
    </section>

        <section
          v-if="activeSection === 'collection' && activeCollection"
          class="mx-auto flex h-full min-h-0 w-full flex-col"
          :class="layoutDensity === 'compact' ? 'gap-3 px-3 py-3 lg:px-4' : 'gap-4 px-4 py-4 lg:px-6'"
        >
          <div v-if="appError" role="alert" class="alert alert-error">
            <AlertCircle :size="18" aria-hidden="true" />
            <span class="min-w-0 flex-1">{{ appError }}</span>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              aria-label="Dismiss error"
              title="Dismiss error"
              @click="appError = null"
            >
              <X :size="16" aria-hidden="true" />
            </button>
          </div>

          <div class="grid min-h-0 flex-1 grid-cols-1 gap-4 min-[1000px]:grid-cols-[minmax(0,1fr)_20rem]">
            <CollectionBrowser
              ref="collectionBrowser"
              :collection-id="activeCollection.id"
              :refresh-key="collectionRefreshKey"
              :active-local-track-id="activeTrack?.id ?? null"
              :active-online-track="activeOnlineTrack"
              :is-playing="isPlaying"
              :density="layoutDensity"
              @play="handleCollectionPlayback"
              @add-to-collection="addCollectionItemsToCollection"
              @create-collection="createCollectionFromCollectionItems"
              @changed="updateCollectionSummary"
              @error="showLibraryError"
            />

            <aside class="hidden min-h-0 flex-col min-[1000px]:flex">
              <NowPlayingPanel
                fill-height
                :title="nowPlayingTitle"
                :subtitle="nowPlayingSubtitle"
                :cover-url="nowPlayingCoverUrl"
                :lyrics="activeLyrics"
                :lyrics-loading="isLoadingLyrics"
                :lyrics-error="lyricsError"
                :playback-position="playbackPosition"
                :can-retry="Boolean(activeTrack || activeRemoteLyricsQuery)"
                :lyrics-preferences="nowPlayingLyricsPreferences"
                @retry-lyrics="retryLyrics"
                @seek-playback="seekPlaybackTo"
                @open-lyrics-settings="openNowPlayingLyricsSettings"
              />
            </aside>
          </div>
        </section>

        <section
          v-if="activeSection === 'sources'"
          class="mx-auto w-full max-w-7xl"
          :class="layoutDensity === 'compact' ? 'px-3 py-3 lg:px-4' : 'px-4 py-5 lg:px-6'"
        >
          <div v-if="appError" role="alert" class="alert alert-error mb-4">
            <AlertCircle :size="18" aria-hidden="true" />
            <span class="min-w-0 flex-1">{{ appError }}</span>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              aria-label="Dismiss error"
              @click="appError = null"
            >
              <X :size="16" aria-hidden="true" />
            </button>
          </div>
          <AudioSourceManager @sources-changed="updateAudioSourceRecords" />
        </section>

        <section
          v-show="activeSection === 'online'"
          class="mx-auto flex min-h-full w-full max-w-7xl flex-col"
          :class="layoutDensity === 'compact' ? 'gap-3 px-3 py-3 lg:px-4' : 'gap-4 px-4 py-4 lg:px-6'"
        >
          <div v-if="appError" role="alert" class="alert alert-error py-2">
            <AlertCircle :size="17" aria-hidden="true" />
            <span class="min-w-0 flex-1 text-sm">{{ appError }}</span>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              aria-label="Dismiss playback error"
              @click="appError = null"
            >
              <X :size="16" aria-hidden="true" />
            </button>
          </div>
          <div
            class="grid min-w-0 items-start min-[1000px]:grid-cols-[minmax(0,1fr)_20rem]"
            :class="layoutDensity === 'compact' ? 'gap-3' : 'gap-4'"
          >
            <OnlineMusic
              ref="onlineMusic"
              class="min-w-0 min-[1000px]:col-start-1 min-[1000px]:row-start-1"
              :is-active="activeSection === 'online'"
              :audio-sources="audioSourceRecords"
              :selected-audio-source-id="playbackAudioSourceId"
              :active-online-track="activeOnlineTrack"
              :resolving-online-track-key="resolvingOnlineTrackKey"
              :is-playing="isPlaying"
              :local-music-folder="selectedFolder"
              @play-request="handleOnlinePlayRequest"
              @add-to-collection="addOnlineTracksToCollection"
              @create-collection="createCollectionFromOnlineTracks"
              @open-audio-sources="selectSection('sources')"
              @open-plugin="selectPlugin"
          @toggle-playback="togglePlayback"
            />
            <NowPlayingPanel
              v-if="activeSection === 'online'"
              class="min-w-0 min-[1000px]:sticky min-[1000px]:top-4 min-[1000px]:col-start-2 min-[1000px]:row-start-1"
              :title="nowPlayingTitle"
              :subtitle="nowPlayingSubtitle"
              :cover-url="nowPlayingCoverUrl"
              :lyrics="activeLyrics"
              :lyrics-loading="isLoadingLyrics"
              :lyrics-error="lyricsError"
              :playback-position="playbackPosition"
              :can-retry="Boolean(activeTrack || activeRemoteLyricsQuery)"
              :lyrics-preferences="nowPlayingLyricsPreferences"
              @retry-lyrics="retryLyrics"
              @seek-playback="seekPlaybackTo"
              @open-lyrics-settings="openNowPlayingLyricsSettings"
            />
          </div>
        </section>

        <section
          v-if="activeSection === 'plugin' && activePlugin"
          class="mx-auto grid w-full max-w-7xl gap-4"
          :class="[
            layoutDensity === 'compact' ? 'px-3 py-3 lg:px-4' : 'px-4 py-4 lg:px-6',
            activePlugin.id === NETEASE_PLUGIN_ID || activePlugin.id === KUGOU_PLUGIN_ID
              ? ''
              : 'xl:grid-cols-[minmax(0,1fr)_20rem]',
          ]"
        >
          <NeteaseSource
            v-if="activePlugin.id === NETEASE_PLUGIN_ID"
            class="min-w-0 xl:col-start-1 xl:row-start-1"
            v-model:playback-source="playbackAudioSourceId"
            :audio-sources="availableAudioSources"
            :automatic-source-selection="automaticAudioSourceSelection"
            @open-plugins="selectSection('plugins')"
            @open-audio-sources="selectSection('sources')"
          />
          <KugouSource
            v-else-if="activePlugin.id === KUGOU_PLUGIN_ID"
            class="min-w-0 xl:col-start-1 xl:row-start-1"
            v-model:playback-source="playbackAudioSourceId"
            :audio-sources="availableAudioSources"
            :automatic-source-selection="automaticAudioSourceSelection"
            @open-plugins="selectSection('plugins')"
            @open-audio-sources="selectSection('sources')"
          />
          <PluginWorkspace
            v-else
            class="min-w-0 self-start xl:col-start-1 xl:row-start-1"
            :plugin="activePlugin"
            @open-plugins="selectSection('plugins')"
          />
          <NowPlayingPanel
            v-if="activePlugin.id !== NETEASE_PLUGIN_ID && activePlugin.id !== KUGOU_PLUGIN_ID"
            class="xl:col-start-2 xl:row-start-1"
            :title="nowPlayingTitle"
            :subtitle="nowPlayingSubtitle"
            :cover-url="nowPlayingCoverUrl"
            :lyrics="activeLyrics"
            :lyrics-loading="isLoadingLyrics"
            :lyrics-error="lyricsError"
            :playback-position="playbackPosition"
            :can-retry="Boolean(activeTrack || activeRemoteLyricsQuery)"
            :lyrics-preferences="nowPlayingLyricsPreferences"
            @retry-lyrics="retryLyrics"
            @seek-playback="seekPlaybackTo"
            @open-lyrics-settings="openNowPlayingLyricsSettings"
          />
        </section>

        <section
          v-if="activeSection === 'plugins'"
          class="mx-auto w-full max-w-7xl"
          :class="layoutDensity === 'compact' ? 'px-3 py-3 lg:px-4' : 'px-4 py-5 lg:px-6'"
        >
          <PluginManager @plugins-changed="updatePluginRecords" />
        </section>

        <section
          v-if="activeSection === 'settings'"
          class="mx-auto flex w-full max-w-4xl flex-col"
          :class="layoutDensity === 'compact' ? 'gap-3 px-3 py-3 lg:px-4' : 'gap-4 px-4 py-5 lg:px-6'"
        >
          <section class="overflow-hidden rounded border border-base-300 bg-base-100">
            <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
              <Palette :size="18" aria-hidden="true" />
              <h2 class="text-base font-semibold">Appearance</h2>
            </div>
            <div class="divide-y divide-base-300">
              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="theme-preference" class="min-w-0">
                  <span class="block text-sm font-medium">Theme</span>
                  <span class="block text-xs text-muted">Follow the device, current cover, or a fixed theme</span>
                  <span
                    v-if="themePreference === 'dynamic'"
                    class="mt-1 flex items-center gap-1.5 text-xs text-muted"
                    role="status"
                  >
                    <span
                      class="status status-xs"
                      :class="{
                        'status-primary': dynamicThemeStatus === 'active',
                        'status-info': dynamicThemeStatus === 'loading',
                        'status-warning': dynamicThemeStatus === 'unavailable',
                        'status-neutral': dynamicThemeStatus === 'waiting',
                      }"
                      aria-hidden="true"
                    ></span>
                    <span v-if="dynamicThemeStatus === 'active'">Cover colors active</span>
                    <span v-else-if="dynamicThemeStatus === 'loading'">Reading cover colors</span>
                    <span v-else-if="dynamicThemeStatus === 'unavailable'">Cover colors unavailable</span>
                    <span v-else>Waiting for cover art</span>
                  </span>
                </label>
                <select
                  id="theme-preference"
                  v-model="themePreference"
                  class="select select-sm w-full sm:w-48"
                >
                  <option
                    v-for="theme in THEME_MODE_OPTIONS"
                    :key="theme.value"
                    :value="theme.value"
                  >
                    {{ theme.label }}
                  </option>
                  <optgroup
                    v-for="group in THEME_GROUPS"
                    :key="group.value"
                    :label="group.label"
                  >
                    <option v-for="theme in group.options" :key="theme.value" :value="theme.value">
                      {{ theme.label }}
                    </option>
                  </optgroup>
                </select>
              </div>

              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="layout-density" class="flex min-w-0 items-start gap-3">
                  <Gauge class="mt-0.5 shrink-0 text-muted" :size="17" aria-hidden="true" />
                  <span>
                    <span class="block text-sm font-medium">Layout density</span>
                    <span class="block text-xs text-muted">Adjust page spacing and library rows</span>
                  </span>
                </label>
                <select
                  id="layout-density"
                  v-model="layoutDensity"
                  class="select select-sm w-full sm:w-44"
                >
                  <option value="comfortable">Comfortable</option>
                  <option value="compact">Compact</option>
                </select>
              </div>
            </div>
          </section>

          <NowPlayingLyricsSettings
            :preferences="nowPlayingLyricsPreferences"
            @update="updateNowPlayingLyricsPreferences"
            @reset="resetNowPlayingLyricsPreferences"
          />

          <DesktopLyricsSettings
            :preferences="desktopLyricsPreferences"
            @update="updateDesktopLyricsPreferences"
            @reset="resetDesktopLyricsPreferences"
          />

      <OnlineMusicSettingsPanel
        :audio-sources="audioSourceRecords"
        @settings-changed="handleOnlineMusicSettingsChanged"
          />

          <section class="overflow-hidden rounded border border-base-300 bg-base-100">
            <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
              <Headphones :size="18" aria-hidden="true" />
              <h2 class="text-base font-semibold">Playback</h2>
            </div>
            <div class="divide-y divide-base-300">
              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="stream-quality" class="min-w-0">
                  <span class="block text-sm font-medium">Default stream quality</span>
                  <span class="block text-xs text-muted">Used when resolving tracks from Audio Sources</span>
                </label>
                <select
                  id="stream-quality"
                  v-model="remoteQuality"
                  class="select select-sm w-full sm:w-44"
                >
                  <option value="128k">128 kbps</option>
                  <option value="320k">320 kbps</option>
                  <option value="flac">FLAC</option>
                  <option value="flac24bit">FLAC 24-bit</option>
                </select>
              </div>

              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="default-volume" class="min-w-0">
                  <span class="block text-sm font-medium">Volume</span>
                  <span class="block text-xs text-muted">Applied to the current and next track</span>
                </label>
                <div class="flex w-full items-center gap-3 sm:w-64">
                  <Volume2 :size="17" aria-hidden="true" />
                  <input
                    id="default-volume"
                    v-model.number="volume"
                    class="range range-sm min-h-6 min-w-0 flex-1"
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    @input="updateVolume"
                  />
                  <output class="w-10 text-right text-xs tabular-nums text-muted">
                    {{ volumePercent }}%
                  </output>
                </div>
              </div>
            </div>
          </section>

          <section class="overflow-hidden rounded border border-base-300 bg-base-100">
            <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
              <FolderOpen :size="18" aria-hidden="true" />
              <h2 class="text-base font-semibold">Library</h2>
            </div>
            <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
              <div class="min-w-0">
                <div class="text-sm font-medium">Music folder</div>
                <div class="truncate text-xs text-muted" :title="selectedFolder || undefined">
                  {{ selectedFolder || "No folder selected" }}
                </div>
              </div>
              <button
                class="btn btn-sm shrink-0"
                type="button"
                :disabled="isChoosingFolder || scanStatus.isRunning"
                @click="chooseFolder"
              >
                <FolderOpen :size="16" aria-hidden="true" />
                Change folder
              </button>
            </div>
          </section>
        </section>
      </main>

      <footer
        class="z-30 shrink-0 border-t border-base-300 bg-base-100/95 backdrop-blur"
        aria-label="Playback bar"
      >
        <div
          class="mx-auto grid w-full max-w-7xl grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-2 md:grid-cols-[minmax(0,1fr)_minmax(13rem,1.4fr)_auto] lg:grid-cols-[minmax(0,1fr)_minmax(15rem,1.5fr)_minmax(11rem,1fr)]"
          :class="layoutDensity === 'compact' ? 'px-3 py-2 lg:px-4' : 'px-4 py-3 lg:px-6'"
        >
          <div class="flex min-w-0 items-center gap-3" data-testid="playback-track-info">
            <div class="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200 sm:size-11">
              <img
                v-if="nowPlayingCoverUrl"
                class="size-full object-cover"
                :src="nowPlayingCoverUrl"
                alt=""
              />
              <Music2 v-else :size="21" aria-hidden="true" />
            </div>
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm font-medium">{{ nowPlayingTitle }}</div>
              <div class="truncate text-xs text-muted">{{ nowPlayingSubtitle }}</div>
              <div v-if="sourceChangeMessage" class="truncate text-xs text-warning">{{ sourceChangeMessage }}</div>
            </div>
          </div>

          <div
            class="col-span-2 flex min-w-0 flex-col gap-1.5 md:col-span-1 md:col-start-2 md:row-start-1"
          >
            <div class="flex h-9 items-center justify-center gap-1">
              <div
                class="tooltip tooltip-top"
                :data-tip="`${playbackModeLabel}; next: ${nextPlaybackModeLabel}`"
              >
                <button
                  class="btn btn-square btn-ghost btn-sm"
                  type="button"
                  :aria-label="`Playback mode: ${playbackModeLabel}. Change to ${nextPlaybackModeLabel}`"
                  :title="`Playback mode: ${playbackModeLabel}`"
                  data-testid="playback-mode"
                  @click="cyclePlaybackMode"
                >
                  <ListOrdered v-if="playbackMode === 'sequential'" :size="16" aria-hidden="true" />
                  <Shuffle v-else-if="playbackMode === 'shuffle'" :size="16" aria-hidden="true" />
                  <Repeat2 v-else :size="16" aria-hidden="true" />
                </button>
              </div>

              <div class="tooltip tooltip-top" data-tip="Previous">
                <button
                  class="btn btn-square btn-ghost btn-sm"
                  type="button"
                  :disabled="isPreparingPlayback || !canGoPrevious"
                  aria-label="Previous track"
                  title="Previous"
                  @click="playPreviousTrack"
                >
                  <SkipBack :size="17" aria-hidden="true" />
                </button>
              </div>

              <button
                class="btn btn-circle btn-neutral btn-sm mx-0.5 shrink-0"
                type="button"
                :disabled="isPreparingPlayback || (!activeTrack && !activeRemoteTitle && !queuedLocalTrack && !collectionQueue.length && !libraryTrackCount)"
                :aria-label="isPlaying ? 'Pause playback' : 'Play playback'"
                :title="isPlaying ? 'Pause' : 'Play'"
                @click="togglePlayback"
              >
                <RefreshCw v-if="isPreparingPlayback" class="animate-spin" :size="17" aria-hidden="true" />
                <Pause v-else-if="isPlaying" :size="17" aria-hidden="true" />
                <Play v-else :size="17" aria-hidden="true" />
              </button>

              <div class="tooltip tooltip-top" data-tip="Next">
                <button
                  class="btn btn-square btn-ghost btn-sm"
                  type="button"
                  :disabled="isPreparingPlayback || !canGoNext"
                  aria-label="Next track"
                  title="Next"
                  @click="playNextTrack"
                >
                  <SkipForward :size="17" aria-hidden="true" />
                </button>
              </div>
            </div>

            <div class="flex min-w-0 items-center gap-2">
              <span class="hidden w-9 text-right text-xs tabular-nums text-muted sm:block">
                {{ formatPlaybackTime(playbackPosition) }}
              </span>
              <input
                class="range range-sm min-h-6 min-w-0 flex-1"
                type="range"
                min="0"
                :max="Math.max(playbackDuration, 1)"
                step="0.1"
                :value="playbackPosition"
                :disabled="!audioUrl || playbackDuration <= 0"
                aria-label="Seek playback"
                :aria-valuetext="`${formatPlaybackTime(playbackPosition)} of ${formatPlaybackTime(playbackDuration)}`"
                @input="seekPlayback"
              />
              <span class="hidden w-9 text-xs tabular-nums text-muted sm:block">
                {{ formatPlaybackTime(playbackDuration) }}
              </span>
            </div>
          </div>

          <div
            class="col-start-2 row-start-1 flex shrink-0 items-center justify-end gap-1 md:col-start-3 lg:col-start-3 lg:row-start-1"
            data-testid="playback-actions"
          >
            <div
              class="tooltip tooltip-top"
              :data-tip="desktopLyricsPreferences.enabled ? 'Hide desktop lyrics' : 'Show desktop lyrics'"
            >
              <button
                class="btn btn-square btn-ghost btn-sm"
                :class="{ 'btn-active': desktopLyricsPreferences.enabled }"
                type="button"
                :aria-label="desktopLyricsPreferences.enabled ? 'Hide desktop lyrics' : 'Show desktop lyrics'"
                :aria-pressed="desktopLyricsPreferences.enabled"
                :title="desktopLyricsPreferences.enabled ? 'Hide desktop lyrics' : 'Show desktop lyrics'"
                data-testid="desktop-lyrics-toggle"
                @click="toggleDesktopLyrics"
              >
                <Captions :size="16" aria-hidden="true" />
              </button>
            </div>

            <div class="tooltip tooltip-top" data-tip="Add to My Favorite Music">
              <button
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                :disabled="!activeOnlineTrackSupportsLibraryActions || activeOnlineFavoritePending"
                :aria-label="activeOnlineTrack ? `Add ${activeOnlineTrack.title} to My Favorite Music` : 'Favorite current online track'"
                :aria-pressed="activeOnlineTrackIsFavorite"
                title="Add to My Favorite Music"
                @click="favoriteActiveOnlineTrack"
              >
                <RefreshCw v-if="activeOnlineFavoritePending" class="animate-spin" :size="16" aria-hidden="true" />
                <Heart
                  v-else
                  :class="{ 'text-error': activeOnlineTrackIsFavorite }"
                  :fill="activeOnlineTrackIsFavorite ? 'currentColor' : 'none'"
                  :size="16"
                  aria-hidden="true"
                />
              </button>
            </div>

            <div class="group relative">
              <button
                class="btn btn-square btn-ghost btn-sm"
                type="button"
                aria-label="Volume"
                :aria-valuetext="`${volumePercent}%`"
                title="Volume"
              >
                <Volume2 :size="17" aria-hidden="true" />
              </button>
              <div
                class="absolute bottom-full left-1/2 z-50 hidden -translate-x-1/2 flex-col pb-2 group-hover:flex group-focus-within:flex"
                data-testid="volume-popover"
              >
                <div class="flex h-36 w-12 flex-col items-center gap-2 rounded border border-base-300 bg-base-100 px-2 py-3 shadow-lg">
                  <output class="text-xs tabular-nums text-muted">{{ volumePercent }}</output>
                  <input
                    v-model.number="volume"
                    class="range range-sm range-vertical min-h-0 min-w-6 flex-1"
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    aria-label="Volume"
                    :aria-valuetext="`${volumePercent}%`"
                    @input="updateVolume"
                  />
                </div>
              </div>
            </div>

            <details
              ref="playbackOptionsMenu"
              class="dropdown dropdown-end dropdown-top"
              data-testid="playback-options-menu"
            >
              <summary
                class="btn btn-square btn-ghost btn-sm"
                aria-label="More options"
                title="More options"
              >
                <Ellipsis :size="18" aria-hidden="true" />
              </summary>
              <ul
                class="dropdown-content menu menu-sm z-50 mb-2 w-60 rounded border border-base-300 bg-base-100 p-2 shadow-lg"
                role="menu"
                aria-label="More playback options"
              >
                <li>
                  <button
                    type="button"
                    :disabled="!activeOnlineTrack || activeOnlineDownloadPending"
                    :aria-label="activeOnlineTrack ? `Download ${activeOnlineTrack.title}` : 'Download current online track'"
                    @click="downloadActiveOnlineTrack"
                  >
                    <RefreshCw v-if="activeOnlineDownloadPending" class="animate-spin" :size="16" aria-hidden="true" />
                    <Download v-else :size="16" aria-hidden="true" />
                    <span>Download</span>
                  </button>
                </li>
                <li>
                  <button
                    type="button"
                    :disabled="!activeOnlineTrackSupportsLibraryActions || activeOnlinePlaylistPending"
                    :aria-label="activeOnlineTrack ? `Add ${activeOnlineTrack.title} to a Playlist` : 'Add current online track to a Playlist'"
                    @click="addActiveOnlineTrackToPlaylist"
                  >
                    <RefreshCw v-if="activeOnlinePlaylistPending" class="animate-spin" :size="16" aria-hidden="true" />
                    <ListPlus v-else :size="16" aria-hidden="true" />
                    <span>Add to Playlist</span>
                  </button>
                </li>
                <li>
                  <details>
                    <summary>
                      <AudioLines :size="16" aria-hidden="true" />
                      <span>Change Audio Source</span>
                    </summary>
                    <ul>
                      <li
                        v-if="automaticAudioSourceSelection && availableAudioSources.length"
                        class="menu-disabled"
                      >
                        <span>
                          Automatic · {{ availableAudioSources.find((source) => source.value === activeOnlineAudioSourceId)?.label || "Selecting" }}
                        </span>
                      </li>
                      <li v-if="!availableAudioSources.length" class="menu-disabled">
                        <span>No Audio Sources available</span>
                      </li>
                      <li
                        v-for="source in automaticAudioSourceSelection ? [] : availableAudioSources"
                        :key="source.value"
                      >
                        <button
                          type="button"
                          role="menuitemradio"
                          :aria-checked="currentPlaybackAudioSourceId === source.value"
                          :data-audio-source-id="source.value"
                          @click="changePlaybackAudioSource(source.value)"
                        >
                          <Check
                            v-if="currentPlaybackAudioSourceId === source.value"
                            class="shrink-0"
                            :size="16"
                            aria-hidden="true"
                          />
                          <span v-else class="size-4 shrink-0" aria-hidden="true"></span>
                          <span>{{ source.label }}</span>
                        </button>
                      </li>
                    </ul>
                  </details>
                </li>
                <li>
                  <details>
                    <summary>
                      <Gauge :size="16" aria-hidden="true" />
                      <span>Change Quality</span>
                    </summary>
                    <ul>
                      <li v-for="quality in STREAM_QUALITY_OPTIONS" :key="quality.value">
                        <button
                          type="button"
                          role="menuitemradio"
                          :aria-checked="currentPlaybackQuality === quality.value"
                          :data-stream-quality="quality.value"
                          @click="changePlaybackQuality(quality.value)"
                        >
                          <Check
                            v-if="currentPlaybackQuality === quality.value"
                            class="shrink-0"
                            :size="16"
                            aria-hidden="true"
                          />
                          <span v-else class="size-4 shrink-0" aria-hidden="true"></span>
                          <span>{{ quality.label }}</span>
                        </button>
                      </li>
                    </ul>
                  </details>
                </li>
              </ul>
            </details>
          </div>

          <audio
            v-if="audioUrl"
            ref="audioElement"
            class="hidden"
    :src="audioUrl"
            @durationchange="onAudioLoadedMetadata"
            @ended="onAudioEnded"
            @loadedmetadata="onAudioLoadedMetadata"
            @pause="onAudioPause"
            @play="onAudioPlay"
            @playing="onAudioPlaying"
            @timeupdate="onAudioTimeUpdate"
            @waiting="onAudioWaiting"
            @error="onAudioError"
          ></audio>
        </div>
      </footer>
    </div>

    <div class="drawer-side z-40">
      <label for="app-sidebar" aria-label="Close navigation" class="drawer-overlay"></label>
      <aside class="flex min-h-full w-60 flex-col border-r border-base-300 bg-base-100">
        <div class="flex min-h-16 items-center gap-3 border-b border-base-300 px-4">
          <img class="size-9 shrink-0" :src="fikaLogoUrl" alt="" />
          <div class="min-w-0">
            <div class="truncate text-base font-semibold leading-tight">Fika Music</div>
            <div class="truncate text-xs text-muted">Local-first library</div>
          </div>
        </div>

        <nav class="flex min-h-0 flex-1 flex-col p-3" aria-label="Primary navigation">
          <ul
            class="menu min-h-0 w-full flex-1 flex-nowrap gap-1 overflow-y-auto p-0"
            :class="layoutDensity === 'compact' ? 'menu-sm' : 'menu-md'"
          >
            <template v-for="section in mainSections" :key="section.id">
              <template v-if="section.id === 'local'">
                <li class="group/local">
                  <div
                    class="flex min-h-10 items-center rounded px-1"
                    :class="[
                      activeSection === 'local'
                        ? 'bg-neutral text-neutral-content'
                        : activeSection === 'collection'
                          ? 'bg-base-200'
                          : 'hover:bg-base-200',
                    ]"
                    @contextmenu="openLocalMusicContextMenu"
                  >
                    <button
                      class="flex min-w-0 flex-1 items-center gap-2 self-stretch px-2 text-left"
                      type="button"
                      data-section-id="local"
                      :aria-current="activeSection === 'local' ? 'page' : undefined"
                      @click="selectSection('local')"
                    >
                      <Library :size="18" aria-hidden="true" />
                      <span class="min-w-0 flex-1 truncate">Local Music</span>
                    </button>
                    <button
                      class="btn btn-square btn-ghost btn-xs size-7 shrink-0"
                      :class="activeSection === 'local' ? 'text-neutral-content' : undefined"
                      type="button"
                      :aria-label="collectionsExpanded ? 'Collapse Collections' : 'Expand Collections'"
                      :aria-expanded="collectionsExpanded"
                      :title="collectionsExpanded ? 'Collapse Collections' : 'Expand Collections'"
                      @click.stop="collectionsExpanded = !collectionsExpanded"
                    >
                      <ChevronRight
                        class="transition-transform"
                        :class="{ 'rotate-90': collectionsExpanded }"
                        :size="15"
                        aria-hidden="true"
                      />
                    </button>
                    <button
                      class="btn btn-square btn-ghost btn-xs size-7 shrink-0"
                      :class="activeSection === 'local' ? 'text-neutral-content' : undefined"
                      type="button"
                      aria-label="New Collection"
                      title="New Collection"
                      @click.stop="openCreateCollection()"
                    >
                      <Plus :size="15" aria-hidden="true" />
                    </button>
                  </div>
                </li>

                <template v-if="collectionsExpanded">
            <li v-for="collection in musicCollections" :key="collection.id">
                    <button
                      type="button"
                      class="group/collection min-h-9 gap-2 pl-9 pr-2"
                      :class="[
                        { 'menu-active': activeSection === 'collection' && activeCollectionId === collection.id },
                        collectionDropTargetId === collection.id ? 'outline outline-2 -outline-offset-2 outline-primary' : '',
                      ]"
                      :aria-current="activeSection === 'collection' && activeCollectionId === collection.id ? 'page' : undefined"
                      :data-collection-id="collection.id"
                      :title="`${collection.name} (${collection.itemCount} tracks)`"
                      @click="selectCollection(collection.id)"
                      @dblclick="playCollectionFromSidebar(collection)"
                      @contextmenu="openCollectionContextMenu($event, collection.id)"
                      @dragover="handleCollectionDragOver($event, collection.id)"
                      @dragleave="handleCollectionDragLeave($event, collection.id)"
                      @drop="handleCollectionDrop($event, collection.id)"
                    >
                      <ListMusic class="shrink-0" :size="16" aria-hidden="true" />
                      <span class="min-w-0 flex-1 truncate text-left">{{ collection.name }}</span>
                      <span class="shrink-0 text-xs tabular-nums opacity-60">
                        {{ collection.itemCount }}
                      </span>
                    </button>
                  </li>
                </template>
              </template>

              <li v-else>
                <button
                  type="button"
                  :data-section-id="section.id"
                  :class="{ 'menu-active': activeSection === section.id }"
                  :aria-current="activeSection === section.id ? 'page' : undefined"
                  @click="selectSection(section.id)"
                >
                  <component :is="section.icon" :size="18" aria-hidden="true" />
                  <span>{{ section.label }}</span>
                </button>
              </li>
            </template>

            <li v-if="enabledPlugins.length" class="menu-title mt-3">
              <span>Enabled plugins</span>
            </li>
            <li v-for="plugin in enabledPlugins" :key="plugin.id">
              <button
                type="button"
                :class="{
                  'menu-active': activeSection === 'plugin' && activePluginId === plugin.id,
                }"
                :aria-current="
                  activeSection === 'plugin' && activePluginId === plugin.id ? 'page' : undefined
                "
                :data-plugin-id="plugin.id"
                :title="`${plugin.name} (${plugin.id})`"
                @click="selectPlugin(plugin.id)"
              >
                <Plug :size="18" aria-hidden="true" />
                <span class="min-w-0 truncate">{{ plugin.name }}</span>
              </button>
            </li>
          </ul>

          <ul
            class="menu w-full shrink-0 gap-1 p-0 pt-4"
            :class="layoutDensity === 'compact' ? 'menu-sm' : 'menu-md'"
          >
            <li>
              <button
                type="button"
                data-section-id="settings"
                :class="{ 'menu-active': activeSection === settingsSection.id }"
                :aria-current="activeSection === settingsSection.id ? 'page' : undefined"
                @click="selectSection(settingsSection.id)"
              >
                <Settings :size="18" aria-hidden="true" />
                <span>{{ settingsSection.label }}</span>
              </button>
            </li>
          </ul>
        </nav>

        <div class="border-t border-base-300 px-4 py-3">
          <div class="truncate text-xs text-muted" :title="selectedFolder || undefined">
            {{ selectedFolder || "No music folder" }}
          </div>
          <div class="mt-1 text-sm font-medium tabular-nums">
            {{ libraryTrackCount.toLocaleString() }} track{{ libraryTrackCount === 1 ? "" : "s" }} indexed
            <span v-if="filteredLibraryTrackCount !== libraryTrackCount" class="text-muted">
              · {{ filteredLibraryTrackCount.toLocaleString() }} shown
            </span>
          </div>
        </div>
      </aside>
    </div>

    <Teleport to="body">
      <div
        v-if="localMusicContextMenu || collectionContextMenu"
        class="fixed inset-0 z-50"
        aria-hidden="true"
        @pointerdown="closeSidebarContextMenus"
        @contextmenu.prevent="closeSidebarContextMenus"
      ></div>
      <ul
        v-if="localMusicContextMenu"
        class="menu menu-sm fixed z-[60] w-56 rounded border border-base-300 bg-base-100 p-2 shadow-xl"
        :style="{ left: `${localMusicContextMenu.x}px`, top: `${localMusicContextMenu.y}px` }"
        data-sidebar-context-menu
        aria-label="Local Music actions"
      >
        <li>
          <button type="button" @click="openCreateCollection()">
            <FolderPlus :size="16" aria-hidden="true" />
            New Collection
          </button>
        </li>
        <li>
          <button type="button" @click="collectionsExpanded = !collectionsExpanded; closeSidebarContextMenus()">
            <ChevronRight
              :class="{ 'rotate-90': collectionsExpanded }"
              :size="16"
              aria-hidden="true"
            />
            {{ collectionsExpanded ? "Collapse Collections" : "Expand Collections" }}
          </button>
        </li>
      </ul>

      <ul
        v-if="collectionContextMenu && collectionForContextMenu()"
        class="menu menu-sm fixed z-[60] w-56 rounded border border-base-300 bg-base-100 p-2 shadow-xl"
        :style="{ left: `${collectionContextMenu.x}px`, top: `${collectionContextMenu.y}px` }"
        data-sidebar-context-menu
        aria-label="Collection actions"
      >
        <li>
          <button type="button" @click="renameContextCollection">
            <Pencil :size="16" aria-hidden="true" />
            Rename Collection
          </button>
        </li>
        <li>
          <button class="text-error" type="button" @click="deleteContextCollection">
            <Trash2 :size="16" aria-hidden="true" />
            Delete Collection
          </button>
        </li>
      </ul>

      <dialog
        v-if="collectionNameDialog"
        open
        tabindex="0"
        class="modal"
        aria-labelledby="collection-name-dialog-title"
        @cancel.prevent="closeCollectionDialogs"
      >
        <form class="modal-box max-w-md rounded" @submit.prevent="submitCollectionName">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">
              <h2 id="collection-name-dialog-title" class="text-base font-semibold">
                {{ collectionNameDialog.intent === "rename" ? "Rename Collection" : "New Collection" }}
              </h2>
              <p
                v-if="collectionNameDialog.intent === 'create' && collectionNameDialog.seed.kind !== 'empty'"
                class="mt-1 text-sm text-muted"
              >
                {{ collectionSeedCount(collectionNameDialog.seed) === null
                  ? "Add the selected tracks"
                  : `${collectionSeedCount(collectionNameDialog.seed)} selected track${collectionSeedCount(collectionNameDialog.seed) === 1 ? '' : 's'}` }}
              </p>
            </div>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              :disabled="collectionActionBusy"
              aria-label="Close Collection dialog"
              @click="closeCollectionDialogs"
            >
              <X :size="17" aria-hidden="true" />
            </button>
          </div>

          <div v-if="collectionActionError" class="alert alert-error mt-4 py-2 text-sm" role="alert">
            <AlertCircle :size="16" aria-hidden="true" />
            <span>{{ collectionActionError }}</span>
          </div>

          <label class="mt-5 block">
            <span class="mb-1.5 block text-sm font-medium">Name</span>
            <input
              ref="collectionNameInput"
              v-model="collectionName"
              class="input input-sm w-full"
              type="text"
              maxlength="80"
              autocomplete="off"
              placeholder="Collection name"
              aria-label="Collection name"
            />
          </label>

          <div class="modal-action">
            <button
              class="btn btn-ghost btn-sm"
              type="button"
              :disabled="collectionActionBusy"
              @click="closeCollectionDialogs"
            >
              Cancel
            </button>
            <button
              class="btn btn-neutral btn-sm"
              type="submit"
              :disabled="!collectionName.trim() || collectionActionBusy"
            >
              <RefreshCw
                v-if="collectionActionBusy"
                class="animate-spin"
                :size="16"
                aria-hidden="true"
              />
              <FolderPlus v-else :size="16" aria-hidden="true" />
              {{ collectionNameDialog.intent === "rename" ? "Save" : "Create" }}
            </button>
          </div>
        </form>
        <form method="dialog" class="modal-backdrop" @submit.prevent="closeCollectionDialogs">
          <button type="submit" :disabled="collectionActionBusy">Close</button>
        </form>
      </dialog>

      <dialog
        v-if="collectionPickerSeed && collectionPickerSeed.kind !== 'empty'"
        open
        tabindex="0"
        class="modal"
        aria-labelledby="collection-picker-title"
        @cancel.prevent="closeCollectionDialogs"
      >
        <form class="modal-box max-w-lg rounded" @submit.prevent="confirmCollectionAdd">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">
              <h2 id="collection-picker-title" class="text-base font-semibold">Add to Collection</h2>
              <p class="mt-1 text-sm text-muted">
                {{ collectionSeedCount(collectionPickerSeed) === null
                  ? "Selected Local Music tracks"
                  : `${collectionSeedCount(collectionPickerSeed)} selected track${collectionSeedCount(collectionPickerSeed) === 1 ? '' : 's'}` }}
              </p>
            </div>
            <button
              class="btn btn-square btn-ghost btn-sm"
              type="button"
              :disabled="collectionActionBusy"
              aria-label="Close Collection picker"
              @click="closeCollectionDialogs"
            >
              <X :size="17" aria-hidden="true" />
            </button>
          </div>

          <div v-if="collectionActionError" class="alert alert-error mt-4 py-2 text-sm" role="alert">
            <AlertCircle :size="16" aria-hidden="true" />
            <span>{{ collectionActionError }}</span>
          </div>

          <ul class="menu mt-4 max-h-72 w-full overflow-y-auto rounded border border-base-300 p-1">
          <li v-for="collection in collectionPickerOptions()" :key="collection.id">
              <button
                type="button"
                :class="{ 'menu-active': selectedCollectionId === collection.id }"
                :aria-pressed="selectedCollectionId === collection.id"
                @click="selectedCollectionId = collection.id"
              >
                <ListMusic :size="17" aria-hidden="true" />
                <span class="min-w-0 flex-1 truncate text-left">{{ collection.name }}</span>
                <span class="badge badge-ghost badge-sm shrink-0">{{ collection.itemCount }}</span>
              </button>
            </li>
          </ul>

          <div class="modal-action justify-between">
            <button
              class="btn btn-ghost btn-sm"
              type="button"
              :disabled="collectionActionBusy"
              @click="createCollectionFromPicker"
            >
              <Plus :size="16" aria-hidden="true" />
              New Collection
            </button>
            <div class="flex gap-2">
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :disabled="collectionActionBusy"
                @click="closeCollectionDialogs"
              >
                Cancel
              </button>
              <button
                class="btn btn-neutral btn-sm"
                type="submit"
                :disabled="!selectedCollectionId || collectionActionBusy"
              >
                <RefreshCw
                  v-if="collectionActionBusy"
                  class="animate-spin"
                  :size="16"
                  aria-hidden="true"
                />
                <ListPlus v-else :size="16" aria-hidden="true" />
                Add
              </button>
            </div>
          </div>
        </form>
        <form method="dialog" class="modal-backdrop" @submit.prevent="closeCollectionDialogs">
          <button type="submit" :disabled="collectionActionBusy">Close</button>
        </form>
      </dialog>

      <dialog
        v-if="collectionDeleteTarget"
        open
        tabindex="0"
        class="modal"
        aria-labelledby="delete-collection-title"
        @cancel.prevent="closeCollectionDialogs"
      >
        <form class="modal-box max-w-md rounded" @submit.prevent="confirmCollectionDelete">
          <h2 id="delete-collection-title" class="text-base font-semibold">Delete Collection?</h2>
          <p class="mt-3 text-sm leading-6 text-muted">
            {{ collectionDeleteTarget.name }} and its {{ collectionDeleteTarget.itemCount }} saved track{{ collectionDeleteTarget.itemCount === 1 ? "" : "s" }} will be removed. Music files are not deleted.
          </p>
          <div v-if="collectionActionError" class="alert alert-error mt-4 py-2 text-sm" role="alert">
            <AlertCircle :size="16" aria-hidden="true" />
            <span>{{ collectionActionError }}</span>
          </div>
          <div class="modal-action">
            <button
              class="btn btn-ghost btn-sm"
              type="button"
              :disabled="collectionActionBusy"
              @click="closeCollectionDialogs"
            >
              Cancel
            </button>
            <button class="btn btn-error btn-sm" type="submit" :disabled="collectionActionBusy">
              <RefreshCw
                v-if="collectionActionBusy"
                class="animate-spin"
                :size="16"
                aria-hidden="true"
              />
              <Trash2 v-else :size="16" aria-hidden="true" />
              Delete
            </button>
          </div>
        </form>
        <form method="dialog" class="modal-backdrop" @submit.prevent="closeCollectionDialogs">
          <button type="submit" :disabled="collectionActionBusy">Close</button>
        </form>
      </dialog>

      <div v-if="collectionNotice" class="toast toast-end toast-bottom z-[70] mb-20" role="status">
        <div class="alert alert-success py-2 text-sm shadow-lg">
          <Check :size="16" aria-hidden="true" />
          <span>{{ collectionNotice }}</span>
        </div>
      </div>
    </Teleport>
  </div>
</template>
