import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import OnlineTrackTable from "./OnlineTrackTable.vue";
import { createOnlineTrack } from "../test/fixtures";

const track = createOnlineTrack({ key: "song-1", title: "Song 1" });

function mountTable(supportsLibraryActions = true, isFavorite = false) {
  return mount(OnlineTrackTable, {
    props: {
      tracks: [track],
      activeKey: null,
      playing: false,
      resolvingKey: null,
      trackActionId: null,
      supportsLibraryActions: () => supportsLibraryActions,
      isFavorite: () => isFavorite,
    },
  });
}

describe("OnlineTrackTable", () => {
  it("emits favorite and playlist actions for an available track", async () => {
    const wrapper = mountTable();

    await wrapper.get('button[aria-label="Add Song 1 to My Favorite Music"]').trigger("click");
    await wrapper.get('button[aria-label="Add Song 1 to a Playlist"]').trigger("click");

    expect(wrapper.emitted("favorite")?.[0]).toEqual([track]);
    expect(wrapper.emitted("addToPlaylist")?.[0]).toEqual([track]);
  });

  it("disables library actions when the track has no NetEase or KuGou candidate", () => {
    const wrapper = mountTable(false);

    expect(wrapper.get('button[aria-label="Add Song 1 to My Favorite Music"]').attributes("disabled")).toBeDefined();
    expect(wrapper.get('button[aria-label="Add Song 1 to a Playlist"]').attributes("disabled")).toBeDefined();
  });

  it("renders a filled error heart for a favorited track", () => {
    const wrapper = mountTable(true, true);
    const heart = wrapper.get('button[aria-label="Add Song 1 to My Favorite Music"]').get("svg");

    expect(heart.attributes("fill")).toBe("currentColor");
    expect(heart.classes()).toContain("text-error");
  });
});
