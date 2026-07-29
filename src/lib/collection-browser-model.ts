import type {
  LibrarySortDirection,
  LibrarySortField,
  LibraryTextField,
  MusicCollectionItem,
} from "../generated/bindings";
import type { LibraryColumnId } from "./library-preferences";
import { currentLocale, formatNumber, t } from "../i18n";

export type CollectionColumnDefinition = {
  id: LibraryColumnId;
  label: string;
  sortField: LibrarySortField | null;
  numeric?: boolean;
};

export type CollectionTrackView = {
  item: MusicCollectionItem;
  title: string;
  artist: string | null;
  album: string | null;
  albumArtist: string | null;
  genre: string | null;
  year: number | null;
  codec: string | null;
  bitrateKbps: number | null;
  sampleRateHz: number | null;
  durationSeconds: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  fileName: string;
  filePath: string;
  fileSizeBytes: number | null;
  modifiedAt: number | null;
  indexedAt: number | null;
  playCount: number | null;
  coverUrl: string | null;
};

export type CollectionAlbumGroup = {
  id: string;
  title: string | null;
  albumArtist: string | null;
  year: number | null;
  tracks: CollectionTrackView[];
  totalTracks: number;
  totalDurationSeconds: number;
  isUngrouped: boolean;
  localAlbumGroupId: string | null;
  coverUrl: string | null;
};

export const COLLECTION_COLUMN_DEFINITIONS: ReadonlyArray<CollectionColumnDefinition> = [
  { id: "playing", label: "", sortField: null },
  { id: "title", label: "Title", sortField: "title" },
  { id: "artist", label: "Artist", sortField: "artist" },
  { id: "album", label: "Album", sortField: "album" },
  { id: "albumArtist", label: "Album artist", sortField: "albumArtist" },
  { id: "genre", label: "Genre", sortField: "genre" },
  { id: "year", label: "Year", sortField: "year", numeric: true },
  { id: "codec", label: "Codec", sortField: "codec" },
  { id: "bitrateKbps", label: "Bitrate", sortField: "bitrateKbps", numeric: true },
  { id: "sampleRateHz", label: "Sample rate", sortField: "sampleRateHz", numeric: true },
  { id: "durationSeconds", label: "Time", sortField: "durationSeconds", numeric: true },
  { id: "trackNumber", label: "#", sortField: "trackNumber", numeric: true },
  { id: "discNumber", label: "Disc", sortField: "discNumber", numeric: true },
  { id: "fileName", label: "File name", sortField: "fileName" },
  { id: "filePath", label: "Path", sortField: "filePath" },
  { id: "fileSizeBytes", label: "Size", sortField: "fileSizeBytes", numeric: true },
  { id: "modifiedAt", label: "Modified", sortField: "modifiedAt" },
  { id: "indexedAt", label: "Indexed", sortField: "indexedAt" },
  { id: "playCount", label: "Plays", sortField: "playCount", numeric: true },
];

export const COLLECTION_SEARCH_FIELD_OPTIONS: ReadonlyArray<{
  id: LibraryTextField;
  label: string;
}> = [
  { id: "title", label: "Title" },
  { id: "artist", label: "Artist" },
  { id: "album", label: "Album" },
  { id: "albumArtist", label: "Album artist" },
  { id: "genre", label: "Genre" },
  { id: "codec", label: "Codec" },
  { id: "fileName", label: "File name" },
  { id: "filePath", label: "File path" },
];

