import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  AudioSourceRecord,
  OnlineAlbum,
  OnlineArtist,
  OnlineChannel,
  OnlineDownloadTask,
  OnlineMusicSettings,
  OnlinePlaylist,
  OnlinePlaylistDetailError,
  OnlineSearchSection,
  OnlineSearchSectionEvent,
  OnlineSearchSectionResult,
  OnlineSuggestionsResult,
  OnlineTrack,
  OnlineTrackPage,
  SourceQuality,
} from "../generated/bindings";
import { dispatchAudioSourceRequest } from "./audio-source-api";
import { cancelSourceRequest } from "./plugin-api";

export type {
  OnlineAlbum,
  OnlineArtist,
  OnlineChannel,
  OnlineDownloadTask,
  OnlineMusicSettings,
  OnlinePlaylist,
  OnlinePlaylistDetailError,
  OnlineSearchSection,
  OnlineSearchSectionEvent,
  OnlineSearchSectionResult,
  OnlineSuggestionsResult,
  OnlineTrack,
  OnlineTrackPage,
} from "../generated/bindings";

export const ONLINE_SEARCH_SECTION_EVENT = "online-music:search-section";
export const ONLINE_DOWNLOAD_TASK_EVENT = "online-music:download-task";
export const ONLINE_DOWNLOAD_COMPLETED_EVENT = "online-music:download-completed";

export type OnlinePlayback = {
  track: OnlineTrack;
  url: string;
  mimeType: string;
  providerName: string;
  channelId: string;
  channelName: string;
  audioSourceId: string;
  quality: SourceQuality;
};

export type ResolveOnlineTrackOptions = {
  track: OnlineTrack;
  audioSources: AudioSourceRecord[];
  settings: OnlineMusicSettings;
  selectedAudioSourceId?: string;
  quality?: SourceQuality;
  signal?: AbortSignal;
  probe?: typeof probeMediaUrl;
  excludedAttempts?: Set<string>;
  excludedUrls?: Set<string>;
};

export function getOnlineMusicSettings() {
  return invoke<OnlineMusicSettings>(TAURI_COMMANDS.getOnlineMusicSettings);
}

export async function updateOnlineMusicSettings(settings: OnlineMusicSettings) {
  const updated = await invoke<OnlineMusicSettings>(TAURI_COMMANDS.updateOnlineMusicSettings, {
    settings,
  });
  invalidateOnlinePlaybackCaches();
  return updated;
}

export function listOnlineMusicChannels(includeExcluded = false) {
  return invoke<OnlineChannel[]>(TAURI_COMMANDS.listOnlineMusicChannels, { includeExcluded });
}

export function clearOnlineSearchHistory() {
  return invoke<void>(TAURI_COMMANDS.clearOnlineSearchHistory);
}

export function selectOnlineDownloadDirectory(initialDirectory?: string | null) {
  return invoke<string | null>(TAURI_COMMANDS.selectOnlineDownloadDirectory, {
    initialDirectory: initialDirectory ?? null,
  });
}

export function createOnlineDownloadTask(
  kind: string,
  title: string,
  tracks: OnlineTrack[],
  selectedAudioSourceId?: string,
  localMusicFolder?: string | null,
) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.createOnlineDownloadTask, {
    kind,
    title,
    tracks,
    selectedAudioSourceId,
    localMusicFolder,
  });
}

export function listOnlineDownloadTasks() {
  return invoke<OnlineDownloadTask[]>(TAURI_COMMANDS.listOnlineDownloadTasks);
}

export function startOnlineDownloadTask(taskId: string) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.startOnlineDownloadTask, { taskId });
}

export function pauseOnlineDownloadTask(taskId: string) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.pauseOnlineDownloadTask, { taskId });
}

export function cancelOnlineDownloadTask(taskId: string) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.cancelOnlineDownloadTask, { taskId });
}

export function retryOnlineDownloadItem(taskId: string, itemId: string) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.retryOnlineDownloadItem, { taskId, itemId });
}

export function refreshOnlineDownloadItemCandidates(taskId: string, itemId: string) {
  return invoke<OnlineDownloadTask>(TAURI_COMMANDS.refreshOnlineDownloadItemCandidates, {
    taskId,
    itemId,
  });
}

export function listenOnlineDownloadTasks(
  handler: (task: OnlineDownloadTask) => void,
): Promise<UnlistenFn> {
  return listen<OnlineDownloadTask>(ONLINE_DOWNLOAD_TASK_EVENT, (event) => {
    handler(event.payload);
  });
}

export function listenOnlineDownloadCompletions(
  handler: (task: OnlineDownloadTask) => void,
): Promise<UnlistenFn> {
  return listen<OnlineDownloadTask>(ONLINE_DOWNLOAD_COMPLETED_EVENT, (event) => {
    handler(event.payload);
  });
}

