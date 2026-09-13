import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PlaybackQueue from "./PlaybackQueue.vue";
import { createLocalTrack, createOnlineTrack } from "../test/fixtures";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const items = [
  { id: "local-1", kind: "local" as const, track: createLocalTrack({ title: "Local song" }) },
  { id: "online-1", kind: "online" as const, track: createOnlineTrack({ title: "Online song" }) },
];

enableAutoUnmount(afterEach);
const elementFromPointDescriptor = Object.getOwnPropertyDescriptor(document, "elementFromPoint");

describe("PlaybackQueue", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    vi.stubGlobal("PointerEvent", MouseEvent);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    if (elementFromPointDescriptor) {
      Object.defineProperty(document, "elementFromPoint", elementFromPointDescriptor);
    } else {
      Reflect.deleteProperty(document, "elementFromPoint");
    }
  });

  it("keeps the queue in a compact panel despite the global modal width rule", () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });

    expect(wrapper.get(".playback-queue-box").classes()).toContain("max-w-lg!");
  });

  it("shows upcoming tracks and emits queue actions", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });

    expect(wrapper.get("#playback-queue-title").text()).toBe("Playback queue");
    expect(wrapper.findAll("[data-playback-queue-index]")).toHaveLength(2);

    await wrapper.get('button[aria-label="Select Local song"]').trigger("dblclick");
    await wrapper.get('button[aria-label="Remove Online song from queue"]').trigger("click");
    await wrapper.get("button").trigger("click");

    expect(wrapper.emitted("play")?.[0]).toEqual([0]);
    expect(wrapper.emitted("remove")?.[0]).toEqual([1]);
    expect(wrapper.emitted("close")?.[0]).toEqual([]);
  });

  it("loads and displays embedded artwork for local queue tracks", async () => {
    const coverDataUrl = "data:image/png;base64,bG9jYWwtY292ZXI=";
    invokeMock.mockResolvedValue(coverDataUrl);
    let revealArtwork: (() => void) | undefined;
    vi.stubGlobal("IntersectionObserver", class {
      constructor(callback: IntersectionObserverCallback) {
        revealArtwork = () => callback(
          [{ isIntersecting: true } as IntersectionObserverEntry],
          this as unknown as IntersectionObserver,
        );
      }

      observe() {}
      disconnect() {}
    });
    const localItem = {
      id: "local-cover",
      kind: "local" as const,
      track: createLocalTrack({ id: 42, title: "Covered local song" }),
    };
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items: [localItem] },
    });

    expect(invokeMock).not.toHaveBeenCalled();
    revealArtwork?.();
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith("local_track_cover_data_url", { trackId: 42 });
    expect(wrapper.get('img[src^="data:image/png"]').attributes("src")).toBe(coverDataUrl);
  });

  it("selects on a single click and plays only on double click", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });
    const local = wrapper.get('button[aria-label="Select Local song"]');
    const online = wrapper.get('button[aria-label="Select Online song"]');

    await local.trigger("click");
    expect(local.attributes("aria-pressed")).toBe("true");
    expect(wrapper.emitted("play")).toBeUndefined();

    await online.trigger("click");
    expect(local.attributes("aria-pressed")).toBe("false");
    expect(online.attributes("aria-pressed")).toBe("true");
    expect(wrapper.emitted("play")).toBeUndefined();

    await online.trigger("dblclick");
    expect(wrapper.emitted("play")).toEqual([[1]]);

    await wrapper.setProps({ items: [items[1], items[0]] });
    expect(wrapper.findAll('[aria-pressed="true"]')).toHaveLength(1);
    expect(online.attributes("aria-pressed")).toBe("true");
  });

  it("reorders with pointer movement even when native drag events are unavailable", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });
    const rows = wrapper.findAll("[data-playback-queue-index]");
    const pointTarget = vi.fn(() => rows[1].element);
    Object.defineProperty(document, "elementFromPoint", { configurable: true, value: pointTarget });

    await rows[0].trigger("pointerdown", { button: 0, pointerId: 1, clientX: 20, clientY: 20 });
    await rows[0].trigger("pointermove", { pointerId: 1, clientX: 20, clientY: 80 });
    expect(wrapper.emitted("move")).toBeUndefined();
    await rows[0].trigger("pointerup", { pointerId: 1, clientX: 20, clientY: 80 });
    await rows[0].trigger("click");

    expect(wrapper.emitted("move")).toEqual([[0, 1]]);
    expect(wrapper.emitted("play")).toBeUndefined();
  });

  it("does not reorder after cancellation, outside release, or a concurrent queue change", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });
    const rows = wrapper.findAll("[data-playback-queue-index]");
    const pointTarget = vi.fn<() => Element | null>(() => rows[1].element);
    Object.defineProperty(document, "elementFromPoint", { configurable: true, value: pointTarget });
    const start = async () => {
      await rows[0].trigger("pointerdown", { button: 0, pointerId: 1, clientX: 20, clientY: 20 });
      await rows[0].trigger("pointermove", { pointerId: 1, clientX: 20, clientY: 80 });
    };

    await start();
    await rows[0].trigger("pointercancel", { pointerId: 1 });
    await rows[0].trigger("pointerup", { pointerId: 1 });
    expect(wrapper.emitted("move")).toBeUndefined();

    await start();
    pointTarget.mockReturnValue(null);
    await rows[0].trigger("pointerup", { pointerId: 1, clientX: -10, clientY: -10 });
    expect(wrapper.emitted("move")).toBeUndefined();

    pointTarget.mockReturnValue(rows[1].element);
    await start();
    await wrapper.setProps({ items: [items[1], items[0]] });
    await rows[0].trigger("pointerup", { pointerId: 1, clientX: 20, clientY: 80 });
    expect(wrapper.emitted("move")).toBeUndefined();
  });

  it("does not select or play a track when its remove button is clicked twice", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, current: null, items },
    });
    const remove = wrapper.get('button[aria-label="Remove Online song from queue"]');
    await remove.trigger("click");
    await remove.trigger("dblclick");
    expect(wrapper.emitted("remove")).toEqual([[1]]);
    expect(wrapper.emitted("play")).toBeUndefined();
    expect(wrapper.find('[aria-pressed="true"]').exists()).toBe(false);
  });

  it("shows a read-only current track while every upcoming item remains editable", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: {
        open: true,
        current: items[1],
        items: [items[0]],
        total: 3,
        canLoadMore: true,
      },
    });

    expect(wrapper.text()).toContain("3 tracks in queue");
    expect(wrapper.text()).toContain("Now playing");
    expect(wrapper.text()).toContain("Online song");
    expect(wrapper.find('button[aria-label="Remove Local song from queue"]').exists()).toBe(true);

    await wrapper.get('button[aria-label="Select Local song"]').trigger("dblclick");
    const loadMoreButton = wrapper.findAll("button")
      .find((button) => button.text().includes("Load more"));
    await loadMoreButton!.trigger("click");

    expect(wrapper.emitted("play")?.[0]).toEqual([0]);
    expect(wrapper.emitted("loadMore")?.[0]).toEqual([]);
  });
});
