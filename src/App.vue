<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertCircle,
  AudioLines,
  FolderOpen,
  Gauge,
  Headphones,
  Library,
  ListOrdered,
  Menu,
  Music2,
  Pause,
  Palette,
  Play,
  Plug,
  RefreshCw,
  Repeat2,
  RotateCcw,
  Settings,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
  X,
} from "@lucide/vue";
import PluginManager from "./components/PluginManager.vue";
import NeteaseSource from "./components/NeteaseSource.vue";
import NowPlayingPanel from "./components/NowPlayingPanel.vue";
import type { NeteasePlayback } from "./lib/netease-api";
import { TAURI_COMMANDS } from "./generated/bindings";
import type {
  LocalTrack,
  LocalTrackPlaybackDetails,
  MediaSource,
  RemoteCommandError,
  RemoteMediaSource,
  RemoteSearchResults,
  ResolvedLyrics,
  ScanProgressEvent,
  ScanStatus,
  SourceSearchResult,
  TrackLyricsQuery,
} from "./generated/bindings";
import {
  DEFAULT_UI_PREFERENCES,
  loadUiPreferences,
  saveUiPreferences,
  type PlaybackMode,
  type ThemePreference,
} from "./lib/ui-preferences";

type PlaybackSource = {
  filePath?: string;
  url?: string;
  mimeType: string;
};

const emptyScanStatus: ScanStatus = {
  isRunning: false,
  folderPath: null,
  discoveredFiles: 0,
  scannedFiles: 0,
  indexedTracks: 0,
  skippedFiles: 0,
  errorCount: 0,
  lastError: null,
  startedAt: null,
  finishedAt: null,
};

const mainSections = [
  {
    id: "local",
    label: "Local Music",
    description: "Browse and index music stored on this device",
    icon: Library,
  },
  {
    id: "sources",
    label: "Audio Sources",
    description: "Browse NetEase recommendations, Playlists, and other providers",
    icon: AudioLines,
  },
  {
    id: "plugins",
    label: "Plugins",
    description: "Review installed packages and their permissions",
    icon: Plug,
  },
] as const;

const settingsSection = {
  id: "settings",
  label: "Settings",
  description: "Manage appearance and playback defaults",
  icon: Settings,
} as const;

const sections = [...mainSections, settingsSection];
type AppSection = (typeof sections)[number]["id"];

const savedUiPreferences = loadUiPreferences();
const tracks = ref<LocalTrack[]>([]);
const scanStatus = ref<ScanStatus>({ ...emptyScanStatus });
const selectedFolder = ref<string | null>(null);
const activeSection = ref<AppSection>("local");
const sidebarOpen = ref(false);
const activeTrack = ref<LocalTrack | null>(null);
const activeRemoteTitle = ref<string | null>(null);
const activeRemoteProvider = ref<string | null>(null);
const activeSource = ref<PlaybackSource | null>(null);
const audioUrl = ref<string | null>(null);
const isPlaying = ref(false);
const playbackPosition = ref(0);
const playbackDuration = ref(0);
const volume = ref(savedUiPreferences.volume);
const playbackMode = ref<PlaybackMode>(savedUiPreferences.playbackMode);
const themePreference = ref(savedUiPreferences.theme);
const layoutDensity = ref(savedUiPreferences.density);
const nowPlayingCoverUrl = ref<string | null>(null);
const activeLyrics = ref<ResolvedLyrics | null>(null);
const activeRemoteLyricsQuery = ref<TrackLyricsQuery | null>(null);
const isLoadingLyrics = ref(false);
const lyricsError = ref<string | null>(null);
const isLoadingTracks = ref(false);
const isChoosingFolder = ref(false);
const isStartingScan = ref(false);
const isPreparingPlayback = ref(false);
const isResolvingRemote = ref(false);
const appError = ref<string | null>(null);
const scanMessage = ref<string | null>(null);
const audioElement = ref<HTMLAudioElement | null>(null);
const remoteFamily = ref("nianxin");
const remoteSource = ref("wy");
const remoteQuality = ref(savedUiPreferences.streamQuality);
const remoteTrackId = ref("");
const remoteDiagnostics = ref<string[]>([]);
const remoteSearchKeyword = ref("");
const remoteSearchResults = ref<SourceSearchResult[]>([]);
const remoteSearchTotal = ref<number | null>(null);
const isSearchingRemote = ref(false);
const activeRemoteRequestId = ref<string | null>(null);
const isCancellingRemoteRequest = ref(false);

let unlistenScanProgress: UnlistenFn | null = null;
let playbackDetailsGeneration = 0;

