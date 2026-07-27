import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  AudioSourceRecord,
  MusicRecommendationKind,
  OnlineAlbum,
  OnlineArtist,
  OnlineChannel,
  OnlineDownloadProgressEvent,
  OnlineDownloadTask,
  OnlineMusicSettings,
  OnlinePlaylist,
  OnlinePlaylistDetailError,
  OnlinePlaylistsResult,
  OnlineRecommendationsResult,
  OnlineSearchSection,
  OnlineSearchSectionEvent,
  OnlineSearchSectionResult,
  OnlineSuggestionsResult,
  OnlineTrack,
  OnlineTrackPage,
  SourceQuality,
} from "../generated/bindings";
import { dispatchAudioSourceRequest } from "./audio-source-api";
import {
  AudioSourceRouter,
  playbackAttemptKey,
} from "./audio-source-router";
import { firstSuccessfulWithTimeout } from "./async-utils";
import { ExpiringCache } from "./expiring-cache";
import { cancelSourceRequest } from "./plugin-api";

export type {
  MusicRecommendationKind,
  OnlineAlbum,
  OnlineArtist,
  OnlineChannel,
  OnlineDownloadProgressEvent,
  OnlineDownloadTask,
  OnlineMusicSettings,
  OnlinePlaylist,
  OnlinePlaylistDetailError,
  OnlinePlaylistsResult,
  OnlineRecommendationsResult,
  OnlineSearchSection,
  OnlineSearchSectionEvent,
  OnlineSearchSectionResult,
  OnlineSuggestionsResult,
  OnlineTrack,
  OnlineTrackPage,
} from "../generated/bindings";

export const ONLINE_SEARCH_SECTION_EVENT = "online-music:search-section";
export const ONLINE_DOWNLOAD_TASK_EVENT = "online-music:download-task";
export const ONLINE_DOWNLOAD_PROGRESS_EVENT = "online-music:download-progress";
export const ONLINE_DOWNLOAD_COMPLETED_EVENT = "online-music:download-completed";

export type OnlinePlayback = {
  track: OnlineTrack;
  url: string;
  providerName: string;
  candidate: Pick<
    OnlineTrack["candidates"][number],
    "id" | "pluginId" | "sourceId" | "channelId" | "channelName"
  >;
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
  router?: AudioSourceRouter;
  cacheFailures?: boolean;
  bypassResolvedCache?: boolean;
};

const defaultAudioSourceRouter = new AudioSourceRouter();
const preloadedMedia = new Map<string, HTMLAudioElement>();

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

export function getOnlineMusicRecommendations(
  kind: MusicRecommendationKind,
  requestId?: string,
) {
  return invoke<OnlineRecommendationsResult>(TAURI_COMMANDS.onlineMusicRecommendations, {
    kind,
    requestId,
  });
}

