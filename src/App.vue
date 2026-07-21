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
  Menu,
  Music2,
  Pause,
  Palette,
  Play,
  Plug,
  RefreshCw,
  RotateCcw,
  Settings,
  Volume2,
  X,
} from "@lucide/vue";
import PluginManager from "./components/PluginManager.vue";
import NeteaseSource from "./components/NeteaseSource.vue";
import type { NeteasePlayback } from "./lib/netease-api";
import {
  DEFAULT_UI_PREFERENCES,
  loadUiPreferences,
  saveUiPreferences,
  type ThemePreference,
} from "./lib/ui-preferences";

type LocalTrack = {
  id: number;
  filePath: string;
  fileName: string;
  title: string;
  artist: string | null;
  album: string | null;
  durationSeconds: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  fileSizeBytes: number;
  modifiedAt: number | null;
  indexedAt: number;
};

type ScanStatus = {
  isRunning: boolean;
  folderPath: string | null;
  discoveredFiles: number;
  scannedFiles: number;
  indexedTracks: number;
  skippedFiles: number;
  errorCount: number;
  lastError: string | null;
  startedAt: number | null;
  finishedAt: number | null;
};

type ScanProgressEvent = {
  status: ScanStatus;
  message: string | null;
};

type MediaSource = {
  filePath: string;
  mimeType: string;
};

type RemoteMediaSource = {
  url: string;
  mimeType: string;
  diagnostics: Array<{
    sourceId: string;
    level: string;
    message: string;
  }>;
};

type RemoteCommandError = {
  message: string;
  diagnostics?: Array<{
    sourceId: string;
    level: string;
    message: string;
  }>;
};

type RemoteSearchResult = {
  id: string;
  source: string;
  title: string;
  artist: string;
  album: string | null;
  durationSeconds: number | null;
  coverUrl: string | null;
  rawInfo: Record<string, unknown>;
};

type RemoteSearchResults = {
  isEnd: boolean;
  total: number | null;
  list: RemoteSearchResult[];
  diagnostics: Array<{
    sourceId: string;
    level: string;
    message: string;
  }>;
};

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
const themePreference = ref(savedUiPreferences.theme);
const layoutDensity = ref(savedUiPreferences.density);
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
const remoteSearchResults = ref<RemoteSearchResult[]>([]);
const remoteSearchTotal = ref<number | null>(null);
const isSearchingRemote = ref(false);
const activeRemoteRequestId = ref<string | null>(null);
const isCancellingRemoteRequest = ref(false);

let unlistenScanProgress: UnlistenFn | null = null;

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

watch(themePreference, applyTheme, { immediate: true });
watch(volume, updateVolume);
watch(audioUrl, () => {
  playbackPosition.value = 0;
  playbackDuration.value = 0;
});
watch([themePreference, layoutDensity, remoteQuality, volume], () => {
  saveUiPreferences({
    theme: themePreference.value,
    density: layoutDensity.value,
    streamQuality: remoteQuality.value,
    volume: volume.value,
  });
});

onMounted(async () => {
  await Promise.all([loadTracks(), loadScanStatus(), bindScanProgress()]);
});

