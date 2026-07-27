import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import OnlineTrackTable from "./OnlineTrackTable.vue";
import { createOnlineTrack } from "../test/fixtures";

const track = createOnlineTrack({ key: "song-1", title: "Song 1" });

function mountTable(
  supportsLibraryActions = true,
  isFavorite = false,
  tracks = [track],
) {
  return mount(OnlineTrackTable, {
    props: {
      tracks,
      activeTrack: null,
      isPlaying: false,
      trackActionId: null,
      supportsLibraryActions: () => supportsLibraryActions,
      supportsPlaylistSelection: () => supportsLibraryActions,
      isFavorite: () => isFavorite,
    },
  });
}

describe("OnlineTrackTable", () => {
  it("plays on row double-click without rendering a leading play button", async () => {
    const wrapper = mountTable();

    expect(wrapper.find('button[aria-label="Play Song 1"]').exists()).toBe(false);
    await wrapper.get('[data-online-track-key="song-1"]').trigger("dblclick");

    expect(wrapper.emitted("play")?.[0]).toEqual([track]);
  });

  it("highlights every normalized match and shows the paused state", async () => {
    const activeTrack = createOnlineTrack({
      key: "exact-song",
      title: "Song",
      artist: "A / B",
      album: "Album",
      durationSeconds: 180,
    });
    const equivalentTrack = createOnlineTrack({
      key: "equivalent-song",
      title: " song ",
      artist: "B & A",
      album: " album ",
      durationSeconds: 185,
    });
    const wrapper = mountTable(true, false, [activeTrack, equivalentTrack]);
    await wrapper.setProps({ activeTrack, isPlaying: false });
    const rows = wrapper.findAll("tbody tr");

    expect(rows.every((row) => row.attributes("aria-current") === "true")).toBe(true);
    expect(rows.every((row) => row.classes().includes("border-l-primary"))).toBe(true);
    expect(wrapper.findAll('[aria-label="Paused"]')).toHaveLength(2);
  });

  it("retains the active marker when the online row is selected", async () => {
    const wrapper = mountTable();
    await wrapper.setProps({ activeTrack: track, isPlaying: true });
    const row = wrapper.get('[data-online-track-key="song-1"]');

    await row.trigger("click");

    expect(row.classes()).toContain("border-l-primary");
    expect(row.classes()).toContain("ring-primary/40");
    expect(row.get('[aria-label="Playing"]').classes()).toContain("text-neutral-content");
  });

  it("multi-selects tracks and exposes batch actions from the context menu", async () => {
    const tracks = [
      track,
      createOnlineTrack({ key: "song-2", title: "Song 2" }),
      createOnlineTrack({ key: "song-3", title: "Song 3" }),
    ];
    const wrapper = mountTable(true, false, tracks);
    const rows = wrapper.findAll("tbody tr");

    await rows[0].trigger("click");
    await rows[2].trigger("click", { ctrlKey: true });
    await rows[2].trigger("contextmenu", { clientX: 100, clientY: 100 });

    expect(rows[0].attributes("aria-selected")).toBe("true");
    expect(rows[1].attributes("aria-selected")).toBe("false");
    expect(rows[2].attributes("aria-selected")).toBe("true");
    expect(wrapper.get("[data-online-track-menu]").text()).toContain("2 tracks selected");

    const download = wrapper
      .findAll("[data-online-track-menu] button")
      .find((button) => button.text().includes("Download"));
    await download?.trigger("click");
    expect(wrapper.emitted("downloadSelection")?.[0]).toEqual([[tracks[0], tracks[2]]]);

    await rows[2].trigger("contextmenu", { clientX: 100, clientY: 100 });
    const addToPlaylist = wrapper
      .findAll("[data-online-track-menu] button")
      .find((button) => button.text().includes("Add to Playlist"));
    await addToPlaylist?.trigger("click");
    expect(wrapper.emitted("addSelectionToPlaylist")?.[0]).toEqual([[tracks[0], tracks[2]]]);
  });

  it("selects a contiguous range with shift-click", async () => {
    const tracks = [
      track,
      createOnlineTrack({ key: "song-2", title: "Song 2" }),
      createOnlineTrack({ key: "song-3", title: "Song 3" }),
    ];
    const wrapper = mountTable(true, false, tracks);
    const rows = wrapper.findAll("tbody tr");

    await rows[0].trigger("click");
    await rows[2].trigger("click", { shiftKey: true });

    expect(rows.every((row) => row.attributes("aria-selected") === "true")).toBe(true);
  });

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
