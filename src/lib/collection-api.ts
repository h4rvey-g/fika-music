import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../generated/bindings";
import type {
  LibrarySelectionRequest,
  MetadataLookupTaskStatus,
  MusicCollectionDetail,
  MusicCollectionMutation,
  MusicCollectionSummary,
  OnlineTrack,
  SmartCollectionRules,
} from "../generated/bindings";

export type {
  MusicCollectionDetail,
  MusicCollectionItem,
  MusicCollectionItemKind,
  MusicCollectionMutation,
  MusicCollectionSummary,
  SmartCollectionField,
  SmartCollectionOperator,
  SmartCollectionRule,
  SmartCollectionRules,
} from "../generated/bindings";

export const COLLECTION_DRAG_TYPE = "application/x-fika-music-collection-items";

export type LocalCollectionSelection = {
  snapshotId: string;
  selection: LibrarySelectionRequest;
};

export type CollectionItemSelection = {
  sourceCollectionId: string;
  itemIds: string[];
};

export type CollectionSeed =
  | { kind: "empty" }
  | ({ kind: "local" } & LocalCollectionSelection)
  | { kind: "online"; tracks: OnlineTrack[] }
  | ({ kind: "collection" } & CollectionItemSelection);

export type CollectionDragPayload = Exclude<CollectionSeed, { kind: "empty" }>;

export function listMusicCollections() {
  return invoke<MusicCollectionSummary[]>(TAURI_COMMANDS.listMusicCollections);
}

export function createMusicCollection(name: string, smartRules: SmartCollectionRules | null = null) {
  return invoke<MusicCollectionSummary>(TAURI_COMMANDS.createMusicCollection, {
    name,
    smartRules,
  });
}

export function renameMusicCollection(collectionId: string, name: string) {
  return invoke<MusicCollectionSummary>(TAURI_COMMANDS.renameMusicCollection, {
    collectionId,
    name,
  });
}

export function deleteMusicCollection(collectionId: string) {
  return invoke<void>(TAURI_COMMANDS.deleteMusicCollection, { collectionId });
}

export function getMusicCollection(collectionId: string) {
  return invoke<MusicCollectionDetail>(TAURI_COMMANDS.getMusicCollection, { collectionId });
}

export function addLocalSelectionToMusicCollection(
  collectionId: string,
  source: LocalCollectionSelection,
) {
  return invoke<MusicCollectionMutation>(TAURI_COMMANDS.addLocalSelectionToMusicCollection, {
    collectionId,
    snapshotId: source.snapshotId,
    selection: source.selection,
  });
}

export function addOnlineTracksToMusicCollection(
  collectionId: string,
  tracks: OnlineTrack[],
) {
  return invoke<MusicCollectionMutation>(TAURI_COMMANDS.addOnlineTracksToMusicCollection, {
    collectionId,
    tracks,
  });
}

export function addMusicCollectionItemsToMusicCollection(
  collectionId: string,
  source: CollectionItemSelection,
) {
  return invoke<MusicCollectionMutation>(
    TAURI_COMMANDS.addMusicCollectionItemsToMusicCollection,
    {
      collectionId,
      sourceCollectionId: source.sourceCollectionId,
      itemIds: source.itemIds,
    },
  );
}

export function removeMusicCollectionItems(collectionId: string, itemIds: string[]) {
  return invoke<MusicCollectionMutation>(TAURI_COMMANDS.removeMusicCollectionItems, {
    collectionId,
    itemIds,
  });
}

export function startMusicCollectionMetadataLookup(
  collectionId: string,
  itemIds: string[],
) {
  return invoke<MetadataLookupTaskStatus>(TAURI_COMMANDS.startMusicCollectionMetadataLookup, {
    collectionId,
    itemIds,
  });
}

export function writeCollectionDragPayload(
  dataTransfer: DataTransfer | null,
  payload: CollectionDragPayload,
) {
  if (!dataTransfer) return false;
  dataTransfer.effectAllowed = "copy";
  dataTransfer.setData(COLLECTION_DRAG_TYPE, JSON.stringify(payload));
  dataTransfer.setData(
    "text/plain",
    payload.kind === "online"
      ? `${payload.tracks.length} online track${payload.tracks.length === 1 ? "" : "s"}`
      : payload.kind === "collection"
        ? `${payload.itemIds.length} Collection track${payload.itemIds.length === 1 ? "" : "s"}`
      : "Local Music selection",
  );
  return true;
}

export function readCollectionDragPayload(
  dataTransfer: DataTransfer | null,
): CollectionDragPayload | null {
  if (!dataTransfer) return null;
  const raw = dataTransfer.getData(COLLECTION_DRAG_TYPE);
  if (!raw) return null;

  try {
    const payload: unknown = JSON.parse(raw);
    if (!isObject(payload)) return null;
    if (payload.kind === "local") {
      return isLocalCollectionSelection(payload) ? payload : null;
    }
    if (payload.kind === "online" && Array.isArray(payload.tracks)) {
      const tracks = payload.tracks.filter(isOnlineTrack);
      return tracks.length === payload.tracks.length && tracks.length
        ? { kind: "online", tracks }
        : null;
    }
    if (payload.kind === "collection") {
      return isCollectionItemSelection(payload) ? payload : null;
    }
  } catch {
    return null;
  }
  return null;
}

function isCollectionItemSelection(
  value: Record<string, unknown>,
): value is { kind: "collection" } & CollectionItemSelection {
  return (
    typeof value.sourceCollectionId === "string"
    && value.sourceCollectionId.length > 0
    && Array.isArray(value.itemIds)
    && value.itemIds.length > 0
    && value.itemIds.every((itemId) => typeof itemId === "string" && itemId.length > 0)
  );
}

function isLocalCollectionSelection(
  value: Record<string, unknown>,
): value is { kind: "local" } & LocalCollectionSelection {
  if (typeof value.snapshotId !== "string" || !isObject(value.selection)) return false;
  const selection = value.selection;
  return (
    typeof selection.selectAll === "boolean"
    && Array.isArray(selection.ranges)
    && Array.isArray(selection.excludedRanges)
  );
}

function isOnlineTrack(value: unknown): value is OnlineTrack {
  return (
    isObject(value)
    && typeof value.key === "string"
    && typeof value.title === "string"
    && typeof value.artist === "string"
    && Array.isArray(value.candidates)
  );
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