export function getOnlineMusicSuggestions(keyword: string, requestId?: string) {
  return invoke<OnlineSuggestionsResult>(TAURI_COMMANDS.onlineMusicSuggestions, {
    keyword,
    requestId,
  });
}

export function startOnlineMusicSearch(keyword: string) {
  return invoke<string>(TAURI_COMMANDS.startOnlineMusicSearch, { keyword });
}

export function listenOnlineMusicSearch(
  handler: (event: OnlineSearchSectionEvent) => void,
): Promise<UnlistenFn> {
  return listen<OnlineSearchSectionEvent>(ONLINE_SEARCH_SECTION_EVENT, (event) => {
    handler(event.payload);
  });
}

export function getOnlineMusicSearchPage(
  keyword: string,
  section: OnlineSearchSection,
  page: number,
  pageSize = 20,
  requestId?: string,
) {
  return invoke<OnlineSearchSectionResult>(TAURI_COMMANDS.onlineMusicSearchPage, {
    keyword,
    section,
    page,
    pageSize,
    requestId,
  });
}

export function getOnlineArtistTracks(artist: OnlineArtist, requestId?: string) {
  return invoke<OnlineTrackPage>(TAURI_COMMANDS.onlineMusicArtistTracks, {
    artist,
    requestId,
  });
}

export function getOnlineAlbumTracks(
  album: OnlineAlbum,
  page: number,
  pageSize = 100,
  requestId?: string,
) {
  return invoke<OnlineTrackPage>(TAURI_COMMANDS.onlineMusicAlbumTracks, {
    album,
    page,
    pageSize,
    requestId,
  });
}

export function getOnlinePlaylistTracks(
  playlist: OnlinePlaylist,
  page: number,
  pageSize = 100,
  requestId?: string,
) {
  return invoke<OnlineTrackPage>(TAURI_COMMANDS.onlineMusicPlaylistTracks, {
    playlist,
    page,
    pageSize,
    requestId,
  });
}

export function onlinePlaylistDetailError(error: unknown): OnlinePlaylistDetailError | null {
  let candidate = error;
  if (typeof candidate === "string") {
    try {
      candidate = JSON.parse(candidate) as unknown;
    } catch {
      return null;
    }
  }
  if (!candidate || typeof candidate !== "object") return null;
  const value = candidate as Partial<OnlinePlaylistDetailError>;
  return typeof value.code === "string" &&
    typeof value.message === "string" &&
    typeof value.pluginId === "string" &&
    typeof value.channelName === "string"
    ? value as OnlinePlaylistDetailError
    : null;
}

export async function resolveOnlineTrack(
  options: ResolveOnlineTrackOptions,
): Promise<OnlinePlayback> {
  const sources = orderedAudioSources(
    options.audioSources,
    options.settings.audioSourcePriority,
    options.selectedAudioSourceId,
  );
  if (!sources.length) {
    throw new Error("No enabled Audio Source can resolve this track.");
  }

  const deadline = Date.now() + options.settings.playbackTimeoutSeconds * 1_000;
  const probe = options.probe ?? probeMediaUrl;
  const failures: unknown[] = [];
  for (const source of sources) {
    throwIfAborted(options.signal);
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      break;
    }
    const layerBudget = Math.min(
      remaining,
      options.settings.layerTimeoutSeconds * 1_000,
    );
    try {
      return await resolveFromAudioSourceLayer({
        ...options,
        audioSource: source,
        qualities: qualityFallback(options.quality ?? options.settings.preferredQuality),
        timeoutMs: layerBudget,
        probe,
      });
    } catch (error) {
      failures.push(error);
    }
  }
  throwIfAborted(options.signal);
  throw new Error(
    failures.length
      ? "Playback is unavailable from the configured Audio Sources."
      : "Playback timed out before a source became available.",
  );
}

type LayerOptions = ResolveOnlineTrackOptions & {
  audioSource: AudioSourceRecord;
  qualities: SourceQuality[];
  timeoutMs: number;
  probe: typeof probeMediaUrl;
};

async function resolveFromAudioSourceLayer(options: LayerOptions): Promise<OnlinePlayback> {
  const deadline = Date.now() + options.timeoutMs;
  const supportedCandidates = options.track.candidates.filter((candidate) =>
    options.audioSource.sources.some(
      (source) =>
        source.id === candidate.sourceId && source.actions.includes("musicUrl"),
    ),
  );
  if (!supportedCandidates.length) {
    throw new Error("Audio Source does not support any candidate channel.");
  }

  for (const [index, quality] of options.qualities.entries()) {
    throwIfAborted(options.signal);
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      break;
    }
    const remainingQualityCount = options.qualities.length - index;
    const qualityBudget = Math.max(1, Math.floor(remaining / remainingQualityCount));
    try {
      return await raceCandidates(options, supportedCandidates, quality, qualityBudget);
    } catch (error) {
      if (isAbortError(error)) {
        throw error;
      }
    }
  }
  throw new Error("Audio Source layer did not produce a playable URL.");
}