onBeforeUnmount(() => {
  unlistenScanProgress?.();
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
}

async function cancelActiveRemoteRequest() {
  const requestId = activeRemoteRequestId.value;
  if (!requestId || isCancellingRemoteRequest.value) {
    return;
  }

  isCancellingRemoteRequest.value = true;
  try {
    await invoke("cancel_source_request", { requestId });
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
  scanStatus.value = await invoke<ScanStatus>("get_scan_status");
  selectedFolder.value = scanStatus.value.folderPath;
}

async function loadTracks() {
  isLoadingTracks.value = true;
  appError.value = null;

  try {
    tracks.value = await invoke<LocalTrack[]>("list_local_tracks");
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
    const folder = await invoke<string | null>("select_music_folder");
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
    scanStatus.value = await invoke<ScanStatus>("start_library_scan", {
      folderPath: selectedFolder.value,
    });
  } catch (error) {
    appError.value = normalizeError(error);
  } finally {
    isStartingScan.value = false;
  }
}

async function playTrack(track: LocalTrack) {
  isPreparingPlayback.value = true;
  appError.value = null;

  try {
    const source = await invoke<MediaSource>("local_track_media_source", {
      trackId: track.id,
    });

    activeTrack.value = track;
    activeRemoteTitle.value = null;
    activeRemoteProvider.value = null;
    activeSource.value = source;
    audioUrl.value = convertFileSrc(source.filePath);

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
    const source = await invoke<RemoteMediaSource>("resolve_imported_lx_template_music_url", {
      family: remoteFamily.value,
      source: remoteSource.value,
      trackId: remoteTrackId.value.trim(),
      quality: remoteQuality.value,
      requestId,
    });

    activeTrack.value = null;
    activeRemoteTitle.value = `${remoteFamily.value}:${remoteSource.value}:${remoteTrackId.value.trim()}`;
    activeRemoteProvider.value = "Remote LX template source";
    activeSource.value = { url: source.url, mimeType: source.mimeType };
    audioUrl.value = source.url;
    remoteDiagnostics.value = source.diagnostics.map((diagnostic) => diagnostic.message);

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
    const response = await invoke<RemoteSearchResults>("search_qishui_music", {
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

async function playRemoteSearchResult(result: RemoteSearchResult) {
  if (hasActiveRemoteRequest.value) {
    return;
  }
  isResolvingRemote.value = true;
  isPreparingPlayback.value = true;
  appError.value = null;
  remoteDiagnostics.value = [];
  const requestId = beginRemoteRequest();

  try {
    const source = await invoke<RemoteMediaSource>("resolve_qishui_music_url", {
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

function updateVolume() {
  if (audioElement.value) {
    audioElement.value.volume = volume.value;
  }
}

function onAudioEnded() {
  isPlaying.value = false;
  playbackPosition.value = playbackDuration.value;
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
    class="drawer min-h-screen bg-base-200 text-base-content md:drawer-open"
    :data-density="layoutDensity"
  >
    <input id="app-sidebar" v-model="sidebarOpen" type="checkbox" class="drawer-toggle" />

    <div class="drawer-content flex min-h-screen min-w-0 flex-col">
      <header class="navbar sticky top-0 z-30 min-h-16 border-b border-base-300 bg-base-100 px-3 sm:px-4 lg:px-6">
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

      <main class="flex-1">
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
                ? 'grid gap-4 xl:grid-cols-[minmax(0,1fr)_18rem]'
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
                      :aria-label="`Play ${track.title}`"
                      @click="playTrack(track)"
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
            class="flex flex-col gap-4"
            :class="activeSection === 'sources' ? 'mx-auto w-full max-w-6xl' : ''"
          >
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

          <NeteaseSource
            v-if="activeSection === 'sources'"
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
        class="sticky bottom-0 z-30 border-t border-base-300 bg-base-100/95 backdrop-blur"
        aria-label="Playback bar"
      >
        <div
          class="mx-auto grid w-full max-w-7xl grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-2 md:grid-cols-[minmax(0,1fr)_minmax(13rem,1.4fr)] lg:grid-cols-[minmax(0,1fr)_minmax(15rem,1.5fr)_minmax(7rem,1fr)]"
          :class="layoutDensity === 'compact' ? 'px-3 py-2 lg:px-4' : 'px-4 py-3 lg:px-6'"
        >
          <div class="flex min-w-0 items-center gap-3">
            <div class="flex size-10 shrink-0 items-center justify-center rounded bg-base-200 sm:size-11">
              <Music2 :size="21" aria-hidden="true" />
            </div>
            <div class="min-w-0">
              <div class="truncate text-sm font-medium">{{ nowPlayingTitle }}</div>
              <div class="truncate text-xs text-base-content/60">{{ nowPlayingSubtitle }}</div>
            </div>
          </div>

          <div
            class="col-span-2 flex min-w-0 items-center gap-2 md:col-span-1 md:col-start-2 md:row-start-1"
          >
            <button
              class="btn btn-circle btn-neutral btn-sm shrink-0"
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
