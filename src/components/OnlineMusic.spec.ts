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

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

const eventListeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriMocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauriMocks.listen }));

const settings = {
  excludedChannels: [],
  channelPriority: [],
  audioSourcePriority: [],
  layerTimeoutSeconds: 8,
  playbackTimeoutSeconds: 20,
  preferredQuality: "320k" as const,
  searchHistoryEnabled: true,
  downloadDirectory: null,
  filenameTemplate: "{artist} - {title}[ \\[{album}\\]]",
  downloadConcurrency: 2,
  batchNotifications: true,
};

function track(index: number): OnlineTrack {
  return {
    key: `song-${index}`,
    title: `Song ${index}`,
    artist: "Artist",
    album: "Album",
    durationSeconds: 180,
    coverUrl: null,
    trackNumber: index,
    discNumber: 1,
    candidates: [
      {
        channelId: "fika.netease:wy",
        pluginId: "fika.netease",
        sourceId: "wy",
        channelName: "NetEase",
        id: String(index),
        title: `Song ${index}`,
        artist: "Artist",
        album: "Album",
        durationSeconds: 180,
        coverUrl: null,
        trackNumber: index,
        discNumber: 1,
        platformIds: { id: index },
        rawInfo: {},
        rank: index,
      },
    ],
  };
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

function mountOnlineMusic() {
  return mount(OnlineMusic, {
    props: {
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
