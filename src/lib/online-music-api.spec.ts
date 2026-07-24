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
  type OnlineMusicSettings,
  type OnlineTrack,
} from "./online-music-api";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function audioSource(id: string, sources = ["wy", "kg"]): AudioSourceRecord {
  return {
    id,
    name: id,
    version: "1",
    description: null,
    author: null,
    homepage: null,
    path: `/sources/${id}`,
    adapter: "test",
    state: "enabled",
    enabled: true,
    permissionsReviewed: true,
    declaredCapabilities: [],
    grantedCapabilities: [],
    sources: sources.map((source) => ({
      id: source,
      name: source,
      type: "music",
      actions: ["musicUrl"],
      qualities: ["128k", "320k"],
    })),
    diagnostics: [],
    canRemove: true,
    canEnable: true,
  };
}

const settings: OnlineMusicSettings = {
  excludedChannels: [],
  channelPriority: [],
  audioSourcePriority: ["second", "first"],
  layerTimeoutSeconds: 8,
  playbackTimeoutSeconds: 20,
  preferredQuality: "320k",
  searchHistoryEnabled: true,
  downloadDirectory: null,
  filenameTemplate: "{artist} - {title}[ \\[{album}\\]]",
  downloadConcurrency: 2,
  batchNotifications: true,
};

const track: OnlineTrack = {
  key: "track",
  title: "Song",
  artist: "Artist",
  album: "Album",
  durationSeconds: 180,
  coverUrl: null,
  trackNumber: 1,
  discNumber: 1,
  candidates: [
    {
      channelId: "netease",
      pluginId: "fika.netease",
      sourceId: "wy",
      channelName: "NetEase",
      id: "1",
      title: "Song",
      artist: "Artist",
      album: "Album",
      durationSeconds: 180,
      coverUrl: null,
      trackNumber: 1,
      discNumber: 1,
      platformIds: { id: "1" },
      rawInfo: {},
      rank: 1,
    },
    {
      channelId: "kugou",
      pluginId: "fika.kugou",
      sourceId: "kg",
      channelName: "KuGou",
      id: "hash",
      title: "Song",
      artist: "Artist",
      album: "Album",
      durationSeconds: 180,
      coverUrl: null,
      trackNumber: 1,
      discNumber: 1,
      platformIds: { hash: "hash" },
      rawInfo: {},
      rank: 1,
    },
  ],
};

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

    expect(playback.channelName).toBe("KuGou");
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

    expect(playback.channelName).toBe("KuGou");
  });
});