export function buildCollectionAlbumGroups(
  items: MusicCollectionItem[],
  search: string,
  searchFields: LibraryTextField[],
  sortField: LibrarySortField,
  sortDirection: LibrarySortDirection,
) {
  const allGroups = new Map<string, CollectionTrackView[]>();
  for (const item of items) {
    const track = collectionTrackView(item);
    const key = collectionAlbumIdentity(track);
    const tracks = allGroups.get(key);
    if (tracks) tracks.push(track);
    else allGroups.set(key, [track]);
  }

  const terms = normalizeSearch(search).split(/\s+/).filter(Boolean);
  const direction = sortDirection === "ascending" ? 1 : -1;
  const groups: CollectionAlbumGroup[] = [];
  for (const [id, allTracks] of allGroups) {
    const tracks = allTracks.filter((track) => matchesSearch(track, terms, searchFields));
    if (!tracks.length) continue;
    tracks.sort((left, right) => collectionTrackOrder(left, right, sortField, direction));
    const representative = tracks[0];
    groups.push({
      id,
      title: representative.album,
      albumArtist: representative.albumArtist || representative.artist,
      year: representativeYear(allTracks),
      tracks,
      totalTracks: allTracks.length,
      totalDurationSeconds: allTracks.reduce(
        (total, track) => total + (track.durationSeconds ?? 0),
        0,
      ),
      isUngrouped: !representative.album,
      localAlbumGroupId: representative.album
        ? allTracks.find((track) => track.item.localAlbumGroupId)?.item.localAlbumGroupId ?? null
        : null,
      coverUrl: allTracks.find((track) => track.coverUrl)?.coverUrl ?? null,
    });
  }

  groups.sort((left, right) => {
    if (sortField === "relevance") {
      return minimumPosition(left) - minimumPosition(right);
    }
    const order = compareOptional(
      sortValue(left.tracks[0], sortField),
      sortValue(right.tracks[0], sortField),
    );
    return order * direction || minimumPosition(left) - minimumPosition(right);
  });
  return groups;
}

export function collectionTrackView(item: MusicCollectionItem): CollectionTrackView {
  if (item.localTrack) {
    const track = item.localTrack;
    return {
      item,
      title: track.title,
      artist: track.artist,
      album: track.album,
      albumArtist: track.albumArtist,
      genre: track.genre,
      year: track.year,
      codec: track.codec,
      bitrateKbps: track.bitrateKbps,
      sampleRateHz: track.sampleRateHz,
      durationSeconds: track.durationSeconds,
      trackNumber: track.trackNumber,
      discNumber: track.discNumber,
      fileName: track.fileName,
      filePath: track.filePath,
      fileSizeBytes: track.fileSizeBytes,
      modifiedAt: track.modifiedAt,
      indexedAt: track.indexedAt,
      playCount: track.playCount,
      coverUrl: null,
    };
  }
  const track = item.onlineTrack;
  return {
    item,
    title: track?.title ?? t("Unavailable track"),
    artist: track?.artist || null,
    album: track?.album ?? null,
    albumArtist: track?.artist || null,
    genre: null,
    year: null,
    codec: track ? t("Online") : null,
    bitrateKbps: null,
    sampleRateHz: null,
    durationSeconds: track?.durationSeconds ?? null,
    trackNumber: track?.trackNumber ?? null,
    discNumber: track?.discNumber ?? null,
    fileName: "",
    filePath: "",
    fileSizeBytes: null,
    modifiedAt: null,
    indexedAt: null,
    playCount: null,
    coverUrl: track?.coverUrl ?? null,
  };
}

export function displayCollectionTrackValue(
  track: CollectionTrackView,
  columnId: LibraryColumnId,
) {
  switch (columnId) {
    case "title": return track.title;
    case "artist": return track.artist || t("Unknown artist");
    case "album": return track.album || t("Unknown album");
    case "albumArtist": return track.albumArtist || "";
    case "genre": return track.genre || "";
    case "year": return track.year?.toString() ?? "";
    case "codec": return track.codec || "";
    case "bitrateKbps": return track.bitrateKbps ? `${track.bitrateKbps} kbps` : "";
    case "sampleRateHz": return track.sampleRateHz ? formatSampleRate(track.sampleRateHz) : "";
    case "durationSeconds": return formatCollectionDuration(track.durationSeconds);
    case "trackNumber": return track.trackNumber?.toString() ?? "";
    case "discNumber": return track.discNumber?.toString() ?? "";
    case "fileName": return track.fileName;
    case "filePath": return track.filePath;
    case "fileSizeBytes": return track.fileSizeBytes === null ? "" : formatFileSize(track.fileSizeBytes);
    case "modifiedAt": return formatTimestamp(track.modifiedAt);
    case "indexedAt": return formatTimestamp(track.indexedAt);
    case "playCount": return track.playCount === null ? "" : formatNumber(track.playCount);
    default: return "";
  }
}

export function formatCollectionDuration(seconds: number | null) {
  if (!seconds) return "--:--";
  const minutes = Math.floor(seconds / 60);
  const remaining = Math.floor(seconds % 60);
  return `${minutes}:${remaining.toString().padStart(2, "0")}`;
}

