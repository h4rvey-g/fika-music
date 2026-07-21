<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertCircle,
  FolderOpen,
  Library,
  Music2,
  Pause,
  Play,
  RefreshCw,
  Volume2,
  X,
} from "@lucide/vue";
import PluginManager from "./components/PluginManager.vue";

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

const tracks = ref<LocalTrack[]>([]);
const scanStatus = ref<ScanStatus>({ ...emptyScanStatus });
const selectedFolder = ref<string | null>(null);
const activeTrack = ref<LocalTrack | null>(null);
const activeRemoteTitle = ref<string | null>(null);
const activeSource = ref<PlaybackSource | null>(null);
const audioUrl = ref<string | null>(null);
const isPlaying = ref(false);
const volume = ref(0.8);
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
const remoteQuality = ref("128k");
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

  return activeRemoteTitle.value ? "Remote LX template source" : "Select a local or remote track";
});
const hasActiveRemoteRequest = computed(() => activeRemoteRequestId.value !== null);

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
}

function onAudioPause() {
  isPlaying.value = false;
}

function onAudioPlay() {
  isPlaying.value = true;
}

function onAudioError() {
  isPlaying.value = false;
  appError.value = "Playback failed for the selected track.";
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
  <main class="min-h-screen bg-base-200 text-base-content">
    <div class="navbar border-b border-base-300 bg-base-100 px-4 lg:px-6">
      <div class="navbar-start gap-3">
        <div class="flex size-10 items-center justify-center rounded bg-neutral text-neutral-content">
          <Music2 :size="22" aria-hidden="true" />
        </div>
        <div>
          <h1 class="text-lg font-semibold leading-tight">Fika Music</h1>
          <p class="text-xs text-base-content/65">Local-first library</p>
        </div>
      </div>
      <div class="navbar-end gap-2">
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
    </div>

    <section class="mx-auto flex w-full max-w-7xl flex-col gap-5 px-4 py-5 lg:px-6">
      <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
        <section class="flex min-h-[28rem] flex-col overflow-hidden rounded border border-base-300 bg-base-100">
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

          <div v-if="appError" role="alert" class="alert alert-error m-4">
            <AlertCircle :size="18" aria-hidden="true" />
            <span>{{ appError }}</span>
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
            <table class="table table-sm table-zebra">
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

        <aside class="flex flex-col gap-4">
          <section class="rounded border border-base-300 bg-base-100 p-4">
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

          <section class="rounded border border-base-300 bg-base-100 p-4">
            <h2 class="text-base font-semibold">Remote LX template</h2>
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

          <section class="rounded border border-base-300 bg-base-100 p-4">
            <h2 class="text-base font-semibold">Now Playing</h2>
            <div class="mt-4 flex items-start gap-3">
              <div class="flex size-12 shrink-0 items-center justify-center rounded bg-base-200">
                <Music2 :size="24" aria-hidden="true" />
              </div>
              <div class="min-w-0">
                <div class="truncate font-medium">{{ nowPlayingTitle }}</div>
                <div class="truncate text-sm text-base-content/65">{{ nowPlayingSubtitle }}</div>
              </div>
            </div>

            <audio
              v-if="audioUrl"
              ref="audioElement"
              class="mt-4 w-full"
              controls
              :src="audioUrl"
              :type="activeSource?.mimeType"
              @ended="onAudioEnded"
              @pause="onAudioPause"
              @play="onAudioPlay"
              @error="onAudioError"
            ></audio>

            <div class="mt-4 flex items-center gap-3">
              <button
                class="btn btn-square"
                type="button"
                :disabled="isPreparingPlayback || (!activeTrack && !activeRemoteTitle && !tracks.length)"
                :aria-label="isPlaying ? 'Pause playback' : 'Play playback'"
                @click="togglePlayback"
              >
                <Pause v-if="isPlaying" :size="18" aria-hidden="true" />
                <Play v-else :size="18" aria-hidden="true" />
              </button>
              <Volume2 :size="18" aria-hidden="true" />
              <input
                v-model.number="volume"
                class="range range-sm"
                type="range"
                min="0"
                max="1"
                step="0.01"
                aria-label="Volume"
                @input="updateVolume"
              />
            </div>
          </section>
        </aside>
      </div>
    </section>

    <section class="mx-auto w-full max-w-7xl px-4 pb-5 lg:px-6">
      <PluginManager />
    </section>
  </main>
</template>
