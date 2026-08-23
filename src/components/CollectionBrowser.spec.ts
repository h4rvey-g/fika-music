import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CollectionBrowser from "./CollectionBrowser.vue";
import type {
  MusicCollectionDetail,
  MusicCollectionMutation,
} from "../lib/collection-api";
import { createLocalTrack, createOnlineTrack } from "../test/fixtures";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(async () => () => undefined),
  revealItemInDir: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: tauriMocks.revealItemInDir,
}));

const collection = {
  id: "collection-1",
  name: "Mixed",
  itemCount: 2,
  localCount: 1,
  onlineCount: 1,
  createdAt: 1,
  updatedAt: 1,
  smartRules: null,
};

const detail: MusicCollectionDetail = {
  collection,
  items: [
    {
      id: "item-local",
      position: 0,
      kind: "local",
      localTrack: createLocalTrack({ title: "Local Song" }),
      localAlbumGroupId: "album:local",
      onlineTrack: null,
      addedAt: 1,
    },
    {
      id: "item-online",
      position: 1,
      kind: "online",
      localTrack: null,
      localAlbumGroupId: null,
      onlineTrack: createOnlineTrack({ key: "online-song", title: "Online Song" }),
      addedAt: 2,
    },
  ],
};

function mountCollection() {
  return mount(CollectionBrowser, {
    props: {
      collectionId: collection.id,
      refreshKey: 0,
      activeLocalTrackId: null,
      activeOnlineTrack: null,
      isPlaying: false,
    },
  });
}

