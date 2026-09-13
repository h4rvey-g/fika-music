import type {
  LocalTrack,
  MusicCollectionItem,
  OnlineTrack,
  PlaybackQueueItem,
  PlaybackTrackInput,
} from "../generated/bindings";

export type { PlaybackQueueItem } from "../generated/bindings";

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

export function playbackTrackInputFromCollectionItem(
  item: MusicCollectionItem,
): PlaybackTrackInput | null {
  if (item.localTrack) {
    return { kind: "local", trackId: item.localTrack.id };
  }
  if (item.onlineTrack) {
    return { kind: "online", track: item.onlineTrack };
  }
  return null;
}
