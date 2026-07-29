import { describe, expect, it } from "vitest";
import type { MusicCollectionItem } from "../generated/bindings";
import { createLocalTrack, createOnlineTrack } from "../test/fixtures";
import { buildCollectionAlbumGroups } from "./collection-browser-model";

function localItem(
  id: string,
  position: number,
  overrides: Parameters<typeof createLocalTrack>[0] = {},
): MusicCollectionItem {
  return {
    id,
    position,
    kind: "local",
    localTrack: createLocalTrack(overrides),
    localAlbumGroupId: overrides.album === null ? "ungrouped" : "album:shared",
    onlineTrack: null,
    addedAt: position,
  };
}

function onlineItem(
  id: string,
  position: number,
  overrides: Parameters<typeof createOnlineTrack>[0] = {},
): MusicCollectionItem {
  return {
    id,
    position,
    kind: "online",
    localTrack: null,
    localAlbumGroupId: null,
    onlineTrack: createOnlineTrack(overrides),
    addedAt: position,
  };
}

describe("Collection browser model", () => {
  it("groups matching local and online tracks into the same album", () => {
    const groups = buildCollectionAlbumGroups(
      [
        localItem("local", 0, { album: "Shared", albumArtist: "Band", artist: "Band" }),
        onlineItem("online", 1, { album: "Shared", artist: "Band" }),
        localItem("ungrouped", 2, { album: null }),
      ],
      "",
      ["title", "artist", "album"],
      "relevance",
      "descending",
    );

    expect(groups.map((group) => ({
      title: group.title,
      items: group.tracks.map((track) => track.item.id),
      localAlbumGroupId: group.localAlbumGroupId,
    }))).toEqual([
      {
        title: "Shared",
        items: ["local", "online"],
        localAlbumGroupId: "album:shared",
      },
      {
        title: null,
        items: ["ungrouped"],
        localAlbumGroupId: null,
      },
    ]);
  });

  it("requires every search term and sorts matched tracks by the selected column", () => {
    const groups = buildCollectionAlbumGroups(
      [
        localItem("second", 0, { title: "Second Song", artist: "Band", album: "Shared" }),
        localItem("first", 1, { title: "First Song", artist: "Band", album: "Shared" }),
        onlineItem("other", 2, { title: "Second Song", artist: "Other", album: "Shared" }),
      ],
      "song band",
      ["title", "artist"],
      "title",
      "ascending",
    );

    expect(groups[0].tracks.map((track) => track.item.id)).toEqual(["first", "second"]);
  });
});