describe("CollectionBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_music_collection") return Promise.resolve(detail);
      if (command === "get_album_art_settings") {
        return Promise.resolve({ networkEnabled: true });
      }
      if (command === "get_album_art_task_status" || command === "get_metadata_lookup_task_status") {
        return Promise.resolve(null);
      }
      if (command === "resolve_local_album_cover") {
        return Promise.resolve({
          groupId: "album:local",
          status: "embedded",
          dataUrl: null,
          candidates: [],
          message: null,
          writtenTracks: 0,
          failedTracks: 0,
        });
      }
      if (command === "remove_music_collection_items") {
        const mutation: MusicCollectionMutation = {
          collection: { ...collection, itemCount: 1, localCount: 0 },
          added: 0,
          skipped: 0,
          removed: 1,
        };
        return Promise.resolve(mutation);
      }
      if (command === "set_local_track_rating") return Promise.resolve(args?.rating);
      return Promise.resolve(null);
    });
  });

  it("renders mixed local and online tracks and starts the Collection queue", async () => {
    const wrapper = mountCollection();
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("get_music_collection", {
      collectionId: "collection-1",
    });
    expect(wrapper.findAll("[data-track-row]").map((row) => row.text())).toEqual([
      expect.stringContaining("Local Song"),
      expect.stringContaining("Online Song"),
    ]);
    expect(wrapper.findAll("[data-album-row]")).toHaveLength(1);
    expect(wrapper.findAll('[role="radiogroup"]')).toHaveLength(1);

    await wrapper.get('[data-collection-item-id="item-online"]').trigger("dblclick");
    expect(wrapper.emitted("play")?.[0]).toEqual([detail.items, 1, true]);
  });

  it("persists ratings for local Collection tracks", async () => {
    const wrapper = mountCollection();
    await flushPromises();

    await wrapper.get('input[name="collection-rating-1"][value="5"]').setValue(true);
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("set_local_track_rating", {
      trackId: 1,
      rating: 5,
    });
  });

  it("waits for its in-flight load and starts the full Collection", async () => {
    let resolveDetail: (value: MusicCollectionDetail) => void = () => undefined;
    const pendingDetail = new Promise<MusicCollectionDetail>((resolve) => {
      resolveDetail = resolve;
    });
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_music_collection") return pendingDetail;
      return defaultInvoke?.(command, args);
    });
    const wrapper = mountCollection();
    const browser = wrapper.vm as unknown as {
      startCollection: (collectionId?: string) => Promise<void>;
    };

    const playback = browser.startCollection(collection.id);
    expect(wrapper.emitted("play")).toBeUndefined();
    resolveDetail(detail);
    await playback;
    await flushPromises();

    expect(
      tauriMocks.invoke.mock.calls.filter(([command]) => command === "get_music_collection"),
    ).toHaveLength(1);
    expect(wrapper.emitted("play")?.[0]).toEqual([detail.items, 0, true]);

    await wrapper.get('input[aria-label="Search Collection"]').setValue("Local");
    await browser.startCollection(collection.id);
    expect(wrapper.emitted("play")?.[1]).toEqual([detail.items, 0, true]);
  });

  it("filters tracks and removes a context-menu selection from the Collection", async () => {
    const wrapper = mountCollection();
    await flushPromises();

    await wrapper.get('input[aria-label="Search Collection"]').setValue("Local");
    expect(wrapper.findAll("[data-track-row]")).toHaveLength(1);

    await wrapper.get('[data-collection-item-id="item-local"]').trigger("contextmenu", {
      clientX: 20,
      clientY: 20,
    });
    const removeButton = wrapper.findAll("button").find((button) =>
      button.text().includes("Remove selection from Collection"));
    expect(removeButton).toBeTruthy();
    await removeButton!.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("remove_music_collection_items", {
      collectionId: "collection-1",
      itemIds: ["item-local"],
    });
    expect(wrapper.emitted("changed")?.[0]).toEqual([
      expect.objectContaining({ itemCount: 1, localCount: 0 }),
    ]);
  });

  it("does not offer manual member removal for a Smart Collection", async () => {
    const smartDetail: MusicCollectionDetail = {
      ...detail,
      collection: {
        ...detail.collection,
        smartRules: {
          rules: [{ field: "artist", operator: "equals", value: "Artist" }],
        },
      },
    };
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_music_collection") return Promise.resolve(smartDetail);
      return defaultInvoke?.(command, args);
    });
    const wrapper = mountCollection();
    await flushPromises();

    await wrapper.get('[data-collection-item-id="item-local"]').trigger("contextmenu", {
      clientX: 20,
      clientY: 20,
    });

    expect(wrapper.get('[aria-label="Collection track actions"]').text())
      .not.toContain("Remove selection from Collection");
  });

  it("supports additive multi-selection and queues the selected tracks", async () => {
    const wrapper = mountCollection();
    await flushPromises();
    const local = wrapper.get('[data-collection-item-id="item-local"]');
    const online = wrapper.get('[data-collection-item-id="item-online"]');

    await local.trigger("click");
    await online.trigger("click", { ctrlKey: true });
    await online.trigger("contextmenu", { clientX: 20, clientY: 20 });
    const queueButton = wrapper.findAll("button").find((button) =>
      button.text().includes("Set playback queue"));
    await queueButton!.trigger("click");

    expect(wrapper.emitted("play")?.[0]).toEqual([detail.items, 1, false]);
  });

  it("collapses album groups without discarding their tracks", async () => {
    const wrapper = mountCollection();
    await flushPromises();

    await wrapper.get('button[aria-label="Collapse album"]').trigger("click");
    expect(wrapper.findAll("[data-track-row]")).toHaveLength(0);
    expect(wrapper.text()).toContain("2 tracks");

    await wrapper.get('button[aria-label="Expand album"]').trigger("click");
    expect(wrapper.findAll("[data-track-row]")).toHaveLength(2);
  });

  it("shows file actions only for a local context track", async () => {
    const wrapper = mountCollection();
    await flushPromises();

    await wrapper.get('[data-collection-item-id="item-online"]').trigger("contextmenu", {
      clientX: 20,
      clientY: 20,
    });
    expect(wrapper.get('[aria-label="Collection track actions"]').text())
      .not.toContain("Show in file manager");

    await wrapper.get('[data-collection-item-id="item-local"]').trigger("contextmenu", {
      clientX: 20,
      clientY: 20,
    });
    expect(wrapper.get('[aria-label="Collection track actions"]').text())
      .toContain("Show in file manager");
  });
});
