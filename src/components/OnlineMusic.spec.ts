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
  accountRef: null,
  id: "playlist-1",
  name: "Private Mix",
  description: null,
  coverUrl: null,
  trackCount: 12,
  ownerName: "Listener",
  canMutate: false,
  isFavorite: false,
  platformIds: { id: "playlist-1" },
  rawInfo: {},
  rank: 1,
};

const libraryPlaylist: OnlinePlaylist = {
  ...playlist,
  key: "fika.netease:wy:netease-account:1:playlist-1",
  accountRef: "netease-account:1",
  coverUrl: "https://cdn.test/private-mix.jpg",
  canMutate: true,
  isFavorite: true,
};

const kugouLibraryPlaylist: OnlinePlaylist = {
  ...playlist,
  key: "fika.kugou:kg:kugou-account:1:playlist-2",
  channelId: "fika.kugou:kg",
  pluginId: "fika.kugou",
  sourceId: "kg",
  channelName: "KuGou",
  accountRef: "kugou-account:1",
  id: "playlist-2",
  name: "KuGou Favorites",
  coverUrl: "https://cdn.test/kugou-favorites.jpg",
  trackCount: 24,
  canMutate: true,
  isFavorite: true,
  platformIds: { id: "playlist-2" },
  rank: 2,
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
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [],
          failures: [],
          supportedChannels: 2,
          completedChannels: 2,
        });
      }
      return Promise.resolve(null);
    });
  });

  it("shows the three personalized entrances on the Online Music home", async () => {
    const wrapper = mountOnlineMusic();
    await flushPromises();

    expect(
      wrapper.findAll("[data-online-recommendation-entry]").map((button) =>
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
    expect(
      tauriMocks.invoke.mock.calls.filter(([command]) => command === "online_music_playlists"),
    ).toHaveLength(1);
    wrapper.unmount();
  });

  it("renders account playlists on For You and opens authenticated playlist details", async () => {
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [kugouLibraryPlaylist, libraryPlaylist],
          failures: [],
          supportedChannels: 2,
          completedChannels: 2,
        });
      }
      if (command === "online_music_playlist_tracks") {
        return Promise.resolve({ items: [track(1)], hasMore: false, total: 1 });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    const providerSections = wrapper.findAll("[data-online-playlist-provider]");
    expect(providerSections.map((section) =>
      section.attributes("data-online-playlist-provider")
    )).toEqual(["fika.netease", "fika.kugou"]);
    expect(providerSections[0].text()).toContain("Private Mix");
    expect(providerSections[0].text()).not.toContain("KuGou Favorites");
    expect(providerSections[1].text()).toContain("KuGou Favorites");
    expect(providerSections[1].text()).not.toContain("Private Mix");

    const card = wrapper.get('button[aria-label="Open playlist Private Mix"]');
    expect(card.text()).toContain("Listener");
    expect(card.text()).not.toContain("NetEase");
    expect(card.find(".badge").exists()).toBe(false);
    expect(card.text()).toContain("12 tracks");
    await card.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("online_music_playlist_tracks", {
      playlist: libraryPlaylist,
      page: 1,
      pageSize: 100,
      requestId: expect.stringMatching(/^online-detail-/),
    });
    expect(wrapper.find('[data-online-track-key="song-1"]').exists()).toBe(true);
    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("keeps playlists visible when one provider fails and offers its channel action", async () => {
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [libraryPlaylist],
          failures: [{
            channelId: "fika.kugou:kg",
            channelName: "KuGou Music",
            message: "Connect an active KuGou Music account to load playlists.",
          }],
          supportedChannels: 2,
          completedChannels: 1,
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();

    expect(wrapper.find('button[aria-label="Open playlist Private Mix"]').exists()).toBe(true);
    const status = wrapper.get('[data-online-playlists] [role="status"]');
    expect(status.text()).toContain("KuGou Music unavailable");
    await status.get("button").trigger("click");
    expect(wrapper.emitted("openPlugin")?.[0]).toEqual(["fika.kugou"]);
    wrapper.unmount();
  });

  it("adds a song to each matching provider favorite playlist", async () => {
    const matchedTrack = {
      ...track(1),
      candidates: [
        ...track(1).candidates,
        createOnlineTrackCandidate({
          channelId: "fika.kugou:kg",
          pluginId: "fika.kugou",
          sourceId: "kg",
          channelName: "KuGou",
          id: "4D766DEC7A90A011D730ED939D158131",
          title: "Song 1",
          platformIds: { albumId: 12, mixSongId: 34 },
          rank: 2,
        }),
      ],
    };
    tauriMocks.invoke.mockImplementation((command: string, args?: {
      request?: { action: string; playlistId: string; track: { id: string } };
    }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "start_online_music_search") return Promise.resolve("search-1");
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [libraryPlaylist, kugouLibraryPlaylist],
          failures: [],
          supportedChannels: 2,
          completedChannels: 2,
        });
      }
      if (command === "dispatch_plugin_request" && args?.request?.action === "playlistAddTrack") {
        return Promise.resolve({
          response: {
            action: "playlistAddTrack",
            data: {
              auditId: 0,
              operation: "add",
              playlistId: args.request.playlistId,
              trackId: args.request.track.id,
              occurredAt: 1,
            },
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", [matchedTrack]);

    await wrapper.get('button[aria-label="Add Song 1 to My Favorite Music"]').trigger("click");
    await flushPromises();

    const mutations = tauriMocks.invoke.mock.calls
      .filter(([command]) => command === "dispatch_plugin_request")
      .map(([, args]) => args);
    expect(mutations).toEqual(expect.arrayContaining([
      expect.objectContaining({
        pluginId: "fika.netease",
        request: expect.objectContaining({
          source: "wy",
          accountRef: "netease-account:1",
          playlistId: "playlist-1",
          track: { id: "1", source: "wy" },
        }),
      }),
      expect.objectContaining({
        pluginId: "fika.kugou",
        request: expect.objectContaining({
          source: "kg",
          accountRef: "kugou-account:1",
          playlistId: "playlist-2",
          track: {
            id: "4D766DEC7A90A011D730ED939D158131",
            source: "kg",
            title: "Song 1",
            platformIds: { albumId: 12, mixSongId: 34 },
          },
        }),
      }),
    ]));
    expect(wrapper.text()).toContain("Added to 2 favorite playlists.");
    const favoriteButton = wrapper.get('button[aria-label="Add Song 1 to My Favorite Music"]');
    expect(favoriteButton.get("svg").attributes("fill")).toBe("currentColor");
    expect(favoriteButton.get("svg").classes()).toContain("text-error");
    wrapper.unmount();
  });

  it("limits the playlist picker to playlists backed by the selected song's providers", async () => {
    tauriMocks.invoke.mockImplementation((command: string, args?: {
      request?: { action: string; playlistId: string; track: { id: string } };
    }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "start_online_music_search") return Promise.resolve("search-1");
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [libraryPlaylist, kugouLibraryPlaylist],
          failures: [],
          supportedChannels: 2,
          completedChannels: 2,
        });
      }
      if (command === "dispatch_plugin_request" && args?.request?.action === "playlistAddTrack") {
        return Promise.resolve({
          response: {
            action: "playlistAddTrack",
            data: {
              auditId: 0,
              operation: "add",
              playlistId: args.request.playlistId,
              trackId: args.request.track.id,
              occurredAt: 1,
            },
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", [track(1)]);

    await wrapper.get('button[aria-label="Add Song 1 to a Playlist"]').trigger("click");
    await flushPromises();

    const picker = wrapper.get("[data-online-playlist-picker]");
    expect(picker.text()).toContain("Private Mix");
    expect(picker.text()).not.toContain("KuGou Favorites");
    await picker.trigger("submit");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("dispatch_plugin_request", {
      pluginId: "fika.netease",
      request: {
        action: "playlistAddTrack",
        source: "wy",
        accountRef: "netease-account:1",
        playlistId: "playlist-1",
        track: { id: "1", source: "wy" },
      },
      requestId: undefined,
    });
    wrapper.unmount();
  });

  it("adds a multi-selection to the same writable playlist", async () => {
    tauriMocks.invoke.mockImplementation((command: string, args?: {
      request?: { action: string; playlistId: string; track: { id: string } };
    }) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "start_online_music_search") return Promise.resolve("search-1");
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [libraryPlaylist],
          failures: [],
          supportedChannels: 1,
          completedChannels: 1,
        });
      }
      if (command === "dispatch_plugin_request" && args?.request?.action === "playlistAddTrack") {
        return Promise.resolve({
          response: {
            action: "playlistAddTrack",
            data: {
              auditId: 0,
              operation: "add",
              playlistId: args.request.playlistId,
              trackId: args.request.track.id,
              occurredAt: 1,
            },
          },
          diagnostics: [],
        });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    const tracks = [track(1), track(2)];
    await search(wrapper, "Song", "songs", tracks);
    const rows = wrapper.findAll("tbody tr");

    await rows[0].trigger("click");
    await rows[1].trigger("click", { ctrlKey: true });
    await rows[1].trigger("contextmenu", { clientX: 100, clientY: 100 });
    const addToPlaylist = wrapper
      .findAll("[data-online-track-menu] button")
      .find((button) => button.text().includes("Add to Playlist"));
    await addToPlaylist?.trigger("click");
    await flushPromises();

    const picker = wrapper.get("[data-online-playlist-picker]");
    expect(picker.text()).toContain("2 selected tracks");
    await picker.trigger("submit");
    await flushPromises();

    const addedTrackIds = tauriMocks.invoke.mock.calls
      .filter(([command, args]) =>
        command === "dispatch_plugin_request"
        && args?.request?.action === "playlistAddTrack"
      )
      .map(([, args]) => args.request.track.id);
    expect(addedTrackIds).toEqual(["1", "2"]);
    expect(wrapper.text()).toContain("Added 2 of 2 tracks to Private Mix.");
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

    const cards = wrapper.findAll("[data-online-recommendation-entry]");
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
    expect(wrapper.find('[data-online-track-key="song-1"]').exists()).toBe(true);
    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(false);
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
      wrapper.findAll("[data-online-track-key]").map((row) =>
        row.attributes("data-online-track-key")
      ),
    ).toEqual(["song-1", "song-2", "song-3", "song-4", "song-5"]);
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
    await wrapper.get('[data-online-track-key="song-3"]').trigger("dblclick");
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
    expect(wrapper.find('[data-online-track-key="song-1"]').exists()).toBe(true);
    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(false);
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

  it("creates one download task for a multi-selection", async () => {
    const tracks = [track(1), track(2)];
    const task: OnlineDownloadTask = {
      taskId: "selection-task",
      kind: "selection",
      title: "2 selected tracks",
      state: "queued",
      destination: "/downloads",
      selectedAudioSourceId: null,
      totalItems: 2,
      completedItems: 0,
      skippedItems: 0,
      failedItems: 0,
      createdAt: 1,
      updatedAt: 1,
      items: [],
    };
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") {
        return Promise.resolve({ ...settings, downloadDirectory: "/downloads" });
      }
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "start_online_music_search") return Promise.resolve("search-1");
      if (command === "online_music_playlists") {
        return Promise.resolve({
          items: [],
          failures: [],
          supportedChannels: 0,
          completedChannels: 0,
        });
      }
      if (command === "create_online_download_task") return Promise.resolve(task);
      if (command === "start_online_download_task") {
        return Promise.resolve({ ...task, state: "running", updatedAt: 2 });
      }
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await search(wrapper, "Song", "songs", tracks);
    const rows = wrapper.findAll("tbody tr");

    await rows[0].trigger("click");
    await rows[1].trigger("click", { ctrlKey: true });
    await rows[1].trigger("contextmenu", { clientX: 100, clientY: 100 });
    const download = wrapper
      .findAll("[data-online-track-menu] button")
      .find((button) => button.text().includes("Download"));
    await download?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_online_download_task", {
      kind: "selection",
      title: "2 selected tracks",
      tracks,
      selectedAudioSourceId: "",
      localMusicFolder: "/music",
    });
    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_online_download_task", {
      taskId: "selection-task",
    });
    expect(wrapper.findAll('[role="tab"]')[1].classes()).toContain("tab-active");
    wrapper.unmount();
  });

  it("includes the active file bytes in task progress updates", async () => {
    const activeTask: OnlineDownloadTask = {
      taskId: "progress-task",
      kind: "selection",
      title: "2 selected tracks",
      state: "running",
      destination: "/downloads",
      selectedAudioSourceId: null,
      totalItems: 2,
      completedItems: 0,
      skippedItems: 0,
      failedItems: 0,
      createdAt: 1,
      updatedAt: 1,
      items: [
        {
          itemId: "active-item",
          position: 0,
          state: "resolving",
          track: track(1),
          targetPath: null,
          message: null,
          bytesDownloaded: 0,
          totalBytes: null,
        },
        {
          itemId: "queued-item",
          position: 1,
          state: "queued",
          track: track(2),
          targetPath: null,
          message: null,
          bytesDownloaded: 0,
          totalBytes: null,
        },
      ],
    };
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_online_music_settings") return Promise.resolve(settings);
      if (command === "list_online_download_tasks") return Promise.resolve([activeTask]);
      return Promise.resolve(null);
    });
    const wrapper = mountOnlineMusic();
    await flushPromises();
    await wrapper.findAll('[role="tab"]')[1].trigger("click");

    eventListeners.get("online-music:download-progress")?.({
      payload: {
        taskId: "progress-task",
        itemId: "active-item",
        state: "downloading",
        bytesDownloaded: 5 * 1024 * 1024,
        totalBytes: 10 * 1024 * 1024,
      },
    });
    await nextTick();

    const progress = wrapper.get('progress[aria-label="2 selected tracks progress"]');
    expect(progress.attributes("value")).toBe("25");
    expect(wrapper.text()).toContain("5 MB / 10 MB");

    eventListeners.get("online-music:download-task")?.({
      payload: {
        ...activeTask,
        state: "completed",
        completedItems: 2,
        items: activeTask.items.map((item) => ({
          ...item,
          state: "completed",
          bytesDownloaded: 10 * 1024 * 1024,
          totalBytes: 10 * 1024 * 1024,
        })),
      },
    });
    eventListeners.get("online-music:download-progress")?.({
      payload: {
        taskId: "progress-task",
        itemId: "active-item",
        state: "downloading",
        bytesDownloaded: 6 * 1024 * 1024,
        totalBytes: 10 * 1024 * 1024,
      },
    });
    await nextTick();

    expect(wrapper.get('progress[aria-label="2 selected tracks progress"]').attributes("value"))
      .toBe("100");
    expect(wrapper.text()).toContain("Complete");
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