async function raceCandidates(
  options: LayerOptions,
  candidates: OnlineTrack["candidates"],
  quality: SourceQuality,
  timeoutMs: number,
): Promise<OnlinePlayback> {
  const branchIds = candidates.map(
    (_, index) => `online-play-${uniqueRequestId()}-${index}`,
  );
  const branchAbort = new AbortController();
  const onAbort = () => branchAbort.abort();
  options.signal?.addEventListener("abort", onAbort, { once: true });

  try {
    const attempts = candidates.map(async (candidate, index) => {
      const attemptKey = playbackAttemptKey(
        options.audioSource.id,
        candidate.channelId,
        quality,
      );
      const failureKey = `${options.track.key}::${attemptKey}`;
      if (
        options.excludedAttempts?.has(attemptKey) ||
        isCachedFailure(failureKey)
      ) {
        throw new Error("Playback attempt is temporarily unavailable.");
      }
      const requestId = branchIds[index];
      const cacheKey = resolvedPlaybackCacheKey(
        options.audioSource.id,
        candidate.channelId,
        candidate.id,
        quality,
      );
      try {
        const cached = cachedPlayback(cacheKey);
        if (cached && !options.excludedUrls?.has(cached.url)) {
          await options.probe(cached.url, {
            timeoutMs,
            signal: branchAbort.signal,
          });
          return { ...cached, track: options.track };
        }
        const outcome = await dispatchAudioSourceRequest(
          options.audioSource.id,
          {
            action: "musicUrl",
            source: candidate.sourceId,
            musicInfo: {
              ...candidate.rawInfo,
              ...candidate.platformIds,
              id: candidate.id,
              title: candidate.title,
              name: candidate.title,
              artist: candidate.artist,
              singer: candidate.artist,
              album: candidate.album,
              albumName: candidate.album,
              duration: candidate.durationSeconds,
            },
            quality,
          },
          requestId,
        );
        if (outcome.response.action !== "musicUrl") {
          throw new Error("Audio Source returned an unexpected response.");
        }
        if (options.excludedUrls?.has(outcome.response.data)) {
          throw new Error("Media URL already failed in this playback session.");
        }
        await options.probe(outcome.response.data, {
          timeoutMs,
          signal: branchAbort.signal,
        });
        const playback = {
          track: options.track,
          url: outcome.response.data,
          mimeType: mediaMimeType(outcome.response.data, quality),
          providerName: options.audioSource.name,
          channelId: candidate.channelId,
          channelName: candidate.channelName,
          audioSourceId: options.audioSource.id,
          quality,
        } satisfies OnlinePlayback;
        resolvedPlaybackCache.set(cacheKey, {
          expiresAt: Date.now() + RESOLVED_URL_CACHE_MS,
          playback,
        });
        failedPlaybackCache.delete(failureKey);
        return playback;
      } catch (error) {
        resolvedPlaybackCache.delete(cacheKey);
        if (!isAbortError(error) && !branchAbort.signal.aborted) {
          failedPlaybackCache.set(failureKey, Date.now() + FAILED_ATTEMPT_CACHE_MS);
        }
        throw error;
      }
    });
    return await promiseAnyWithTimeout(attempts, timeoutMs, options.signal);
  } finally {
    branchAbort.abort();
    options.signal?.removeEventListener("abort", onAbort);
    await Promise.allSettled(branchIds.map((requestId) => cancelSourceRequest(requestId)));
  }
}

export function playbackAttemptKey(
  audioSourceId: string,
  channelId: string,
  quality: SourceQuality,
) {
  return `${audioSourceId}::${channelId}::${quality}`;
}

const RESOLVED_URL_CACHE_MS = 2 * 60_000;
const FAILED_ATTEMPT_CACHE_MS = 5 * 60_000;
const resolvedPlaybackCache = new Map<
  string,
  { expiresAt: number; playback: OnlinePlayback }
>();
const failedPlaybackCache = new Map<string, number>();

function resolvedPlaybackCacheKey(
  audioSourceId: string,
  channelId: string,
  candidateId: string,
  quality: SourceQuality,
) {
  return `${audioSourceId}::${channelId}::${candidateId}::${quality}`;
}

function cachedPlayback(key: string) {
  const cached = resolvedPlaybackCache.get(key);
  if (!cached) return null;
  if (cached.expiresAt <= Date.now()) {
    resolvedPlaybackCache.delete(key);
    return null;
  }
  return cached.playback;
}

