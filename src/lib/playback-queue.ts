import type { LocalTrack, MusicCollectionItem } from "../generated/bindings";
import type { OnlineTrack } from "./online-music-api";

export type PlaybackQueuePlacement = "next" | "last";

export type PlaybackQueueItem =
  | {
      id: string;
      kind: "local";
      track: LocalTrack;
    }
  | {
      id: string;
      kind: "online";
      track: OnlineTrack;
    };

export type PlaybackQueueTrack = LocalTrack | OnlineTrack;

export function playbackQueueItemTitle(item: PlaybackQueueItem) {
  return item.track.title || (item.kind === "local" ? item.track.fileName : "");
}

export function playbackQueueItemSubtitle(item: PlaybackQueueItem) {
  if (item.kind === "local") {
    return [item.track.artist, item.track.album].filter(Boolean).join(" - ")
      || item.track.fileName;
  }
  return [item.track.artist, item.track.album].filter(Boolean).join(" - ")
    || "Remote track";
}

export function playbackQueueItemFromCollectionItem(
  item: MusicCollectionItem,
  id: string,
): PlaybackQueueItem | null {
  if (item.localTrack) {
    return { id, kind: "local", track: item.localTrack };
  }
  if (item.onlineTrack) {
    return { id, kind: "online", track: item.onlineTrack };
  }
  return null;
}
