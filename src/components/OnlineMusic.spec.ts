import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import OnlineMusic from "./OnlineMusic.vue";
import type {
  OnlineDownloadTask,
  OnlinePlaylist,
  OnlineSearchSection,
  OnlineSearchSectionEvent,
  OnlineTrack,
} from "../lib/online-music-api";
import {
  createOnlineMusicSettings,
  createOnlineTrack,
  createOnlineTrackCandidate,
} from "../test/fixtures";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

const eventListeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriMocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauriMocks.listen }));

const settings = createOnlineMusicSettings();

function track(index: number, coverUrl: string | null = null): OnlineTrack {
  const title = `Song ${index}`;
  return createOnlineTrack({
    key: `song-${index}`,
    title,
    coverUrl,
    trackNumber: index,
    candidates: [
      createOnlineTrackCandidate({
        channelId: "fika.netease:wy",
        id: String(index),
        title,
        trackNumber: index,
        platformIds: { id: index },
        rank: index,
      }),
    ],
  });
}

const playlist: OnlinePlaylist = {
  key: "playlist-key",
  channelId: "fika.netease:wy",
  pluginId: "fika.netease",
  sourceId: "wy",
  channelName: "NetEase",
  id: "playlist-1",
  name: "Private Mix",
  description: null,
  coverUrl: null,
  trackCount: 12,
  ownerName: "Listener",
  platformIds: { id: "playlist-1" },
  rawInfo: {},
  rank: 1,
};

function failedDownloadTask(): OnlineDownloadTask {
  return {
    taskId: "task-1",
    kind: "track",
    title: "Song 1",
    state: "completedWithErrors",
    destination: "/music",
    selectedAudioSourceId: null,
    totalItems: 1,
    completedItems: 0,
    skippedItems: 0,
    failedItems: 1,
    createdAt: 1,
    updatedAt: 1,
    items: [
      {
        itemId: "item-1",
        position: 0,
        state: "failed",
        track: track(1),
        targetPath: null,
        message: "source failed",
        bytesDownloaded: 0,
        totalBytes: null,
      },
    ],
  };
}

function mountOnlineMusic(isActive = true) {
  return mount(OnlineMusic, {
    props: {
      isActive,
      audioSources: [],
      selectedAudioSourceId: "",
      activeOnlineTrackKey: null,
      resolvingOnlineTrackKey: null,
      isPlaying: false,
      localMusicFolder: "/music",
    },
  });
}

async function search(
  wrapper: ReturnType<typeof mountOnlineMusic>,
  keyword: string,
  section: OnlineSearchSection,
  items: OnlineTrack[] | OnlinePlaylist[],
  hasMore = false,
) {
  await wrapper.get('input[aria-label="Search Online Music"]').setValue(keyword);
  await wrapper.get('form[role="search"]').trigger("submit");
  await flushPromises();
  const listener = eventListeners.get("online-music:search-section");
  const event: OnlineSearchSectionEvent = {
    searchId: "search-1",
    result: {
      section,
      data: { section, items } as OnlineSearchSectionEvent["result"]["data"],
      failures: [],
      supportedChannels: 1,
      completedChannels: 1,
      hasMore,
    },
  };
  listener?.({ payload: event });
  await nextTick();
}

