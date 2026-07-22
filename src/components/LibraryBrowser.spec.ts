import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import LibraryBrowser from "./LibraryBrowser.vue";
import type {
  AlbumArtTaskStatus,
  LibraryAlbumGroup,
  LibraryQueryPage,
  LibraryViewItem,
  LocalTrack,
  MetadataLookupTaskStatus,
  ScanStatus,
} from "../generated/bindings";

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

vi.mock("@tanstack/vue-virtual", () => ({
  useVirtualizer: (options: { value: { count: number; estimateSize: () => number } }) => {
    const virtualizer = {
      getVirtualItems: () =>
        Array.from({ length: Math.min(options.value.count, 20) }, (_, index) => ({
          index,
          key: index,
          start: index * options.value.estimateSize(),
          size: options.value.estimateSize(),
          end: (index + 1) * options.value.estimateSize(),
          lane: 0,
        })),
      getTotalSize: () => options.value.count * options.value.estimateSize(),
      scrollToIndex: vi.fn(),
      measure: vi.fn(),
    };
    return ref(virtualizer);
  },
}));

const idleScanStatus: ScanStatus = {
  isRunning: false,
  folderPath: "/music",
  discoveredFiles: 2,
  scannedFiles: 2,
  indexedTracks: 2,
  skippedFiles: 0,
  errorCount: 0,
  lastError: null,
  startedAt: null,
  finishedAt: null,
};

const tracks: LocalTrack[] = [
  {
    id: 1,
    filePath: "/music/first.flac",
    fileName: "first.flac",
    title: "First",
    artist: "周杰伦",
    album: "叶惠美",
    albumArtist: "周杰伦",
    genre: "Pop",
    year: 2003,
    codec: "FLAC",
    bitrateKbps: 900,
    sampleRateHz: 44100,
    durationSeconds: 180,
    trackNumber: 1,
    discNumber: 1,
    fileSizeBytes: 1024,
    modifiedAt: 1,
    indexedAt: 1,
    playCount: 4,
  },
  {
    id: 2,
    filePath: "/music/second.mp3",
    fileName: "second.mp3",
    title: "Second",
    artist: "Artist",
    album: "Album",
    albumArtist: "Artist",
    genre: "Rock",
    year: 2024,
    codec: "MP3",
    bitrateKbps: 320,
    sampleRateHz: 48000,
    durationSeconds: 181,
    trackNumber: 2,
    discNumber: 1,
    fileSizeBytes: 2048,
    modifiedAt: 1,
    indexedAt: 1,
    playCount: 0,
  },
];

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
    needsReindex: false,
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

function mountLibrary() {
  return mount(LibraryBrowser, {
    props: {
      activeTrackId: null,
      isPlaying: false,
      density: "comfortable",
      scanStatus: idleScanStatus,
      scanMessage: null,
      canIndex: true,
    },
  });
}

describe("LibraryBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorage.clear();
    tauriMocks.listen.mockResolvedValue(vi.fn());
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "query_local_library") {
        return Promise.resolve(queryPage());
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
      "Plays",
    ]);
    expect(wrapper.text()).toContain("2 tracks");
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
