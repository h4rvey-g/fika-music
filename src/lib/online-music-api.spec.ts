import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioSourceRecord } from "../generated/bindings";
import {
  clearPreloadedMedia,
  orderedAudioSources,
  invalidateOnlinePlaybackCaches,
  onlineTracksMatch,
  onlinePlaylistDetailError,
  OnlinePlaybackResolutionError,
  playbackAttemptKey,
  preloadMediaUrl,
  qualityFallback,
  refreshOnlineDownloadItemCandidates,
  resolveOnlineTrack,
  selectOnlineDownloadDirectory,
  splitOnlineArtistNames,
} from "./online-music-api";
import {
  createAudioSourceRecord,
  createOnlineMusicSettings,
  createOnlineTrack,
  createOnlineTrackCandidate,
} from "../test/fixtures";

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string, protocol: string) =>
    `${protocol}://localhost/${encodeURIComponent(path)}`
  ),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc, invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function audioSource(
  id: string,
  sources = ["wy", "kg"],
  qualities: AudioSourceRecord["sources"][number]["qualities"] = ["128k", "320k"],
): AudioSourceRecord {
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
      qualities,
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
    convertFileSrc.mockClear();
    invalidateOnlinePlaybackCaches();
  });

  it("retains preloaded media until the preload slot is cleared", async () => {
    const loaded: HTMLAudioElement[] = [];
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      loaded.push(this as HTMLAudioElement);
      if (this.getAttribute("src")) {
        queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
      }
    });

    await preloadMediaUrl("https://cdn.test/next.mp3", { timeoutMs: 1_000 });

    expect(loaded[0].getAttribute("src")).toBe("https://cdn.test/next.mp3");
    clearPreloadedMedia();
    expect(loaded[0].getAttribute("src")).toBeNull();
  });

  it("matches provider snapshots by normalized title, artist set, album, and duration", () => {
    const left = createOnlineTrack({
      title: " Ｓｏｎｇ ",
      artist: "A feat. B",
      album: " Album ",
      durationSeconds: 180,
    });
    const right = createOnlineTrack({
      title: "song",
      artist: "B / A",
      album: "album",
      durationSeconds: 185,
    });

    expect(onlineTracksMatch(left, right)).toBe(true);
  });

  it("splits and deduplicates artist display names", () => {
    expect(splitOnlineArtistNames("镜予歌、陈亦洺、喧笑 / 陈亦洺")).toEqual([
      "镜予歌",
      "陈亦洺",
      "喧笑",
    ]);
  });

  it("does not match online tracks whose durations differ by more than five seconds", () => {
    const left = createOnlineTrack({ durationSeconds: 180 });
    const right = createOnlineTrack({ durationSeconds: 186 });

    expect(onlineTracksMatch(left, right)).toBe(false);
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

  it("skips qualities that an Audio Source does not declare", async () => {
    const requestedQualities: string[] = [];
    invoke.mockImplementation((command: string, payload: { request?: { quality?: string } }) => {
      if (command === "dispatch_audio_source_request") {
        requestedQualities.push(payload.request?.quality ?? "");
        return Promise.resolve({
          response: {
            action: "musicUrl",
            data: "https://cdn.test/youtube.m4a",
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });

    await resolveOnlineTrack({
      track: createOnlineTrack({
        candidates: [createOnlineTrackCandidate({ sourceId: "yt" })],
      }),
      audioSources: [audioSource("youtube", ["yt"], ["128k"])],
      settings: { ...settings, preferredQuality: "320k" },
      probe: async () => undefined,
    });

    expect(requestedQualities).toEqual(["128k"]);
  });

  it("routes Google media through the Tauri media protocol before probing", async () => {
    const resolvedUrl =
      "https://rr5---sn.example.googlevideo.com/videoplayback?itag=140&sig=test";
    invoke.mockImplementation((command: string) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: resolvedUrl },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const probe = vi.fn(async () => undefined);

    const playback = await resolveOnlineTrack({
      track: createOnlineTrack({
        candidates: [createOnlineTrackCandidate({ sourceId: "yt" })],
      }),
      audioSources: [audioSource("youtube", ["yt"], ["128k"])],
      settings: { ...settings, preferredQuality: "128k" },
      probe,
    });

    const proxiedUrl = `fika-media://localhost/${encodeURIComponent(resolvedUrl)}`;
    expect(convertFileSrc).toHaveBeenCalledWith(resolvedUrl, "fika-media");
    expect(probe).toHaveBeenCalledWith(proxiedUrl, expect.any(Object));
    expect(playback.url).toBe(proxiedUrl);
  });

  it("does not proxy a hostname that only contains the Google media suffix", async () => {
    const resolvedUrl =
      "https://googlevideo.com.attacker.test/videoplayback?itag=140&sig=test";
    invoke.mockImplementation((command: string) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: resolvedUrl },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const probe = vi.fn(async () => undefined);

    const playback = await resolveOnlineTrack({
      track: createOnlineTrack({
        candidates: [createOnlineTrackCandidate({ sourceId: "yt" })],
      }),
      audioSources: [audioSource("youtube", ["yt"], ["128k"])],
      settings: { ...settings, preferredQuality: "128k" },
      probe,
    });

    expect(convertFileSrc).not.toHaveBeenCalled();
    expect(playback.url).toBe(resolvedUrl);
  });

  it("resolves a fresh Google media URL after the first CDN probe fails", async () => {
    const resolvedUrls = [
      "https://first.googlevideo.com/videoplayback?itag=140&sig=first",
      "https://second.googlevideo.com/videoplayback?itag=140&sig=second",
    ];
    let dispatchCount = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "dispatch_audio_source_request") {
        const data = resolvedUrls[Math.min(dispatchCount, resolvedUrls.length - 1)];
        dispatchCount += 1;
        return Promise.resolve({
          response: { action: "musicUrl", data },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const probe = vi.fn((url: string) =>
      url.includes("first.googlevideo.com")
        ? Promise.reject(new Error("CDN connection failed"))
        : Promise.resolve()
    );

    const playback = await resolveOnlineTrack({
      track: createOnlineTrack({
        candidates: [createOnlineTrackCandidate({ sourceId: "yt" })],
      }),
      audioSources: [audioSource("youtube", ["yt"], ["128k"])],
      settings: { ...settings, preferredQuality: "128k" },
      probe,
    });

    expect(dispatchCount).toBe(2);
    expect(playback.url).toContain("second.googlevideo.com");
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

  it("lets the bundled yt-dlp source use the remaining playback budget", async () => {
    vi.useFakeTimers();
    try {
      invoke.mockImplementation((command: string) => {
        if (command === "dispatch_audio_source_request") {
          return new Promise((resolve) => {
            setTimeout(() => resolve({
              response: {
                action: "musicUrl",
                data: "https://cdn.test/youtube.m4a",
              },
              diagnostics: [],
            }), 12);
          });
        }
        return Promise.resolve(true);
      });
      const source = {
        ...audioSource("youtube", ["yt"], ["128k"]),
        adapter: "builtin:youtube-music-playback",
      };
      const resolving = resolveOnlineTrack({
        track: createOnlineTrack({
          candidates: [createOnlineTrackCandidate({ sourceId: "yt" })],
        }),
        audioSources: [source],
        settings: {
          ...settings,
          audioSourceSelectionMode: "automatic",
          layerTimeoutSeconds: 0.008,
          playbackTimeoutSeconds: 0.02,
          preferredQuality: "128k",
        },
        probe: async () => undefined,
      });
      const outcome = resolving.then(
        (playback) => ({ playback }),
        (error: unknown) => ({ error }),
      );

      await vi.advanceTimersByTimeAsync(20);

      expect(await outcome).toMatchObject({
        playback: { audioSourceId: "youtube", quality: "128k" },
      });
    } finally {
      vi.useRealTimers();
    }
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

  it("starts one delayed fallback source when the automatic primary stalls", async () => {
    vi.useFakeTimers();
    invoke.mockImplementation((command: string, payload: { audioSourceId?: string }) => {
      if (command === "dispatch_audio_source_request") {
        if (payload.audioSourceId === "first") return new Promise(() => undefined);
        return Promise.resolve({
          response: { action: "musicUrl", data: "https://cdn.test/second.mp3" },
          diagnostics: [],
        });
      }
      return Promise.resolve(true);
    });
    const resolving = resolveOnlineTrack({
      track,
      audioSources: [audioSource("first", ["wy"]), audioSource("second", ["wy"])],
      settings: {
        ...settings,
        audioSourceSelectionMode: "automatic",
        preferredQuality: "320k",
      },
      probe: async () => undefined,
    });

    await vi.advanceTimersByTimeAsync(719);
    expect(invoke.mock.calls.filter(([command]) => command === "dispatch_audio_source_request"))
      .toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await expect(resolving).resolves.toMatchObject({ audioSourceId: "second" });
    expect(invoke.mock.calls.filter(([command]) => command === "dispatch_audio_source_request"))
      .toHaveLength(2);
    vi.useRealTimers();
  });

  it("reports every attempted Audio Source and its failure reason", async () => {
    invoke.mockImplementation((command: string, payload: { audioSourceId?: string }) => {
      if (command === "dispatch_audio_source_request") {
        return Promise.reject(new Error(`${payload.audioSourceId} rejected the request`));
      }
      return Promise.resolve(true);
    });

    const error = await resolveOnlineTrack({
      track,
      audioSources: [audioSource("first", ["wy"]), audioSource("second", ["wy"])],
      settings: {
        ...settings,
        audioSourceSelectionMode: "automatic",
      },
      probe: async () => undefined,
    }).catch((failure: unknown) => failure);

    expect(error).toBeInstanceOf(OnlinePlaybackResolutionError);
    expect((error as OnlinePlaybackResolutionError).failures).toEqual(expect.arrayContaining([
      expect.objectContaining({
        audioSourceId: "second",
        audioSourceName: "second",
        reason: expect.stringContaining("second rejected the request"),
      }),
      expect.objectContaining({
        audioSourceId: "first",
        audioSourceName: "first",
        reason: expect.stringContaining("first rejected the request"),
      }),
    ]));
  });
});
