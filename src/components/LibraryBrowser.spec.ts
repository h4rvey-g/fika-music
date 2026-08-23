import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import LibraryBrowser from "./LibraryBrowser.vue";
import type {
  AlbumArtTaskStatus,
  LibraryAlbumGroup,
  LibraryQueryPage,
  LibraryViewItem,
  MetadataLookupTaskStatus,
} from "../generated/bindings";
import { createLocalTrack, createScanStatus } from "../test/fixtures";
import { virtualizerMocks } from "../test/vue-virtual.mock";
import { COLLECTION_DRAG_TYPE } from "../lib/collection-api";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  revealItemInDir: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: tauriMocks.revealItemInDir,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
}));

vi.mock("@tanstack/vue-virtual", () => import("../test/vue-virtual.mock"));

const idleScanStatus = createScanStatus({
  folderPath: "/music",
  discoveredFiles: 2,
  scannedFiles: 2,
  indexedTracks: 2,
});

const tracks = [
  createLocalTrack({
    filePath: "/music/first.flac",
    fileName: "first.flac",
    artist: "周杰伦",
    album: "叶惠美",
    albumArtist: "周杰伦",
    year: 2003,
    codec: "FLAC",
    bitrateKbps: 900,
    playCount: 4,
  }),
  createLocalTrack({
    id: 2,
    filePath: "/music/second.mp3",
    fileName: "second.mp3",
    title: "Second",
    genre: "Rock",
    sampleRateHz: 48000,
    durationSeconds: 181,
    trackNumber: 2,
    fileSizeBytes: 2048,
  }),
];
let queryNeedsReindex = false;

function queryPage(): LibraryQueryPage {
  const groups: LibraryAlbumGroup[] = tracks.map((track, index) => ({
    id: `album-${index + 1}`,
    title: track.album,
    albumArtist: track.albumArtist,
    year: track.year,
    matchedTracks: 1,
    totalTracks: 1,
    totalDurationSeconds: track.durationSeconds ?? 0,
    startIndex: index,
    endIndex: index,
    isUngrouped: false,
  }));
  const items: LibraryViewItem[] = groups.flatMap((group, index) => {
    const offset = index * 3;
    return [
      { index: offset, kind: "albumHeader", group, track: null, trackIndex: null },
      { index: offset + 1, kind: "albumContinuation", group, track: null, trackIndex: null },
      { index: offset + 2, kind: "track", group: null, track: tracks[index], trackIndex: index },
    ];
  });
  return {
    snapshotId: "snapshot-1",
    total: tracks.length,
    libraryTotal: tracks.length,
    totalDurationSeconds: 361,
    needsReindex: queryNeedsReindex,
    groupTotal: groups.length,
    virtualTotal: items.length,
    offset: 0,
    items,
  };
}

const idleAlbumArtTask: AlbumArtTaskStatus = {
  state: "idle",
  total: 0,
  processed: 0,
  embedded: 0,
  downloaded: 0,
  notFound: 0,
  needsReview: 0,
  failed: 0,
  currentAlbum: null,
};

const idleMetadataTask: MetadataLookupTaskStatus = {
  state: "idle",
  total: 0,
  processed: 0,
  updated: 0,
  unchanged: 0,
  failed: 0,
  currentTrack: null,
  results: [],
};

function mountLibrary(activeTrackId: number | null = null, isPlaying = false) {
  return mount(LibraryBrowser, {
    props: {
      activeTrackId,
      isPlaying,
      scanStatus: idleScanStatus,
      scanMessage: null,
    },
  });
}