const hasTracks = computed(() => tracks.value.length > 0);
const visibleTracks = computed(() => tracks.value.slice(0, 200));
const currentSection = computed(
  () => sections.find((section) => section.id === activeSection.value) ?? mainSections[0],
);
const canScan = computed(
  () => Boolean(selectedFolder.value) && !scanStatus.value.isRunning && !isStartingScan.value,
);
const scanPercent = computed(() => {
  const total = scanStatus.value.discoveredFiles;
  if (total <= 0) {
    return scanStatus.value.isRunning ? 1 : 0;
  }

  return Math.round((scanStatus.value.scannedFiles / total) * 100);
});
const libraryDuration = computed(() => {
  const totalSeconds = tracks.value.reduce(
    (total, track) => total + (track.durationSeconds ?? 0),
    0,
  );

  return formatLongDuration(totalSeconds);
});
const nowPlayingTitle = computed(() => activeTrack.value?.title || activeRemoteTitle.value || "Nothing playing");
const nowPlayingSubtitle = computed(() => {
  if (activeTrack.value) {
    return trackSubtitle(activeTrack.value);
  }

  return activeRemoteTitle.value
    ? activeRemoteProvider.value || "Remote Source Provider"
    : "Select a local or remote track";
});
const hasActiveRemoteRequest = computed(() => activeRemoteRequestId.value !== null);
const volumePercent = computed(() => Math.round(volume.value * 100));
const activeTrackIndex = computed(() =>
  activeTrack.value ? tracks.value.findIndex((track) => track.id === activeTrack.value?.id) : -1,
);
const canGoPrevious = computed(() => {
  if (activeTrackIndex.value < 0 || tracks.value.length === 0) {
    return false;
  }
  return playbackMode.value !== "sequential" || activeTrackIndex.value > 0;
});
const canGoNext = computed(() => {
  if (activeTrackIndex.value < 0 || tracks.value.length === 0) {
    return false;
  }
  return playbackMode.value !== "sequential" || activeTrackIndex.value < tracks.value.length - 1;
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

watch(themePreference, applyTheme, { immediate: true });
watch(volume, updateVolume);
watch(audioUrl, () => {
  playbackPosition.value = 0;
  playbackDuration.value = 0;
});
watch([themePreference, layoutDensity, remoteQuality, volume, playbackMode], () => {
  saveUiPreferences({
    theme: themePreference.value,
    density: layoutDensity.value,
    streamQuality: remoteQuality.value,
    volume: volume.value,
    playbackMode: playbackMode.value,
  });
});

onMounted(async () => {
  await Promise.all([loadTracks(), loadScanStatus(), bindScanProgress()]);
});

onBeforeUnmount(() => {
  unlistenScanProgress?.();
  playbackDetailsGeneration += 1;
  void cancelActiveRemoteRequest();
});

function beginRemoteRequest() {
  const requestId = crypto.randomUUID();
  activeRemoteRequestId.value = requestId;
  return requestId;
}

function selectSection(section: AppSection) {
  activeSection.value = section;
  sidebarOpen.value = false;
}

function applyTheme(theme: ThemePreference) {
  if (typeof document === "undefined") {
    return;
  }

  if (theme === "system") {
    document.documentElement.removeAttribute("data-theme");
    return;
  }

  document.documentElement.dataset.theme = theme;
}

function resetUiPreferences() {
  themePreference.value = DEFAULT_UI_PREFERENCES.theme;
  layoutDensity.value = DEFAULT_UI_PREFERENCES.density;
  remoteQuality.value = DEFAULT_UI_PREFERENCES.streamQuality;
  volume.value = DEFAULT_UI_PREFERENCES.volume;
  playbackMode.value = DEFAULT_UI_PREFERENCES.playbackMode;
}

async function cancelActiveRemoteRequest() {
  const requestId = activeRemoteRequestId.value;
  if (!requestId || isCancellingRemoteRequest.value) {
    return;
  }

  isCancellingRemoteRequest.value = true;
  try {
    await invoke(TAURI_COMMANDS.cancelSourceRequest, { requestId });
  } catch (error) {
    appError.value = normalizeError(error);
  } finally {
    isCancellingRemoteRequest.value = false;
  }
}

async function bindScanProgress() {
  unlistenScanProgress = await listen<ScanProgressEvent>("library:scan-progress", async (event) => {
    scanStatus.value = event.payload.status;
    scanMessage.value = event.payload.message;

    if (!event.payload.status.isRunning) {
      await loadTracks();
    }
  });
}

async function loadScanStatus() {
  scanStatus.value = await invoke<ScanStatus>(TAURI_COMMANDS.getScanStatus);
  selectedFolder.value = scanStatus.value.folderPath;
}

async function loadTracks() {
  isLoadingTracks.value = true;
  appError.value = null;

  try {
    tracks.value = await invoke<LocalTrack[]>(TAURI_COMMANDS.listLocalTracks);
  } catch (error) {
    appError.value = normalizeError(error);
  } finally {
    isLoadingTracks.value = false;
  }
}

async function chooseFolder() {
  isChoosingFolder.value = true;
  appError.value = null;

  try {
    const folder = await invoke<string | null>(TAURI_COMMANDS.selectMusicFolder);
    if (folder) {
      selectedFolder.value = folder;
    }
  } catch (error) {
    appError.value = normalizeError(error);
  } finally {
    isChoosingFolder.value = false;
  }
}

async function startScan() {
  if (!selectedFolder.value) {
    return;
  }

  isStartingScan.value = true;
  appError.value = null;
  scanMessage.value = null;

  try {
    scanStatus.value = await invoke<ScanStatus>(TAURI_COMMANDS.startLibraryScan, {
      folderPath: selectedFolder.value,
    });
  } catch (error) {
    appError.value = normalizeError(error);
  } finally {
    isStartingScan.value = false;
  }
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

function lyricsQueryForRemoteTrack(track: SourceSearchResult): TrackLyricsQuery {
  return {
    title: track.title,
    artist: track.artist || null,
    album: track.album,
    durationSeconds: track.durationSeconds,
    source: track.source,
    trackId: track.id,
  };
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

async function playTrack(track: LocalTrack) {
  isPreparingPlayback.value = true;
  appError.value = null;

  try {
    const source = await invoke<MediaSource>(TAURI_COMMANDS.localTrackMediaSource, {
      trackId: track.id,
    });

    activeTrack.value = track;
    activeRemoteTitle.value = null;
    activeRemoteProvider.value = null;
    activeSource.value = source;
    audioUrl.value = convertFileSrc(source.filePath);
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

async function playRemoteTrack() {
  if (hasActiveRemoteRequest.value) {
    return;
  }
  if (!remoteTrackId.value.trim()) {
    appError.value = "Enter a remote track id first.";
    return;
  }

  isResolvingRemote.value = true;
  isPreparingPlayback.value = true;
  appError.value = null;
  remoteDiagnostics.value = [];
  const requestId = beginRemoteRequest();

  try {
    const source = await invoke<RemoteMediaSource>(
      TAURI_COMMANDS.resolveImportedLxTemplateMusicUrl,
      {
      family: remoteFamily.value,
      source: remoteSource.value,
      trackId: remoteTrackId.value.trim(),
      quality: remoteQuality.value,
      requestId,
      },
    );

    activeTrack.value = null;
    activeRemoteTitle.value = `${remoteFamily.value}:${remoteSource.value}:${remoteTrackId.value.trim()}`;
    activeRemoteProvider.value = "Remote LX template source";
    activeSource.value = { url: source.url, mimeType: source.mimeType };
    audioUrl.value = source.url;
    remoteDiagnostics.value = source.diagnostics.map((diagnostic) => diagnostic.message);
    resetPlaybackDetails(null);

    await nextTick();
    if (audioElement.value) {
      audioElement.value.volume = volume.value;
      await audioElement.value.play();
      isPlaying.value = true;
    }
  } catch (error) {
    appError.value = normalizeRemoteError(error);
    isPlaying.value = false;
  } finally {
    isPreparingPlayback.value = false;
    isResolvingRemote.value = false;
    if (activeRemoteRequestId.value === requestId) {
      activeRemoteRequestId.value = null;
    }
  }
}

async function searchRemoteMusic() {
  if (hasActiveRemoteRequest.value) {
    return;
  }
  if (!remoteSearchKeyword.value.trim()) {
    appError.value = "Enter a search keyword first.";
    return;
  }

  isSearchingRemote.value = true;
  appError.value = null;
  remoteDiagnostics.value = [];
  const requestId = beginRemoteRequest();

  try {
    const response = await invoke<RemoteSearchResults>(TAURI_COMMANDS.searchQishuiMusic, {
      keyword: remoteSearchKeyword.value.trim(),
      page: 1,
      pageSize: 20,
      requestId,
    });

    remoteSearchResults.value = response.list;
    remoteSearchTotal.value = response.total;
    remoteDiagnostics.value = response.diagnostics.map((diagnostic) => diagnostic.message);
  } catch (error) {
    appError.value = normalizeRemoteError(error);
  } finally {
    isSearchingRemote.value = false;
    if (activeRemoteRequestId.value === requestId) {
      activeRemoteRequestId.value = null;
    }
  }
}

async function playRemoteSearchResult(result: SourceSearchResult) {
  if (hasActiveRemoteRequest.value) {
    return;
  }
  isResolvingRemote.value = true;
  isPreparingPlayback.value = true;
  appError.value = null;
  remoteDiagnostics.value = [];
  const requestId = beginRemoteRequest();

  try {
    const source = await invoke<RemoteMediaSource>(TAURI_COMMANDS.resolveQishuiMusicUrl, {
      musicInfo: result.rawInfo,
      quality: remoteQuality.value,
      requestId,
    });

    activeTrack.value = null;
    activeRemoteTitle.value = `${result.title} - ${result.artist}`;
    activeRemoteProvider.value = "Qishui Source Provider";
    activeSource.value = { url: source.url, mimeType: source.mimeType };
    audioUrl.value = source.url;
    remoteDiagnostics.value = source.diagnostics.map((diagnostic) => diagnostic.message);
    void loadRemoteTrackLyrics(lyricsQueryForRemoteTrack(result), result.coverUrl);

    await nextTick();
    if (audioElement.value) {
      audioElement.value.volume = volume.value;
      await audioElement.value.play();
      isPlaying.value = true;
    }
  } catch (error) {
    appError.value = normalizeRemoteError(error);
    isPlaying.value = false;
  } finally {
    isPreparingPlayback.value = false;
    isResolvingRemote.value = false;
    if (activeRemoteRequestId.value === requestId) {
      activeRemoteRequestId.value = null;
    }
  }
}

async function playNeteasePlayback(playback: NeteasePlayback) {
  isPreparingPlayback.value = true;
  appError.value = null;
  remoteDiagnostics.value = playback.diagnostics.map((diagnostic) => diagnostic.message);

  try {
    activeTrack.value = null;
    activeRemoteTitle.value = `${playback.track.title} - ${playback.track.artist}`;
    activeRemoteProvider.value = "NetEase Cloud Music";
    activeSource.value = { url: playback.url, mimeType: playback.mimeType };
    audioUrl.value = playback.url;
    void loadRemoteTrackLyrics(
      lyricsQueryForRemoteTrack(playback.track),
      playback.track.coverUrl,
    );

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

async function togglePlayback() {
  if (!audioElement.value) {
    if (activeTrack.value) {
      await playTrack(activeTrack.value);
      return;
    }

    if (tracks.value[0]) {
      await playTrack(tracks.value[0]);
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
  const currentIndex = activeTrackIndex.value;
  if (currentIndex < 0 || tracks.value.length === 0) {
    return;
  }

  let previousIndex: number;
  if (playbackMode.value === "shuffle") {
    previousIndex = randomTrackIndex(currentIndex);
  } else if (currentIndex > 0) {
    previousIndex = currentIndex - 1;
  } else if (playbackMode.value === "repeat") {
    previousIndex = tracks.value.length - 1;
  } else {
    return;
  }

  const track = tracks.value[previousIndex];
  if (track) {
    await playTrack(track);
  }
}

async function playNextTrack() {
  const currentIndex = activeTrackIndex.value;
  if (currentIndex < 0 || tracks.value.length === 0) {
    return;
  }

  let nextIndex: number;
  if (playbackMode.value === "shuffle") {
    nextIndex = randomTrackIndex(currentIndex);
  } else if (currentIndex < tracks.value.length - 1) {
    nextIndex = currentIndex + 1;
  } else if (playbackMode.value === "repeat") {
    nextIndex = 0;
  } else {
    return;
  }

  const track = tracks.value[nextIndex];
  if (track) {
    await playTrack(track);
  }
}

function randomTrackIndex(currentIndex: number) {
  if (tracks.value.length <= 1) {
    return 0;
  }

  const candidate = Math.floor(Math.random() * (tracks.value.length - 1));
  return candidate >= currentIndex ? candidate + 1 : candidate;
}

function updateVolume() {
  if (audioElement.value) {
    audioElement.value.volume = volume.value;
  }
}

async function onAudioEnded() {
  isPlaying.value = false;
  playbackPosition.value = playbackDuration.value;
  await playNextTrack();
}

function onAudioPause() {
  isPlaying.value = false;
}

function onAudioPlay() {
  isPlaying.value = true;
}

function onAudioLoadedMetadata() {
  syncPlaybackTimeline();
}

function onAudioTimeUpdate() {
  syncPlaybackTimeline();
}

function syncPlaybackTimeline() {
  const audio = audioElement.value;
  if (!audio) {
    return;
  }

  playbackPosition.value = Number.isFinite(audio.currentTime) ? audio.currentTime : 0;
  playbackDuration.value = Number.isFinite(audio.duration) ? audio.duration : 0;
}

function seekPlayback(event: Event) {
  const audio = audioElement.value;
  if (!audio || playbackDuration.value <= 0) {
    return;
  }

  const nextPosition = Number((event.currentTarget as HTMLInputElement).value);
  audio.currentTime = nextPosition;
  playbackPosition.value = nextPosition;
}

function onAudioError() {
  isPlaying.value = false;
  appError.value = "Playback failed for the selected track.";
}

function formatPlaybackTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) {
    return "--:--";
  }

  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

function formatDuration(seconds: number | null) {
  if (!seconds) {
    return "--:--";
  }

  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

function formatLongDuration(seconds: number) {
  if (seconds <= 0) {
    return "0 min";
  }

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);

  if (hours === 0) {
    return `${minutes} min`;
  }

  return `${hours} hr ${minutes} min`;
}

function formatFileSize(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function trackSubtitle(track: LocalTrack) {
  return [track.artist, track.album].filter(Boolean).join(" - ") || track.fileName;
}

function normalizeError(error: unknown) {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return "Unexpected application error.";
}

function normalizeRemoteError(error: unknown) {
  const remoteError = parseRemoteCommandError(error);
  if (remoteError) {
    remoteDiagnostics.value = (remoteError.diagnostics ?? [])
      .map((diagnostic) => diagnostic.message)
      .filter(Boolean);
    return remoteError.message;
  }

  return normalizeError(error);
}

function parseRemoteCommandError(error: unknown): RemoteCommandError | null {
  let candidate: unknown = error;
  if (typeof candidate === "string") {
    try {
      candidate = JSON.parse(candidate);
    } catch {
      return null;
    }
  }

  if (!candidate || typeof candidate !== "object") {
    return null;
  }

  const value = candidate as Partial<RemoteCommandError>;
  return typeof value.message === "string" ? (value as RemoteCommandError) : null;
}
</script>

<template>
  <div
    class="drawer h-screen overflow-hidden bg-base-200 text-base-content md:drawer-open"
    :data-density="layoutDensity"
  >
    <input id="app-sidebar" v-model="sidebarOpen" type="checkbox" class="drawer-toggle" />

    <div class="drawer-content flex h-screen min-h-0 min-w-0 flex-col">
      <header class="navbar z-30 min-h-16 shrink-0 border-b border-base-300 bg-base-100 px-3 sm:px-4 lg:px-6">
        <div class="navbar-start min-w-0 flex-1 gap-2 sm:gap-3">
          <label
            for="app-sidebar"
            class="btn btn-square btn-ghost btn-sm drawer-button md:hidden"
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
            <p class="hidden truncate text-xs text-base-content/60 xl:block">
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
        <button
          class="btn btn-primary btn-sm"
          type="button"
          :disabled="!canScan"
          @click="startScan"
        >
          <RefreshCw :class="{ 'animate-spin': scanStatus.isRunning }" :size="16" aria-hidden="true" />
          Index
        </button>
        </div>

        <div v-else-if="activeSection === 'settings'" class="navbar-end w-auto">
          <button class="btn btn-ghost btn-sm" type="button" @click="resetUiPreferences">
            <RotateCcw :size="16" aria-hidden="true" />
            Reset
          </button>
        </div>
      </header>

      <main class="min-h-0 flex-1 overflow-y-auto">
        <section
          v-if="activeSection === 'local' || activeSection === 'sources'"
          class="mx-auto flex w-full max-w-7xl flex-col"
          :class="layoutDensity === 'compact' ? 'gap-3 px-3 py-3 lg:px-4' : 'gap-5 px-4 py-5 lg:px-6'"
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

          <div
            :class="
              activeSection === 'local'
                ? 'grid gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]'
                : 'block'
            "
          >
            <section
              v-if="activeSection === 'local'"
              class="flex min-h-[28rem] flex-col overflow-hidden rounded border border-base-300 bg-base-100"
            >
          <div class="flex flex-col gap-3 border-b border-base-300 p-4 md:flex-row md:items-center md:justify-between">
            <div>
              <h2 class="flex items-center gap-2 text-base font-semibold">
                <Library :size="18" aria-hidden="true" />
                Library
              </h2>
              <p class="mt-1 flex flex-wrap items-center gap-2 text-sm text-base-content/65">
                <span>{{ tracks.length }} tracks · {{ libraryDuration }}</span>
                <span v-if="tracks.length > visibleTracks.length" class="badge badge-sm">
                  Showing first {{ visibleTracks.length }}
                </span>
              </p>
            </div>
            <button class="btn btn-sm" type="button" :disabled="isLoadingTracks" @click="loadTracks">
              <RefreshCw :class="{ 'animate-spin': isLoadingTracks }" :size="16" aria-hidden="true" />
              Refresh
            </button>
          </div>

          <div v-if="!hasTracks && !isLoadingTracks" class="grid flex-1 place-items-center p-8 text-center">
            <div class="max-w-sm">
              <div class="mx-auto mb-4 flex size-14 items-center justify-center rounded border border-base-300 bg-base-200">
                <FolderOpen :size="26" aria-hidden="true" />
              </div>
              <h3 class="text-base font-semibold">No local tracks indexed</h3>
              <p class="mt-2 text-sm text-base-content/65">
                Choose a music folder and index MP3, FLAC, M4A, or AAC files.
              </p>
            </div>
          </div>

          <div v-else class="overflow-x-auto">
            <table
              class="table table-zebra"
              :class="layoutDensity === 'compact' ? 'table-xs' : 'table-sm'"
            >
              <thead>
                <tr>
                  <th class="w-12"></th>
                  <th>Track</th>
                  <th>Artist</th>
                  <th>Album</th>
                  <th class="text-right">Time</th>
                  <th class="text-right">Size</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="track in visibleTracks"
                  :key="track.id"
                  :class="{ 'bg-base-200': activeTrack?.id === track.id }"
                >
                  <td>
                    <button
                      class="btn btn-square btn-ghost btn-sm"
                      type="button"
                      :disabled="isPreparingPlayback"
                      :aria-label="
                        activeTrack?.id === track.id && isPlaying
                          ? `Pause ${track.title}`
                          : `Play ${track.title}`
                      "
                      @click="
                        activeTrack?.id === track.id ? togglePlayback() : playTrack(track)
                      "
                    >
                      <Pause v-if="activeTrack?.id === track.id && isPlaying" :size="16" aria-hidden="true" />
                      <Play v-else :size="16" aria-hidden="true" />
                    </button>
                  </td>
                  <td class="min-w-56">
                    <div class="font-medium">{{ track.title }}</div>
                    <div class="max-w-80 truncate text-xs text-base-content/60">{{ track.fileName }}</div>
                  </td>
                  <td>{{ track.artist || "Unknown artist" }}</td>
                  <td>{{ track.album || "Unknown album" }}</td>
                  <td class="text-right tabular-nums">{{ formatDuration(track.durationSeconds) }}</td>
                  <td class="text-right tabular-nums">{{ formatFileSize(track.fileSizeBytes) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

          <aside
            :class="
              activeSection === 'sources'
                ? 'mx-auto grid w-full max-w-7xl gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]'
                : 'flex flex-col gap-4'
            "
          >
          <NowPlayingPanel
            :class="activeSection === 'sources' ? 'xl:col-start-2 xl:row-start-1' : ''"
            :title="nowPlayingTitle"
            :subtitle="nowPlayingSubtitle"
            :cover-url="nowPlayingCoverUrl"
            :lyrics="activeLyrics"
            :lyrics-loading="isLoadingLyrics"
            :lyrics-error="lyricsError"
            :playback-position="playbackPosition"
            :can-retry="Boolean(activeTrack || activeRemoteLyricsQuery)"
            @retry-lyrics="retryLyrics"
          />

          <section
            v-if="activeSection === 'local'"
            class="rounded border border-base-300 bg-base-100 p-4"
          >
            <h2 class="text-base font-semibold">Indexing</h2>
            <p class="mt-1 truncate text-sm text-base-content/65">
              {{ selectedFolder || "No folder selected" }}
            </p>

            <div class="mt-4 grid grid-cols-2 gap-2 text-sm">
              <div class="rounded bg-base-200 p-3">
                <div class="text-xs text-base-content/60">Discovered</div>
                <div class="font-semibold tabular-nums">{{ scanStatus.discoveredFiles }}</div>
              </div>
              <div class="rounded bg-base-200 p-3">
                <div class="text-xs text-base-content/60">Indexed</div>
                <div class="font-semibold tabular-nums">{{ scanStatus.indexedTracks }}</div>
              </div>
              <div class="rounded bg-base-200 p-3">
                <div class="text-xs text-base-content/60">Skipped</div>
                <div class="font-semibold tabular-nums">{{ scanStatus.skippedFiles }}</div>
              </div>
              <div class="rounded bg-base-200 p-3">
                <div class="text-xs text-base-content/60">Errors</div>
                <div class="font-semibold tabular-nums">{{ scanStatus.errorCount }}</div>
              </div>
            </div>

            <progress class="progress mt-4" :value="scanPercent" max="100"></progress>
            <p class="mt-2 text-sm text-base-content/65">
              {{ scanMessage || (scanStatus.isRunning ? "Indexing local tracks" : "Idle") }}
            </p>

            <div v-if="scanStatus.lastError" role="alert" class="alert alert-warning mt-4 alert-soft">
              <AlertCircle :size="18" aria-hidden="true" />
              <span class="text-sm">{{ scanStatus.lastError }}</span>
            </div>
          </section>

          <div
            v-if="activeSection === 'sources'"
            class="flex min-w-0 flex-col gap-4 xl:col-start-1 xl:row-start-1"
          >
          <NeteaseSource
            :stream-quality="remoteQuality"
            @playback-ready="playNeteasePlayback"
            @open-plugins="selectSection('plugins')"
          />

          <section
            v-if="activeSection === 'sources'"
            class="rounded border border-base-300 bg-base-100 p-4"
          >
            <h2 class="text-base font-semibold">Other Source Providers</h2>
            <p class="mt-1 text-sm text-base-content/65">
              Search through the Rust qsvip port, or resolve a known platform ID through bundled LX templates.
            </p>

            <div class="mt-4 flex gap-2">
              <input
                v-model="remoteSearchKeyword"
                class="input input-sm min-w-0 flex-1"
                type="text"
                placeholder="Search remote music"
                @keyup.enter="searchRemoteMusic"
              />
              <button
                class="btn btn-sm"
                type="button"
                :disabled="hasActiveRemoteRequest || !remoteSearchKeyword.trim()"
                @click="searchRemoteMusic"
              >
                <RefreshCw v-if="isSearchingRemote" class="animate-spin" :size="16" aria-hidden="true" />
                <span v-else>Search</span>
              </button>
            </div>

            <div v-if="remoteSearchResults.length" class="mt-3 rounded border border-base-300 bg-base-200/50">
              <div class="border-b border-base-300 px-3 py-2 text-xs text-base-content/65">
                qsvip results<span v-if="remoteSearchTotal !== null"> · {{ remoteSearchTotal }}</span>
              </div>
              <ul class="list max-h-72 overflow-y-auto">
                <li v-for="result in remoteSearchResults" :key="result.id" class="list-row px-3 py-2">
                  <button
                    class="btn btn-square btn-ghost btn-sm"
                    type="button"
                    :disabled="isPreparingPlayback || hasActiveRemoteRequest"
                    :aria-label="`Play ${result.title}`"
                    @click="playRemoteSearchResult(result)"
                  >
                    <Play :size="15" aria-hidden="true" />
                  </button>
                  <div class="min-w-0">
                    <div class="truncate text-sm font-medium">{{ result.title }}</div>
                    <div class="truncate text-xs text-base-content/65">
                      {{ result.artist }}<span v-if="result.album"> · {{ result.album }}</span>
                    </div>
                  </div>
                  <div class="text-right text-xs tabular-nums text-base-content/60">
                    {{ formatDuration(result.durationSeconds) }}
                  </div>
                </li>
              </ul>
            </div>

            <div class="mt-4 grid grid-cols-2 gap-2">
              <label class="flex flex-col gap-1 text-xs">
                <span>Family</span>
                <select v-model="remoteFamily" class="select select-sm w-full">
                  <option value="nianxin">念心</option>
                  <option value="changqing">长青</option>
                </select>
              </label>
              <label class="flex flex-col gap-1 text-xs">
                <span>Source</span>
                <select v-model="remoteSource" class="select select-sm w-full">
                  <option value="wy">网易云</option>
                  <option value="tx">QQ</option>
                  <option value="kw">酷我</option>
                  <option value="kg">酷狗</option>
                  <option value="mg">咪咕</option>
                </select>
              </label>
              <label class="flex flex-col gap-1 text-xs">
                <span>Quality</span>
                <select v-model="remoteQuality" class="select select-sm w-full">
                  <option value="128k">128k</option>
                  <option value="320k">320k</option>
                  <option value="flac">flac</option>
                  <option value="flac24bit">flac24bit</option>
                </select>
              </label>
              <label class="flex flex-col gap-1 text-xs">
                <span>Track ID</span>
                <input
                  v-model="remoteTrackId"
                  class="input input-sm w-full"
                  type="text"
                  placeholder="e.g. 347230"
                  @keyup.enter="playRemoteTrack"
                />
              </label>
            </div>

            <button
              class="btn btn-primary btn-sm mt-3 w-full"
              type="button"
              :disabled="hasActiveRemoteRequest || !remoteTrackId.trim()"
              @click="playRemoteTrack"
            >
              <RefreshCw v-if="isResolvingRemote" class="animate-spin" :size="16" aria-hidden="true" />
              <Play v-else :size="16" aria-hidden="true" />
              Resolve & play
            </button>

            <button
              v-if="hasActiveRemoteRequest"
              class="btn btn-ghost btn-sm mt-2 w-full"
              type="button"
              :disabled="isCancellingRemoteRequest"
              title="Cancel remote request"
              @click="cancelActiveRemoteRequest"
            >
              <RefreshCw v-if="isCancellingRemoteRequest" class="animate-spin" :size="16" aria-hidden="true" />
              <X v-else :size="16" aria-hidden="true" />
              Cancel request
            </button>

            <div v-if="remoteDiagnostics.length" role="alert" class="alert alert-info alert-soft mt-3">
              <AlertCircle :size="18" aria-hidden="true" />
              <div class="text-xs">
                <div v-for="message in remoteDiagnostics" :key="message">{{ message }}</div>
              </div>
            </div>
          </section>
          </div>

        </aside>
      </div>
    </section>

        <section
          v-if="activeSection === 'plugins'"
          class="mx-auto w-full max-w-7xl"
          :class="layoutDensity === 'compact' ? 'px-3 py-3 lg:px-4' : 'px-4 py-5 lg:px-6'"
        >
          <PluginManager />
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
                  <span class="block text-xs text-base-content/60">Use the device theme or choose an override</span>
                </label>
                <select
                  id="theme-preference"
                  v-model="themePreference"
                  class="select select-sm w-full sm:w-44"
                >
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </div>

              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="layout-density" class="flex min-w-0 items-start gap-3">
                  <Gauge class="mt-0.5 shrink-0 text-base-content/60" :size="17" aria-hidden="true" />
                  <span>
                    <span class="block text-sm font-medium">Layout density</span>
                    <span class="block text-xs text-base-content/60">Adjust page spacing and library rows</span>
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

          <section class="overflow-hidden rounded border border-base-300 bg-base-100">
            <div class="flex items-center gap-3 border-b border-base-300 px-4 py-3">
              <Headphones :size="18" aria-hidden="true" />
              <h2 class="text-base font-semibold">Playback</h2>
            </div>
            <div class="divide-y divide-base-300">
              <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
                <label for="stream-quality" class="min-w-0">
                  <span class="block text-sm font-medium">Default stream quality</span>
                  <span class="block text-xs text-base-content/60">Used when resolving tracks from Audio Sources</span>
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
                  <span class="block text-xs text-base-content/60">Applied to the current and next track</span>
                </label>
                <div class="flex w-full items-center gap-3 sm:w-64">
                  <Volume2 :size="17" aria-hidden="true" />
                  <input
                    id="default-volume"
                    v-model.number="volume"
                    class="range range-sm min-w-0 flex-1"
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    @input="updateVolume"
                  />
                  <output class="w-10 text-right text-xs tabular-nums text-base-content/65">
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
                <div class="truncate text-xs text-base-content/60" :title="selectedFolder || undefined">
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
          class="mx-auto grid w-full max-w-7xl grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-2 md:grid-cols-[minmax(0,1fr)_minmax(13rem,1.4fr)] lg:grid-cols-[minmax(0,1fr)_minmax(15rem,1.5fr)_minmax(7rem,1fr)]"
          :class="layoutDensity === 'compact' ? 'px-3 py-2 lg:px-4' : 'px-4 py-3 lg:px-6'"
        >
          <div class="flex min-w-0 items-center gap-3">
            <div class="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200 sm:size-11">
              <img
                v-if="nowPlayingCoverUrl"
                class="size-full object-cover"
                :src="nowPlayingCoverUrl"
                alt=""
              />
              <Music2 v-else :size="21" aria-hidden="true" />
            </div>
            <div class="min-w-0">
              <div class="truncate text-sm font-medium">{{ nowPlayingTitle }}</div>
              <div class="truncate text-xs text-base-content/60">{{ nowPlayingSubtitle }}</div>
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
                :disabled="isPreparingPlayback || (!activeTrack && !activeRemoteTitle && !tracks.length)"
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
              <span class="hidden w-9 text-right text-xs tabular-nums text-base-content/60 sm:block">
                {{ formatPlaybackTime(playbackPosition) }}
              </span>
              <input
                class="range range-xs min-w-0 flex-1"
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
              <span class="hidden w-9 text-xs tabular-nums text-base-content/60 sm:block">
                {{ formatPlaybackTime(playbackDuration) }}
              </span>
            </div>
          </div>

          <div class="hidden min-w-0 items-center justify-end gap-2 lg:col-start-3 lg:row-start-1 lg:flex">
            <Volume2 class="shrink-0" :size="17" aria-hidden="true" />
            <input
              v-model.number="volume"
              class="range range-xs min-w-0 max-w-32"
              type="range"
              min="0"
              max="1"
              step="0.01"
              aria-label="Volume"
              @input="updateVolume"
            />
          </div>

          <audio
            v-if="audioUrl"
            ref="audioElement"
            class="hidden"
            :src="audioUrl"
            :type="activeSource?.mimeType"
            @durationchange="onAudioLoadedMetadata"
            @ended="onAudioEnded"
            @loadedmetadata="onAudioLoadedMetadata"
            @pause="onAudioPause"
            @play="onAudioPlay"
            @timeupdate="onAudioTimeUpdate"
            @error="onAudioError"
          ></audio>
        </div>
      </footer>
    </div>

    <div class="drawer-side z-40">
      <label for="app-sidebar" aria-label="Close navigation" class="drawer-overlay"></label>
      <aside class="flex min-h-full w-60 flex-col border-r border-base-300 bg-base-100">
        <div class="flex min-h-16 items-center gap-3 border-b border-base-300 px-4">
          <div class="flex size-9 shrink-0 items-center justify-center rounded bg-neutral text-neutral-content">
            <Music2 :size="20" aria-hidden="true" />
          </div>
          <div class="min-w-0">
            <div class="truncate text-base font-semibold leading-tight">Fika Music</div>
            <div class="truncate text-xs text-base-content/60">Local-first library</div>
          </div>
        </div>

        <nav class="flex flex-1 flex-col p-3" aria-label="Primary navigation">
          <ul
            class="menu w-full gap-1 p-0"
            :class="layoutDensity === 'compact' ? 'menu-sm' : 'menu-md'"
          >
            <li v-for="section in mainSections" :key="section.id">
              <button
                type="button"
                :class="{ 'menu-active': activeSection === section.id }"
                :aria-current="activeSection === section.id ? 'page' : undefined"
                @click="selectSection(section.id)"
              >
                <component :is="section.icon" :size="18" aria-hidden="true" />
                <span>{{ section.label }}</span>
              </button>
            </li>
          </ul>

          <ul
            class="menu mt-auto w-full gap-1 p-0 pt-4"
            :class="layoutDensity === 'compact' ? 'menu-sm' : 'menu-md'"
          >
            <li>
              <button
                type="button"
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
          <div class="truncate text-xs text-base-content/60" :title="selectedFolder || undefined">
            {{ selectedFolder || "No music folder" }}
          </div>
          <div class="mt-1 text-sm font-medium tabular-nums">
            {{ tracks.length }} track{{ tracks.length === 1 ? "" : "s" }} indexed
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>