function isCachedFailure(key: string) {
  const expiresAt = failedPlaybackCache.get(key);
  if (!expiresAt) return false;
  if (expiresAt <= Date.now()) {
    failedPlaybackCache.delete(key);
    return false;
  }
  return true;
}

export function clearOnlinePlaybackFailures(trackKey: string) {
  for (const key of failedPlaybackCache.keys()) {
    if (key.startsWith(`${trackKey}::`)) failedPlaybackCache.delete(key);
  }
}

export function invalidateOnlinePlaybackCaches() {
  resolvedPlaybackCache.clear();
  failedPlaybackCache.clear();
}

export function orderedAudioSources(
  records: AudioSourceRecord[],
  configuredPriority: string[],
  selectedAudioSourceId?: string,
) {
  const enabled = records.filter(
    (record) =>
      record.enabled &&
      record.state === "enabled" &&
      record.sources.some((source) => source.actions.includes("musicUrl")),
  );
  const order = [selectedAudioSourceId, ...configuredPriority]
    .filter((id): id is string => Boolean(id))
    .filter((id, index, values) => values.indexOf(id) === index);
  return [...enabled].sort((left, right) => {
    const leftIndex = order.indexOf(left.id);
    const rightIndex = order.indexOf(right.id);
    const leftRank = leftIndex < 0 ? order.length : leftIndex;
    const rightRank = rightIndex < 0 ? order.length : rightIndex;
    return leftRank - rightRank || left.name.localeCompare(right.name);
  });
}

export function qualityFallback(quality: SourceQuality): SourceQuality[] {
  const qualities: SourceQuality[] = ["128k", "320k", "flac", "flac24bit"];
  return qualities.slice(0, qualities.indexOf(quality) + 1).reverse();
}

export function probeMediaUrl(
  url: string,
  options: { timeoutMs: number; signal?: AbortSignal },
): Promise<void> {
  return new Promise((resolve, reject) => {
    const audio = new Audio();
    audio.preload = "metadata";
    const timer = window.setTimeout(() => finish(new Error("Media probe timed out.")), options.timeoutMs);
    const onCanPlay = () => finish();
    const onError = () => finish(new Error("Media URL is not playable."));
    const onAbort = () => finish(abortError());
    function finish(error?: Error) {
      window.clearTimeout(timer);
      audio.removeEventListener("canplay", onCanPlay);
      audio.removeEventListener("error", onError);
      options.signal?.removeEventListener("abort", onAbort);
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
      error ? reject(error) : resolve();
    }
    audio.addEventListener("canplay", onCanPlay, { once: true });
    audio.addEventListener("error", onError, { once: true });
    options.signal?.addEventListener("abort", onAbort, { once: true });
    if (options.signal?.aborted) {
      finish(abortError());
      return;
    }
    audio.src = url;
    audio.load();
  });
}

async function promiseAnyWithTimeout<T>(
  promises: Promise<T>[],
  timeoutMs: number,
  signal?: AbortSignal,
) {
  const timeout = new Promise<never>((_, reject) => {
    const timer = window.setTimeout(() => reject(new Error("Source layer timed out.")), timeoutMs);
    signal?.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timer);
        reject(abortError());
      },
      { once: true },
    );
  });
  return Promise.race([firstSuccessful(promises), timeout]);
}

function firstSuccessful<T>(promises: Promise<T>[]) {
  return new Promise<T>((resolve, reject) => {
    if (!promises.length) {
      reject(new Error("No playback candidates are available."));
      return;
    }
    const errors: unknown[] = [];
    promises.forEach((promise, index) => {
      promise.then(resolve).catch((error) => {
        errors[index] = error;
        if (errors.filter((item) => item !== undefined).length === promises.length) {
          reject(new Error("All playback candidates failed."));
        }
      });
    });
  });
}

function mediaMimeType(url: string, quality: SourceQuality) {
  const pathname = url.split(/[?#]/, 1)[0]?.toLowerCase() ?? "";
  if (pathname.endsWith(".flac") || quality === "flac" || quality === "flac24bit") {
    return "audio/flac";
  }
  if (pathname.endsWith(".m4a") || pathname.endsWith(".mp4")) {
    return "audio/mp4";
  }
  if (pathname.endsWith(".aac")) {
    return "audio/aac";
  }
  if (pathname.endsWith(".ogg") || pathname.endsWith(".opus")) {
    return "audio/ogg";
  }
  return "audio/mpeg";
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) {
    throw abortError();
  }
}

function abortError() {
  return new DOMException("The operation was cancelled.", "AbortError");
}

function isAbortError(error: unknown) {
  return error instanceof DOMException && error.name === "AbortError";
}

let fallbackRequestId = 0;

function uniqueRequestId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  fallbackRequestId += 1;
  return `${Date.now()}-${fallbackRequestId}`;
}