describe("LibraryBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorage.clear();
    queryNeedsReindex = false;
    tauriMocks.listen.mockResolvedValue(vi.fn());
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "query_local_library") {
        return Promise.resolve(queryPage());
      }
      if (command === "local_library_track_position") {
        const trackId = args?.trackId;
        return Promise.resolve(trackId === 1 ? 2 : trackId === 2 ? 5 : null);
      }
      if (command === "get_album_art_settings") {
        return Promise.resolve({ networkEnabled: true });
      }
      if (command === "get_album_art_task_status") {
        return Promise.resolve(idleAlbumArtTask);
      }
      if (command === "get_metadata_lookup_task_status") {
        return Promise.resolve(idleMetadataTask);
      }
      if (command === "resolve_local_album_cover") {
        return Promise.resolve({
          groupId: args?.groupId,
          status: "embedded",
          dataUrl: "data:image/jpeg;base64,AA==",
          candidates: [],
          message: null,
          writtenTracks: 0,
          failedTracks: 0,
        });
      }
      if (command === "set_album_art_network_enabled") {
        return Promise.resolve({ networkEnabled: true });
      }
      if (command === "start_local_metadata_lookup") {
        return Promise.resolve({ ...idleMetadataTask, state: "running", total: 2 });
      }
      if (command === "create_local_library_playback_queue") {
        return Promise.resolve({
          queueId: "queue-1",
          total: 2,
          currentIndex: 0,
          track: tracks[0],
        });
      }
      if (command === "set_local_track_rating") {
        return Promise.resolve(args?.rating);
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("queries all tracks and renders the default columns", async () => {
    const wrapper = mountLibrary();
    await flushPromises();

    const queryCall = tauriMocks.invoke.mock.calls.find(([command]) => command === "query_local_library");
    const headers = wrapper.findAll('[role="columnheader"]').map((header) => header.attributes("aria-label"));

    expect(queryCall?.[1]).toEqual({
      request: {
        search: "",
        searchFields: ["title", "artist", "album"],
        sortField: "relevance",
        sortDirection: "descending",
        collapsedGroupIds: [],
      },
    });
    expect(headers).toEqual([
      "Playback status",
      "Title",
      "Artist",
      "#",
      "Time",
      "Rating",
      "Plays",
    ]);
    expect(wrapper.text()).toContain("2 tracks");
  });

  it("persists a rating selected from the rating column", async () => {
    const wrapper = mountLibrary();
    await flushPromises();

    await wrapper.get('input[name="library-rating-1"][value="4"]').setValue(true);
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_local_track_rating", {
      trackId: 1,
      rating: 4,
    });
    expect(wrapper.get('input[name="library-rating-1"][value="4"]').attributes("checked"))
      .toBeDefined();
  });

  it("centers the active track when the virtual list is entered", async () => {
    mountLibrary(2, true);
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("local_library_track_position", {
      snapshotId: "snapshot-1",
      trackId: 2,
    });
    expect(virtualizerMocks.scrollToIndex).toHaveBeenCalledWith(5, {
      align: "center",
      behavior: "auto",
    });
  });

  it("smoothly follows a changed active track", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    virtualizerMocks.scrollToIndex.mockClear();

    await wrapper.setProps({ activeTrackId: 2, isPlaying: true });
    await flushPromises();

    expect(virtualizerMocks.scrollToIndex).toHaveBeenCalledWith(5, {
      align: "center",
      behavior: "smooth",
    });
  });

  it("pauses track following after user scrolling", async () => {
    const wrapper = mountLibrary(1, true);
    await flushPromises();
    tauriMocks.invoke.mockClear();

    await wrapper.get('[role="table"]').trigger("wheel");
    await wrapper.setProps({ activeTrackId: 2 });
    await flushPromises();

    expect(
      tauriMocks.invoke.mock.calls.some(([command]) =>
        command === "local_library_track_position"
      ),
    ).toBe(false);
  });

  it("keeps the active marker visible when the playing row is selected", async () => {
    const wrapper = mountLibrary(1, true);
    await flushPromises();
    const firstTrack = wrapper.get("#library-row-2");

    await firstTrack.trigger("click");

    expect(firstTrack.attributes("aria-current")).toBe("true");
    expect(firstTrack.classes()).toContain("border-l-primary");
    expect(firstTrack.classes()).toContain("ring-primary/40");
    expect(firstTrack.find('[aria-label="Playing"]').exists()).toBe(true);
  });

  it("does not expose manual refresh or re-index actions", async () => {
    queryNeedsReindex = true;
    const wrapper = mountLibrary();
    await flushPromises();

    expect(wrapper.find('[aria-label="Refresh library"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Re-index");
  });

  it("debounces pinyin search and keeps all selected search fields", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockClear();

    await wrapper.get('input[role="searchbox"]').setValue("zjl yhm");
    await vi.advanceTimersByTimeAsync(120);
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("query_local_library", {
      request: {
        search: "zjl yhm",
        searchFields: ["title", "artist", "album"],
        sortField: "relevance",
        sortDirection: "descending",
        collapsedGroupIds: [],
      },
    });
  });

  it("sorts by a clicked column and persists the choice", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockClear();

    await wrapper.get('[role="columnheader"][aria-label="Artist"] button').trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("query_local_library", {
      request: expect.objectContaining({
        sortField: "artist",
        sortDirection: "ascending",
        collapsedGroupIds: [],
      }),
    });
    expect(localStorage.getItem("fika.library-preferences.v1")).toContain('"sortField":"artist"');
  });

  it("selects the complete result set and creates a snapshot-backed queue", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockClear();

    const grid = wrapper.get('[role="table"]');
    await grid.trigger("keydown", { key: "a", ctrlKey: true });
    const firstRow = wrapper.get('#library-row-2');
    await firstRow.trigger("contextmenu", { clientX: 100, clientY: 100 });
    const playAction = wrapper
      .findAll('[aria-label="Track actions"] button')
      .find((button) => button.text().includes("Play selection"));
    await playAction?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_local_library_playback_queue", {
      snapshotId: "snapshot-1",
      startIndex: 0,
      selection: {
        selectAll: true,
        ranges: [],
        excludedRanges: [],
      },
    });
    expect(wrapper.emitted("playbackQueue")?.[0]?.[0]).toEqual(
      expect.objectContaining({ queueId: "queue-1", total: 2 }),
    );
  });

  it("sends the snapshot-backed selection to Collection actions", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    const grid = wrapper.get('[role="table"]');
    const firstRow = wrapper.get("#library-row-2");

    await grid.trigger("keydown", { key: "a", ctrlKey: true });
    await firstRow.trigger("contextmenu", { clientX: 100, clientY: 100 });
    const add = wrapper
      .findAll('[aria-label="Track actions"] button')
      .find((button) => button.text().includes("Add selection to Collection"));
    await add?.trigger("click");

    expect(wrapper.emitted("addToCollection")?.[0]).toEqual([{
      snapshotId: "snapshot-1",
      selection: {
        selectAll: true,
        ranges: [],
        excludedRanges: [],
      },
    }]);

    await firstRow.trigger("contextmenu", { clientX: 100, clientY: 100 });
    const create = wrapper
      .findAll('[aria-label="Track actions"] button')
      .find((button) => button.text().includes("New Collection"));
    await create?.trigger("click");
    expect(wrapper.emitted("createCollection")?.[0]).toEqual(
      wrapper.emitted("addToCollection")?.[0],
    );
  });

  it("writes the selected Local Music rows to the Collection drag payload", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    const setData = vi.fn();
    const dataTransfer = { effectAllowed: "none", setData };

    await wrapper.get("#library-row-2").trigger("dragstart", { dataTransfer });

    const payload = JSON.parse(
      setData.mock.calls.find(([type]) => type === COLLECTION_DRAG_TYPE)?.[1] ?? "null",
    );
    expect(payload).toEqual({
      kind: "local",
      snapshotId: "snapshot-1",
      selection: {
        selectAll: false,
        ranges: [{ start: 0, end: 0 }],
        excludedRanges: [],
      },
    });
  });

  it("double-clicks into the full query snapshot instead of narrowing to the clicked row", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockClear();

    const firstRow = wrapper.get("#library-row-2");
    await firstRow.trigger("click");
    await firstRow.trigger("dblclick");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_local_library_playback_queue", {
      snapshotId: "snapshot-1",
      startIndex: 0,
      selection: null,
    });
  });

  it("starts a random track after the initial library query is ready", async () => {
    let resolveQuery!: (page: LibraryQueryPage) => void;
    const pendingQuery = new Promise<LibraryQueryPage>((resolve) => {
      resolveQuery = resolve;
    });
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "query_local_library") return pendingQuery;
      return defaultInvoke?.(command, args);
    });
    const random = vi.spyOn(Math, "random").mockReturnValue(0.75);
    const wrapper = mountLibrary();
    const browser = wrapper.vm as unknown as { startRandomTrack: () => Promise<void> };

    const playback = browser.startRandomTrack();
    expect(tauriMocks.invoke).not.toHaveBeenCalledWith(
      "create_local_library_playback_queue",
      expect.anything(),
    );
    resolveQuery(queryPage());
    await playback;
    random.mockRestore();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_local_library_playback_queue", {
      snapshotId: "snapshot-1",
      startIndex: 1,
      selection: null,
    });
    expect(wrapper.emitted("playbackQueue")?.[0]?.[1]).toBe(true);
  });

  it("selects and plays a complete album group from its header", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockClear();

    const firstAlbum = wrapper.get("#library-row-0");
    expect(firstAlbum.text()).toContain("叶惠美");
    await firstAlbum.trigger("click");
    await firstAlbum.trigger("dblclick");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_local_library_playback_queue", {
      snapshotId: "snapshot-1",
      startIndex: 0,
      selection: {
        selectAll: false,
        ranges: [{ start: 0, end: 0 }],
        excludedRanges: [],
      },
    });
  });

  it("visually distinguishes a lone track selection from its album selection", async () => {
    const wrapper = mountLibrary();
    await flushPromises();

    const firstAlbum = wrapper.get("#library-row-0");
    const firstTrack = wrapper.get("#library-row-2");

    await firstTrack.trigger("click");

    expect(firstAlbum.attributes("aria-selected")).toBe("false");
    expect(firstTrack.attributes("aria-selected")).toBe("true");
    expect(firstTrack.classes()).toContain("bg-neutral");

    await firstAlbum.trigger("click");

    expect(firstAlbum.attributes("aria-selected")).toBe("true");
    expect(firstTrack.attributes("aria-selected")).toBe("true");
    expect(firstAlbum.findAll("span").some((span) =>
      span.classes().includes("text-neutral-content"))).toBe(true);
    expect(firstTrack.classes()).toContain("bg-neutral/15");
    expect(firstTrack.classes()).toContain("border-l-neutral");
    expect(firstTrack.classes()).not.toContain("text-neutral-content");
  });

  it("collapses an album without changing its track result indexes", async () => {
    const wrapper = mountLibrary();
    await flushPromises();
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "set_local_library_group_collapsed") {
        const page = queryPage();
        return Promise.resolve({
          snapshotId: page.snapshotId,
          groupId: "album-1",
          collapsed: true,
          virtualTotal: 5,
          groupVirtualIndex: 0,
          offset: 0,
          items: page.items.filter((item) => item.trackIndex !== 0).map((item, index) => ({ ...item, index })),
        });
      }
      return Promise.resolve(null);
    });

    await wrapper.get('button[aria-label="Collapse album"]').trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_local_library_group_collapsed", {
      snapshotId: "snapshot-1",
      groupId: "album-1",
      collapsed: true,
    });
  });

  it("requires one-time permission before online cover completion", async () => {
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "query_local_library") return Promise.resolve(queryPage());
      if (command === "get_album_art_settings") return Promise.resolve({ networkEnabled: false });
      if (command === "get_album_art_task_status") return Promise.resolve(idleAlbumArtTask);
      if (command === "get_metadata_lookup_task_status") return Promise.resolve(idleMetadataTask);
      if (command === "resolve_local_album_cover") {
        return Promise.resolve({
          groupId: args?.groupId,
          status: "authorizationRequired",
          dataUrl: null,
          candidates: [],
          message: null,
          writtenTracks: 0,
          failedTracks: 0,
        });
      }
      if (command === "set_album_art_network_enabled") return Promise.resolve({ networkEnabled: true });
      return Promise.resolve(null);
    });
    const wrapper = mountLibrary();
    await flushPromises();

    expect(wrapper.text()).toContain("Enable online metadata completion");
    const enable = wrapper.findAll("button").find((button) => button.text() === "Enable");
    await enable?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_album_art_network_enabled", { enabled: true });
  });

  it("shows the album mismatch reason and writes only the chosen cover candidate", async () => {
    const defaultImplementation = tauriMocks.invoke.getMockImplementation()!;
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "resolve_local_album_cover") {
        if (args?.releaseGroupId) {
          return Promise.resolve({
            groupId: args.groupId,
            status: "downloaded",
            dataUrl: "data:image/jpeg;base64,AA==",
            candidates: [],
            message: null,
            writtenTracks: 1,
            failedTracks: 0,
          });
        }
        return Promise.resolve({
          groupId: args?.groupId,
          status: "needsReview",
          dataUrl: null,
          candidates: [{
            releaseGroupId: "4a45bfa5-eb1e-49eb-a20c-1021389b2121",
            title: "Candidate album",
            artist: "Candidate artist",
            year: 2003,
            score: 100,
          }],
          message: "The MusicBrainz album match has a different track listing.",
          writtenTracks: 0,
          failedTracks: 0,
        });
      }
      return defaultImplementation(command, args);
    });
    const wrapper = mountLibrary();
    await flushPromises();

    await wrapper.get('button[aria-label="Review album cover matches"]').trigger("click");
    expect(wrapper.text()).toContain("different track listing");
    const candidate = wrapper.findAll("button").find((button) => button.text().includes("Candidate album"));
    await candidate?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("resolve_local_album_cover", {
      groupId: "album-1",
      releaseGroupId: "4a45bfa5-eb1e-49eb-a20c-1021389b2121",
    });
  });

  it("confirms a multi-selection before starting metadata lookup", async () => {
    const wrapper = mountLibrary();
    await flushPromises();

    await wrapper.get('[role="table"]').trigger("keydown", { key: "a", ctrlKey: true });
    await wrapper.get("#library-row-2").trigger("contextmenu", { clientX: 100, clientY: 100 });
    const lookup = wrapper
      .findAll('[aria-label="Track actions"] button')
      .find((button) => button.text().includes("Look up metadata"));
    await lookup?.trigger("click");
    const start = wrapper.findAll("button").find((button) => button.text() === "Start");
    await start?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_local_metadata_lookup", {
      snapshotId: "snapshot-1",
      selection: {
        selectAll: true,
        ranges: [],
        excludedRanges: [],
      },
    });
  });
});
