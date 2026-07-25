import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioSourceRecord } from "../generated/bindings";
import {
  orderedAudioSources,
  invalidateOnlinePlaybackCaches,
  onlinePlaylistDetailError,
  playbackAttemptKey,
  qualityFallback,
  refreshOnlineDownloadItemCandidates,
  resolveOnlineTrack,
  selectOnlineDownloadDirectory,
} from "./online-music-api";
import {
  createAudioSourceRecord,
  createOnlineMusicSettings,
  createOnlineTrack,
  createOnlineTrackCandidate,
} from "../test/fixtures";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function audioSource(id: string, sources = ["wy", "kg"]): AudioSourceRecord {
  return createAudioSourceRecord({
    id,
    name: id,
    version: "1",
    path: `/sources/${id}`,
    adapter: "test",
    declaredCapabilities: [],
    grantedCapabilities: [],
    sources: sources.map((source) => ({
      id: source,
      name: source,
      type: "music",
      actions: ["musicUrl"],
      qualities: ["128k", "320k"],
    })),
  });
}

const settings = createOnlineMusicSettings({
  audioSourcePriority: ["second", "first"],
});

const track = createOnlineTrack({
  candidates: [
    createOnlineTrackCandidate(),
    createOnlineTrackCandidate({
      channelId: "kugou",
      pluginId: "fika.kugou",
      sourceId: "kg",
      channelName: "KuGou",
      id: "hash",
      platformIds: { hash: "hash" },
    }),
  ],
});

describe("online music playback routing", () => {
  beforeEach(() => {
    invoke.mockReset();
    invalidateOnlinePlaybackCaches();
  });

  it("places the selected Audio Source before the persistent fallback order", () => {
    const result = orderedAudioSources(
      [audioSource("first"), audioSource("second"), audioSource("third")],
      settings.audioSourcePriority,
      "third",
    );

    expect(result.map((source) => source.id)).toEqual(["third", "second", "first"]);
  });

  it("degrades quality only toward lower levels", () => {
    expect(qualityFallback("flac24bit")).toEqual(["flac24bit", "flac", "320k", "128k"]);
    expect(qualityFallback("320k")).toEqual(["320k", "128k"]);
  });

  it("preserves time for lower-quality fallback when the preferred URL probe stalls", async () => {
    invoke.mockImplementation((command: string, payload: { request?: { quality?: string } }) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: {
            action: "musicUrl",
            data: `https://cdn.test/${payload.request?.quality}.mp3`,
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const probe = vi.fn(
      (url: string, options: { signal?: AbortSignal }) => {
        if (!url.includes("/320k.")) return Promise.resolve();
        return new Promise<void>((_resolve, reject) => {
          options.signal?.addEventListener(
            "abort",
            () => reject(new DOMException("The operation was cancelled.", "AbortError")),
            { once: true },
          );
        });
      },
    );

    const playback = await resolveOnlineTrack({
      track,
      audioSources: [audioSource("first", ["kg"])],
      settings: {
        ...settings,
        layerTimeoutSeconds: 0.08,
        playbackTimeoutSeconds: 1,
        preferredQuality: "320k",
      },
      probe,
    });

    expect(playback.quality).toBe("128k");
  });

  it("forwards the Local Music folder as the download picker starting directory", async () => {
    invoke.mockResolvedValue("/music/downloads");

    await expect(selectOnlineDownloadDirectory("/music")).resolves.toBe("/music/downloads");

    expect(invoke).toHaveBeenCalledWith("select_online_download_directory", {
      initialDirectory: "/music",
    });
  });

  it("forwards failed item identity when refreshing persisted candidates", async () => {
    invoke.mockResolvedValue({ taskId: "task-1" });

    await refreshOnlineDownloadItemCandidates("task-1", "item-1");

    expect(invoke).toHaveBeenCalledWith("refresh_online_download_item_candidates", {
      taskId: "task-1",
      itemId: "item-1",
    });
  });

  it("recognizes structured playlist errors from object and JSON rejection values", () => {
    const error = {
      code: "credential-expired",
      message: "session expired",
      pluginId: "fika.netease",
      channelName: "NetEase",
    };

    expect(onlinePlaylistDetailError(error)).toEqual(error);
    expect(onlinePlaylistDetailError(JSON.stringify(error))).toEqual(error);
    expect(onlinePlaylistDetailError("session expired")).toBeNull();
  });

  it("uses the first candidate whose URL probe succeeds", async () => {
    invoke.mockImplementation((command: string, payload: { request?: { source?: string } }) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: {
            action: "musicUrl",
            data: `https://cdn.test/${payload.request?.source}.mp3`,
          },
          diagnostics: [],
        });
      }
      if (command === "cancel_source_request") {
        return Promise.resolve(true);
      }
      return Promise.resolve(null);
    });
    const probe = vi.fn(async (url: string) => {
      if (url.includes("/wy.")) {
        throw new Error("failed");
      }
    });

    const playback = await resolveOnlineTrack({
      track,
      audioSources: [audioSource("first")],
      settings,
      probe,
    });

    expect(playback.candidate).toMatchObject({
      id: "hash",
      sourceId: "kg",
      channelName: "KuGou",
    });
    expect(playback.url).toBe("https://cdn.test/kg.mp3");
  });

  it("reuses a resolved URL for two minutes while probing it again", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: "https://cdn.test/song.mp3" },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const probe = vi.fn(async () => undefined);

    await resolveOnlineTrack({ track, audioSources: [audioSource("first", ["wy"])], settings, probe });
    await resolveOnlineTrack({ track, audioSources: [audioSource("first", ["wy"])], settings, probe });

    expect(invoke.mock.calls.filter(([command]) => command === "dispatch_audio_source_request"))
      .toHaveLength(1);
    expect(probe).toHaveBeenCalledTimes(2);
  });

  it("skips an explicitly failed Audio Source, channel, and quality combination", async () => {
    invoke.mockImplementation((command: string, payload: { request?: { source?: string } }) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: {
            action: "musicUrl",
            data: `https://cdn.test/${payload.request?.source}.mp3`,
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });

    const playback = await resolveOnlineTrack({
      track,
      audioSources: [audioSource("first")],
      settings,
      excludedAttempts: new Set([playbackAttemptKey("first", "netease", "320k")]),
      probe: async () => undefined,
    });

    expect(playback.candidate.channelName).toBe("KuGou");
  });
});