export function formatCollectionLongDuration(seconds: number) {
  if (seconds <= 0) return t("{count} min", { count: 0 });
  const hours = Math.floor(seconds / 3_600);
  const minutes = Math.round((seconds % 3_600) / 60);
  return hours
    ? t("{hours} hr {minutes} min", { hours, minutes })
    : t("{count} min", { count: minutes });
}

function collectionAlbumIdentity(track: CollectionTrackView) {
  if (!track.album) return "collection:ungrouped";
  return `collection:album:${normalizeSearch(track.albumArtist || track.artist || "")}\u001f${normalizeSearch(track.album)}`;
}

function matchesSearch(
  track: CollectionTrackView,
  terms: string[],
  searchFields: LibraryTextField[],
) {
  if (!terms.length) return true;
  const values = searchFields.map((field) => normalizeSearch(searchValue(track, field)));
  return terms.every((term) => values.some((value) => value.includes(term)));
}

function searchValue(track: CollectionTrackView, field: LibraryTextField) {
  switch (field) {
    case "title": return track.title;
    case "artist": return track.artist || "";
    case "album": return track.album || "";
    case "albumArtist": return track.albumArtist || "";
    case "genre": return track.genre || "";
    case "codec": return track.codec || "";
    case "fileName": return track.fileName;
    case "filePath": return track.filePath;
  }
}

function collectionTrackOrder(
  left: CollectionTrackView,
  right: CollectionTrackView,
  sortField: LibrarySortField,
  direction: number,
) {
  if (sortField !== "relevance") {
    const explicit = compareOptional(sortValue(left, sortField), sortValue(right, sortField));
    if (explicit) return explicit * direction;
  }
  return compareOptional(left.discNumber, right.discNumber)
    || compareOptional(left.trackNumber, right.trackNumber)
    || left.item.position - right.item.position;
}

function sortValue(track: CollectionTrackView, field: LibrarySortField): string | number | null {
  switch (field) {
    case "title": return normalizeSearch(track.title);
    case "artist": return normalizeSearch(track.artist || "");
    case "album": return normalizeSearch(track.album || "");
    case "albumArtist": return normalizeSearch(track.albumArtist || "");
    case "genre": return normalizeSearch(track.genre || "");
    case "year": return track.year;
    case "codec": return normalizeSearch(track.codec || "");
    case "bitrateKbps": return track.bitrateKbps;
    case "sampleRateHz": return track.sampleRateHz;
    case "durationSeconds": return track.durationSeconds;
    case "trackNumber": return track.trackNumber;
    case "discNumber": return track.discNumber;
    case "fileName": return normalizeSearch(track.fileName);
    case "filePath": return normalizeSearch(track.filePath);
    case "fileSizeBytes": return track.fileSizeBytes;
    case "modifiedAt": return track.modifiedAt;
    case "indexedAt": return track.indexedAt;
    case "playCount": return track.playCount;
    case "relevance": return track.item.position;
  }
}

function compareOptional(
  left: string | number | null,
  right: string | number | null,
) {
  const leftMissing = left === null || left === "";
  const rightMissing = right === null || right === "";
  if (leftMissing || rightMissing) {
    if (leftMissing && rightMissing) return 0;
    return leftMissing ? 1 : -1;
  }
  return typeof left === "number" && typeof right === "number"
    ? left - right
    : String(left).localeCompare(String(right));
}

function minimumPosition(group: CollectionAlbumGroup) {
  return Math.min(...group.tracks.map((track) => track.item.position));
}

function representativeYear(tracks: CollectionTrackView[]) {
  const counts = new Map<number, number>();
  for (const track of tracks) {
    if (track.year !== null) counts.set(track.year, (counts.get(track.year) ?? 0) + 1);
  }
  return [...counts].sort((left, right) => right[1] - left[1] || left[0] - right[0])[0]?.[0] ?? null;
}

function normalizeSearch(value: string) {
  return value
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase()
    .replace(/\s+/g, " ")
    .trim();
}

function formatFileSize(bytes: number) {
  return bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatSampleRate(hz: number) {
  return hz >= 1_000 ? `${(hz / 1_000).toFixed(hz % 1_000 ? 1 : 0)} kHz` : `${hz} Hz`;
}

function formatTimestamp(timestamp: number | null) {
  if (!timestamp) return "";
  return new Intl.DateTimeFormat(currentLocale.value, {
    year: "numeric",
    month: "short",
    day: "numeric",
  }).format(new Date(timestamp * 1_000));
}