export function getOnlineMusicPlaylists(requestId?: string) {
  return invoke<OnlinePlaylistsResult>(TAURI_COMMANDS.onlineMusicPlaylists, {
    requestId,
  });
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

export function listenOnlineDownloadProgress(
  handler: (progress: OnlineDownloadProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<OnlineDownloadProgressEvent>(ONLINE_DOWNLOAD_PROGRESS_EVENT, (event) => {
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
  const qualities = qualityFallback(options.quality ?? options.settings.preferredQuality);
  const router = options.router ?? defaultAudioSourceRouter;
  const mode = options.settings.audioSourceSelectionMode ?? "automatic";
  const sources = router.order({
    records: options.audioSources,
    track: options.track,
    qualities,
    mode,
    configuredPriority: options.settings.audioSourcePriority,
    selectedAudioSourceId: options.selectedAudioSourceId,
  });
  if (!sources.length) {
    throw new Error("No enabled Audio Source can resolve this track.");
  }

  const deadline = Date.now() + options.settings.playbackTimeoutSeconds * 1_000;
  const probe = options.probe ?? probeMediaUrl;
  const failures: unknown[] = [];
  if (mode === "automatic") {
    for (let index = 0; index < sources.length; index += 2) {
      throwIfAborted(options.signal);
      try {
        return await raceAudioSourcePair({
          options,
          sources: sources.slice(index, index + 2),
          qualities,
          deadline,
          probe,
          router,
        });
      } catch (error) {
        if (isAbortError(error)) throw error;
        failures.push(error);
      }
    }
  } else {
    for (const source of sources) {
      throwIfAborted(options.signal);
      const remaining = deadline - Date.now();
      if (remaining <= 0) break;
      try {
        return await resolveFromAudioSourceLayer({
          ...options,
          audioSource: source,
          qualities,
          timeoutMs: Math.min(
            remaining,
            options.settings.layerTimeoutSeconds * 1_000,
          ),
          probe,
          router,
        });
      } catch (error) {
        if (isAbortError(error)) throw error;
        failures.push(error);
      }
    }
  }
  throwIfAborted(options.signal);
  throw new Error(
    failures.length
      ? "Playback is unavailable from the configured Audio Sources."
      : "Playback timed out before a source became available.",
  );
}

type SourcePairOptions = {
  options: ResolveOnlineTrackOptions;
  sources: AudioSourceRecord[];
  qualities: SourceQuality[];
  deadline: number;
  probe: typeof probeMediaUrl;
  router: AudioSourceRouter;
};

async function raceAudioSourcePair(options: SourcePairOptions): Promise<OnlinePlayback> {
  const [primary, secondary] = options.sources;
  if (!primary) throw new Error("No Audio Source layer is available.");
  const controllers = options.sources.map(() => new AbortController());
  const onAbort = () => controllers.forEach((controller) => controller.abort());
  options.options.signal?.addEventListener("abort", onAbort, { once: true });
  if (options.options.signal?.aborted) onAbort();

  let timer: ReturnType<typeof setTimeout> | undefined;
  let settled = false;
  let secondaryStarted = false;
  let started = 0;
  let failed = 0;

  const result = new Promise<OnlinePlayback>((resolve, reject) => {
    const finishWithFailure = (error: unknown) => {
      if (settled) return;
      failed += 1;
      if (options.options.signal?.aborted) {
        settled = true;
        reject(abortError());
        return;
      }
      if (!secondaryStarted && secondary) {
        startSecondary();
        return;
      }
      if (failed === started) {
        settled = true;
        reject(error);
      }
    };

    const start = (source: AudioSourceRecord, index: number) => {
      if (settled) return;
      started += 1;
      const remaining = options.deadline - Date.now();
      if (remaining <= 0) {
        finishWithFailure(new Error("Playback timed out before a source became available."));
        return;
      }
      void resolveFromAudioSourceLayer({
        ...options.options,
        audioSource: source,
        qualities: options.qualities,
        timeoutMs: Math.min(
          remaining,
          options.options.settings.layerTimeoutSeconds * 1_000,
        ),
        signal: controllers[index].signal,
        probe: options.probe,
        router: options.router,
      }).then((playback) => {
        if (settled) return;
        settled = true;
        controllers.forEach((controller, controllerIndex) => {
          if (controllerIndex !== index) controller.abort();
        });
        resolve(playback);
      }).catch(finishWithFailure);
    };

    const startSecondary = () => {
      if (secondaryStarted || !secondary || settled) return;
      secondaryStarted = true;
      if (timer !== undefined) clearTimeout(timer);
      start(secondary, 1);
    };

    start(primary, 0);
    if (secondary && !secondaryStarted && !settled) {
      timer = setTimeout(
        startSecondary,
        options.router.hedgeDelayMs(primary, options.options.track, options.qualities),
      );
    }
  });

  try {
    return await result;
  } finally {
    settled = true;
    if (timer !== undefined) clearTimeout(timer);
    controllers.forEach((controller) => controller.abort());
    options.options.signal?.removeEventListener("abort", onAbort);
  }
}

type LayerOptions = ResolveOnlineTrackOptions & {
  audioSource: AudioSourceRecord;
  qualities: SourceQuality[];
  timeoutMs: number;
  probe: typeof probeMediaUrl;
  router: AudioSourceRouter;
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
    if (remaining <= 0) break;
    const remainingQualityCount = options.qualities.length - index;
    const qualityBudget = Math.max(1, Math.floor(remaining / remainingQualityCount));
    try {
      return await raceCandidates(options, supportedCandidates, quality, qualityBudget);
    } catch (error) {
      if (isAbortError(error)) throw error;
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
  const attemptKeys = candidates.map((candidate) => playbackAttemptKey(
    options.audioSource.id,
    candidate.channelId,
    quality,
  ));
  const hasHealthyAttempt = attemptKeys.some((attemptKey) =>
    options.router.isAttemptAvailable(attemptKey)
  );
  const recoveryAttempt = hasHealthyAttempt ? null : options.router.recoveryAttempt(attemptKeys);
  const branchIds = candidates.map(
    (_, index) => `online-play-${uniqueRequestId()}-${index}`,
  );
  const branchAbort = new AbortController();
  const startedAttempts = new Set<string>();
  const settledAttempts = new Set<string>();
  const onAbort = () => branchAbort.abort();
  options.signal?.addEventListener("abort", onAbort, { once: true });

  try {
    const attempts = candidates.map(async (candidate, index) => {
      const attemptKey = attemptKeys[index];
      const failureKey = `${options.track.key}::${attemptKey}`;
      if (
        options.excludedAttempts?.has(attemptKey) ||
        (options.cacheFailures !== false && isCachedFailure(failureKey)) ||
        (hasHealthyAttempt && !options.router.isAttemptAvailable(attemptKey)) ||
        (!hasHealthyAttempt && attemptKey !== recoveryAttempt)
      ) {
        throw new Error("Playback attempt is temporarily unavailable.");
      }
      const requestId = branchIds[index];
      startedAttempts.add(attemptKey);
      const cacheKey = resolvedPlaybackCacheKey(
        options.audioSource.id,
        candidate.channelId,
        candidate.id,
        quality,
      );
      const startedAt = Date.now();
      try {
        const cached = options.bypassResolvedCache ? null : cachedPlayback(cacheKey);
        if (cached && !options.excludedUrls?.has(cached.url)) {
          await options.probe(cached.url, {
            timeoutMs,
            signal: branchAbort.signal,
          });
          settledAttempts.add(attemptKey);
          options.router.reportSuccess(attemptKey, Date.now() - startedAt);
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
          providerName: options.audioSource.name,
          candidate: {
            id: candidate.id,
            pluginId: candidate.pluginId,
            sourceId: candidate.sourceId,
            channelId: candidate.channelId,
            channelName: candidate.channelName,
          },
          audioSourceId: options.audioSource.id,
          quality,
        } satisfies OnlinePlayback;
        resolvedPlaybackCache.set(cacheKey, playback);
        failedPlaybackCache.delete(failureKey);
        settledAttempts.add(attemptKey);
        options.router.reportSuccess(attemptKey, Date.now() - startedAt);
        return playback;
      } catch (error) {
        resolvedPlaybackCache.delete(cacheKey);
        if (!isAbortError(error) && !branchAbort.signal.aborted) {
          settledAttempts.add(attemptKey);
          if (options.cacheFailures !== false) failedPlaybackCache.set(failureKey, true);
          options.router.reportFailure(attemptKey);
        }
        throw error;
      }
    });
    try {
      return await firstSuccessfulWithTimeout(attempts, timeoutMs, options.signal);
    } catch (error) {
      if (!isAbortError(error) && !options.signal?.aborted) {
        for (const attemptKey of startedAttempts) {
          if (settledAttempts.has(attemptKey)) continue;
          settledAttempts.add(attemptKey);
          options.router.reportFailure(attemptKey);
        }
      }
      throw error;
    }
  } finally {
    branchAbort.abort();
    options.signal?.removeEventListener("abort", onAbort);
    await Promise.allSettled(branchIds.map((requestId) => cancelSourceRequest(requestId)));
  }
}

export function reportOnlinePlaybackFailure(attemptKey: string) {
  defaultAudioSourceRouter.reportFailure(attemptKey);
}

export { playbackAttemptKey } from "./audio-source-router";

const RESOLVED_URL_CACHE_MS = 2 * 60_000;
const FAILED_ATTEMPT_CACHE_MS = 5 * 60_000;
const resolvedPlaybackCache = new ExpiringCache<string, OnlinePlayback>(
  RESOLVED_URL_CACHE_MS,
  128,
);
const failedPlaybackCache = new ExpiringCache<string, true>(
  FAILED_ATTEMPT_CACHE_MS,
  256,
);

function resolvedPlaybackCacheKey(
  audioSourceId: string,
  channelId: string,
  candidateId: string,
  quality: SourceQuality,
) {
  return `${audioSourceId}::${channelId}::${candidateId}::${quality}`;
}

function cachedPlayback(key: string) {
  return resolvedPlaybackCache.get(key) ?? null;
}

function isCachedFailure(key: string) {
  return failedPlaybackCache.get(key) === true;
}

export function clearOnlinePlaybackFailures(trackKey: string) {
  failedPlaybackCache.deleteWhere((key) => key.startsWith(`${trackKey}::`));
}

export function invalidateOnlinePlaybackCaches() {
  resolvedPlaybackCache.clear();
  failedPlaybackCache.clear();
  defaultAudioSourceRouter.reset();
  clearPreloadedMedia();
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
  return loadMediaUrl(url, "metadata", options);
}

export function preloadMediaUrl(
  url: string,
  options: { timeoutMs: number; signal?: AbortSignal },
): Promise<void> {
  return loadMediaUrl(url, "auto", options, true);
}

export function clearPreloadedMedia(exceptUrl?: string): void {
  for (const [url, audio] of preloadedMedia) {
    if (url === exceptUrl) continue;
    releaseMediaElement(audio);
    preloadedMedia.delete(url);
  }
}

function loadMediaUrl(
  url: string,
  preload: "auto" | "metadata",
  options: { timeoutMs: number; signal?: AbortSignal },
  retain = false,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const audio = new Audio();
    audio.preload = preload;
    const timer = window.setTimeout(() => finish(new Error("Media probe timed out.")), options.timeoutMs);
    const onCanPlay = () => finish();
    const onError = () => finish(new Error("Media URL is not playable."));
    const onAbort = () => finish(abortError());
    function finish(error?: Error) {
      window.clearTimeout(timer);
      audio.removeEventListener("canplay", onCanPlay);
      audio.removeEventListener("error", onError);
      options.signal?.removeEventListener("abort", onAbort);
      if (error || !retain) {
        releaseMediaElement(audio);
      } else {
        const previous = preloadedMedia.get(url);
        if (previous && previous !== audio) releaseMediaElement(previous);
        preloadedMedia.set(url, audio);
      }
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

function releaseMediaElement(audio: HTMLAudioElement) {
  audio.pause();
  audio.removeAttribute("src");
  audio.load();
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
