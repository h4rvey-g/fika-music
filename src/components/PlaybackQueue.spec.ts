import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import PlaybackQueue from "./PlaybackQueue.vue";
import { createLocalTrack, createOnlineTrack } from "../test/fixtures";

const items = [
  { id: "local-1", kind: "local" as const, track: createLocalTrack({ title: "Local song" }) },
  { id: "online-1", kind: "online" as const, track: createOnlineTrack({ title: "Online song" }) },
];

describe("PlaybackQueue", () => {
  it("shows upcoming tracks and emits queue actions", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, items },
    });

    expect(wrapper.get("#playback-queue-title").text()).toBe("Playback queue");
    expect(wrapper.findAll("[data-playback-queue-index]")).toHaveLength(2);

    await wrapper.get('button[aria-label="Play Local song"]').trigger("click");
    await wrapper.get('button[aria-label="Remove Online song from queue"]').trigger("click");
    await wrapper.get("button").trigger("click");

    expect(wrapper.emitted("play")?.[0]).toEqual([0]);
    expect(wrapper.emitted("remove")?.[0]).toEqual([1]);
    expect(wrapper.emitted("close")?.[0]).toEqual([]);
  });

  it("emits a move when a track is dropped at another position", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: { open: true, items },
    });

    await wrapper.findAll("[data-playback-queue-index]")[0].trigger("dragstart");
    await wrapper.findAll("[data-playback-queue-index]")[1].trigger("drop");

    expect(wrapper.emitted("move")?.[0]).toEqual([0, 1]);
  });

  it("keeps playback context tracks read-only and loads more on request", async () => {
    const wrapper = mount(PlaybackQueue, {
      props: {
        open: true,
        items: [{
          ...items[0],
          context: { kind: "local" as const, index: 1 },
        }],
        total: 3,
        canLoadMore: true,
      },
    });

    expect(wrapper.text()).toContain("3 tracks in queue");
    expect(wrapper.find('button[aria-label^="Remove "]').exists()).toBe(false);

    await wrapper.get('button[aria-label="Play Local song"]').trigger("click");
    const loadMoreButton = wrapper.findAll("button")
      .find((button) => button.text().includes("Load more"));
    await loadMoreButton!.trigger("click");

    expect(wrapper.emitted("play")?.[0]).toEqual([0]);
    expect(wrapper.emitted("loadMore")?.[0]).toEqual([]);
  });
});