describe("Online Music workspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventListeners.clear();
    tauriMocks.listen.mockImplementation(
      async (event: string, listener: (event: { payload: unknown }) => void) => {
        eventListeners.set(event, listener);
        return vi.fn();
      },
    );
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "start_online_music_search") return Promise.resolve("search-1");
      return Promise.resolve(null);
    });
  });

  it("shows the three personalized entrances on the Online Music home", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();

    expect(
      wrapper.findAll("[data-online-home] button[aria-label]").map((button) =>
        button.attributes("aria-label")
      ),
    ).toEqual(["每日推荐", "私人漫游", "私人雷达"]);
    wrapper.unmount();
  });

  it("preloads recommendation covers only while Online Music is active", async () => {
    const wrapper = mountOnlineMusic(false);
    await flushPromises();

    expect(
      tauriMocks.invoke.mock.calls.filter(([command]) => command === "online_music_recommendations"),
    ).toHaveLength(0);

    await wrapper.setProps({ isActive: true });
    await flushPromises();
    expect(
      tauriMocks.invoke.mock.calls.filter(([command]) => command === "online_music_recommendations"),
    ).toHaveLength(3);
    wrapper.unmount();
  });

  it("fills each recommendation entrance with its first track cover", async () => {
    const covers = {
      daily: "https://cdn.test/daily.jpg",
      roaming: "https://cdn.test/roaming.jpg",
      radar: "https://cdn.test/radar.jpg",
    } as const;
    tauriMocks.invoke.mockImplementation((command: string, args?: { kind?: keyof typeof covers }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_recommendations" && args?.kind) {
        return Promise.resolve({
          kind: args.kind,
          items: [track(1, covers[args.kind])],
          failures: [],
          supportedChannels: args.kind === "daily" ? 2 : 1,
          completedChannels: args.kind === "daily" ? 2 : 1,
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    const cards = wrapper.findAll("[data-online-home] button[aria-label]");
    expect(cards.map((card) => card.get("img").attributes("src"))).toEqual([
      covers.daily,
      covers.roaming,
      covers.radar,
    ]);

    await wrapper.get('button[aria-label="私人漫游"]').trigger("click");
    await flushPromises();
    expect(
      tauriMocks.invoke.mock.calls.filter(
        ([command, args]) => command === "online_music_recommendations" && args.kind === "roaming",
      ),
    ).toHaveLength(1);
    wrapper.unmount();
  });

  it("cancels an in-flight recommendation when returning home", async () => {
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_recommendations") return new Promise(() => undefined);
      if (command === "cancel_source_request") return Promise.resolve(true);
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic(false);
    await flushPromises();

    await wrapper.get('button[aria-label="每日推荐"]').trigger("click");
    const request = tauriMocks.invoke.mock.calls.find(
      ([command]) => command === "online_music_recommendations",
    );
    (wrapper.vm as unknown as { showHome: () => void }).showHome();
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("cancel_source_request", {
      requestId: request?.[1].requestId,
    });
    wrapper.unmount();
  });

  it("loads the aggregated daily recommendations into the shared track table", async () => {
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_recommendations") {
        return Promise.resolve({
          kind: "daily",
          items: [track(1)],
          failures: [],
          supportedChannels: 2,
          completedChannels: 2,
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    await wrapper.get('button[aria-label="每日推荐"]').trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("online_music_recommendations", {
      kind: "daily",
      requestId: expect.stringMatching(/^online-recommendation-daily-/),
    });
    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("appends unique private roaming tracks from a control below the current batch", async () => {
    let roamingRequests = 0;
    tauriMocks.invoke.mockImplementation((command: string, args?: { kind?: string }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_recommendations") {
        if (args?.kind === "roaming") {
          roamingRequests += 1;
          const items = roamingRequests === 1
            ? [track(1), track(2), track(3)]
            : [track(3), track(4), track(4), track(5)];
          return Promise.resolve({
            kind: "roaming",
            items,
            failures: [],
            supportedChannels: 1,
            completedChannels: 1,
          });
        }
        return Promise.resolve({
          kind: args?.kind ?? "daily",
          items: [track(10)],
          failures: [],
          supportedChannels: 1,
          completedChannels: 1,
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    await wrapper.get('button[aria-label="私人漫游"]').trigger("click");
    await flushPromises();

    expect(
      wrapper.find(
        '[data-online-recommendation] > div:first-child button[aria-label="Load next private roaming batch"]',
      ).exists(),
    ).toBe(false);
    const loadNext = wrapper.get('[data-private-roaming-next]');
    expect(loadNext.text()).toContain("Load next songs");
    await loadNext.trigger("click");
    await flushPromises();

    expect(
      wrapper.findAll('button[aria-label^="Play Song "]').map((button) =>
        button.attributes("aria-label")
      ),
    ).toEqual([
      "Play Song 1",
      "Play Song 2",
      "Play Song 3",
      "Play Song 4",
      "Play Song 5",
    ]);
    expect(roamingRequests).toBe(2);
    wrapper.unmount();
  });

  it("keeps the private roaming playback queue appendable by later batches", async () => {
    let roamingRequests = 0;
    tauriMocks.invoke.mockImplementation((command: string, args?: { kind?: string }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "list_online_music_channels") {
        return Promise.resolve([{ id: "fika.netease:wy" }]);
      }
      if (command === "online_music_recommendations") {
        if (args?.kind === "roaming") roamingRequests += 1;
        return Promise.resolve({
          kind: args?.kind ?? "daily",
          items: args?.kind === "roaming" && roamingRequests > 1
            ? [track(4), track(5), track(6)]
            : [track(1), track(2), track(3)],
          failures: [],
          supportedChannels: 1,
          completedChannels: 1,
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    await wrapper.get('button[aria-label="私人漫游"]').trigger("click");
    await flushPromises();
    await wrapper.get('button[aria-label="Play Song 3"]').trigger("click");
    await flushPromises();

    const requests = wrapper.emitted("playRequest") ?? [];
    const request = requests[requests.length - 1];
    expect(request?.[3]).toBe(true);
    expect(request?.[4]).toEqual(expect.any(Function));
    const queue = request?.[1] as OnlineTrack[];
    const loadNext = request?.[4] as () => Promise<OnlineTrack[]>;
    await loadNext();

    expect(queue.map((item) => item.title)).toEqual([
      "Song 1",
      "Song 2",
      "Song 3",
      "Song 4",
      "Song 5",
      "Song 6",
    ]);
    wrapper.unmount();
  });

  it("returns a submitted search to the Online Music home through its exposed action", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", [track(1)]);

    (wrapper.vm as unknown as { showHome: () => void }).showHome();
    await nextTick();

    expect(wrapper.find("[data-online-home]").exists()).toBe(true);
    expect(wrapper.get<HTMLInputElement>('input[aria-label="Search Online Music"]').element.value)
      .toBe("");
    wrapper.unmount();
  });

  it("keeps a category visible while expanding its complete result page", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", Array.from({ length: 5 }, (_, index) => track(index + 1)), true);
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "online_music_search_page") {
        return Promise.resolve({
          section: "songs",
          data: { section: "songs", items: Array.from({ length: 6 }, (_, index) => track(index + 1)) },
          failures: [],
          supportedChannels: 1,
          completedChannels: 1,
          hasMore: false,
        });
      }
      return Promise.resolve(null);
    });

    const moreSongs = wrapper
      .findAll("button")
      .find((button) => button.text().includes("More Songs"));
    expect(moreSongs).toBeDefined();
    await moreSongs?.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("All Songs");
    expect(wrapper.text()).toContain("Song 6");
    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("renders a typed login-required playlist error with channel and retry actions", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Mix", "playlists", [playlist]);
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "online_music_playlist_tracks") {
        return Promise.reject(JSON.stringify({
          code: "credential-expired",
          message: "session expired",
          pluginId: "fika.netease",
          channelName: "NetEase",
        }));
      }
      return Promise.resolve(null);
    });

    await wrapper.get("li.list-row").trigger("click");
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain(
      "NetEase requires login to read this playlist.",
    );
    expect(wrapper.get('[role="alert"]').text()).toContain("Retry");
    await wrapper.get('[role="alert"] button.btn-sm').trigger("click");
    expect(wrapper.emitted("openPlugin")?.[0]).toEqual(["fika.netease"]);
    wrapper.unmount();
  });

  it("leaves search active and creates no task when the first download picker is cancelled", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", [track(1)]);
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "select_online_download_directory") return Promise.resolve(null);
      return Promise.resolve(null);
    });

    await wrapper.get('button[aria-label="Download Song 1"]').trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("select_online_download_directory", {
      initialDirectory: "/music",
    });
    expect(
      tauriMocks.invoke.mock.calls.some(([command]) => command === "create_online_download_task"),
    ).toBe(false);
    expect(wrapper.findAll('[role="tab"]')[0].classes()).toContain("tab-active");
    wrapper.unmount();
  });

  it("refreshes a failed item snapshot before retrying it", async () => {
    const failed = failedDownloadTask();
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([failed]);
      if (command === "refresh_online_download_item_candidates") return Promise.resolve(failed);
      if (command === "retry_online_download_item") {
        return Promise.resolve({
          ...failed,
          state: "running",
          failedItems: 0,
          items: [{ ...failed.items[0], state: "resolving", message: null }],
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await wrapper.findAll('[role="tab"]')[1].trigger("click");

    expect(wrapper.find('button[aria-label="Retry Song 1"]').exists()).toBe(true);
    await wrapper.get('button[aria-label="Refresh candidates for Song 1"]').trigger("click");
    await flushPromises();

    const commands = tauriMocks.invoke.mock.calls.map(([command]) => command);
    expect(commands.indexOf("refresh_online_download_item_candidates")).toBeLessThan(
      commands.indexOf("retry_online_download_item"),
    );
    wrapper.unmount();
  });
});
