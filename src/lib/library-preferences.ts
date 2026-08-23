import type {
  LibrarySortDirection,
  LibrarySortField,
  LibraryTextField,
} from "../generated/bindings";

export type LibraryColumnId =
  | "playing"
  | "title"
  | "artist"
  | "album"
  | "albumArtist"
  | "genre"
  | "year"
  | "codec"
  | "bitrateKbps"
  | "sampleRateHz"
  | "durationSeconds"
  | "trackNumber"
  | "discNumber"
  | "fileName"
  | "filePath"
  | "fileSizeBytes"
  | "modifiedAt"
  | "indexedAt"
  | "playCount"
  | "rating";

export type LibraryColumnPreference = {
  id: LibraryColumnId;
  visible: boolean;
  width: number;
};

export type LibraryPreferences = {
  columns: LibraryColumnPreference[];
  searchFields: LibraryTextField[];
  sortField: LibrarySortField;
  sortDirection: LibrarySortDirection;
};

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export const LIBRARY_PREFERENCES_STORAGE_KEY = "fika.library-preferences.v1";

export const LIBRARY_COLUMN_DEFAULTS: ReadonlyArray<LibraryColumnPreference> = [
  { id: "playing", visible: true, width: 36 },
  { id: "title", visible: true, width: 200 },
  { id: "artist", visible: true, width: 140 },
  { id: "album", visible: false, width: 180 },
  { id: "trackNumber", visible: true, width: 56 },
  { id: "year", visible: false, width: 62 },
  { id: "durationSeconds", visible: true, width: 68 },
  { id: "rating", visible: true, width: 92 },
  { id: "playCount", visible: true, width: 70 },
  { id: "albumArtist", visible: false, width: 180 },
  { id: "genre", visible: false, width: 140 },
  { id: "discNumber", visible: false, width: 64 },
  { id: "codec", visible: false, width: 78 },
  { id: "bitrateKbps", visible: false, width: 96 },
  { id: "sampleRateHz", visible: false, width: 104 },
  { id: "fileName", visible: false, width: 220 },
  { id: "filePath", visible: false, width: 360 },
  { id: "fileSizeBytes", visible: false, width: 96 },
  { id: "modifiedAt", visible: false, width: 150 },
  { id: "indexedAt", visible: false, width: 150 },
];

export const DEFAULT_LIBRARY_PREFERENCES: LibraryPreferences = {
  columns: LIBRARY_COLUMN_DEFAULTS.map((column) => ({ ...column })),
  searchFields: ["title", "artist", "album"],
  sortField: "relevance",
  sortDirection: "descending",
};

const textFields = new Set<LibraryTextField>([
  "title",
  "artist",
  "album",
  "albumArtist",
  "genre",
  "codec",
  "fileName",
  "filePath",
]);

const sortFields = new Set<LibrarySortField>([
  "relevance",
  "title",
  "artist",
  "album",
  "albumArtist",
  "genre",
  "year",
  "codec",
  "bitrateKbps",
  "sampleRateHz",
  "durationSeconds",
  "trackNumber",
  "discNumber",
  "fileName",
  "filePath",
  "fileSizeBytes",
  "modifiedAt",
  "indexedAt",
  "playCount",
  "rating",
]);

export function loadLibraryPreferences(
  storage: ReadableStorage | null = browserStorage(),
): LibraryPreferences {
  if (!storage) {
    return cloneDefaults();
  }

  try {
    const stored = storage.getItem(LIBRARY_PREFERENCES_STORAGE_KEY);
    return stored ? parseLibraryPreferences(JSON.parse(stored)) : cloneDefaults();
  } catch {
    return cloneDefaults();
  }
}

export function saveLibraryPreferences(
  preferences: LibraryPreferences,
  storage: WritableStorage | null = browserStorage(),
) {
  if (!storage) {
    return;
  }
  try {
    storage.setItem(
      LIBRARY_PREFERENCES_STORAGE_KEY,
      JSON.stringify(parseLibraryPreferences(preferences)),
    );
  } catch {
    // A storage failure must not make the library unusable.
  }
}

export function parseLibraryPreferences(value: unknown): LibraryPreferences {
  const candidate = value && typeof value === "object"
    ? (value as Partial<LibraryPreferences>)
    : {};
  const storedColumns = Array.isArray(candidate.columns) ? candidate.columns : [];
  const columnsById = new Map(
    storedColumns
      .filter((column): column is LibraryColumnPreference => Boolean(column && typeof column === "object"))
      .map((column) => [column.id, column]),
  );
  const orderedIds = storedColumns
    .map((column) => column?.id)
    .filter((id): id is LibraryColumnId => LIBRARY_COLUMN_DEFAULTS.some((column) => column.id === id));
  for (const column of LIBRARY_COLUMN_DEFAULTS) {
    if (!orderedIds.includes(column.id)) {
      orderedIds.push(column.id);
    }
  }

  const columns = orderedIds.map((id) => {
    const defaults = LIBRARY_COLUMN_DEFAULTS.find((column) => column.id === id)!;
    const stored = columnsById.get(id);
    return {
      id,
      visible: typeof stored?.visible === "boolean" ? stored.visible : defaults.visible,
      width: clampWidth(stored?.width, defaults.width),
    };
  });
  if (!columns.some((column) => column.visible && column.id !== "playing")) {
    const title = columns.find((column) => column.id === "title");
    if (title) {
      title.visible = true;
    }
  }

  const searchFields = Array.isArray(candidate.searchFields)
    ? candidate.searchFields.filter((field): field is LibraryTextField => textFields.has(field as LibraryTextField))
    : [];
  const uniqueSearchFields = [...new Set(searchFields)];

  return {
    columns,
    searchFields: uniqueSearchFields.length
      ? uniqueSearchFields
      : [...DEFAULT_LIBRARY_PREFERENCES.searchFields],
    sortField: sortFields.has(candidate.sortField as LibrarySortField)
      ? (candidate.sortField as LibrarySortField)
      : DEFAULT_LIBRARY_PREFERENCES.sortField,
    sortDirection:
      candidate.sortDirection === "ascending" || candidate.sortDirection === "descending"
        ? candidate.sortDirection
        : DEFAULT_LIBRARY_PREFERENCES.sortDirection,
  };
}

function clampWidth(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.round(Math.min(640, Math.max(36, value)))
    : fallback;
}

function cloneDefaults(): LibraryPreferences {
  return {
    ...DEFAULT_LIBRARY_PREFERENCES,
    columns: DEFAULT_LIBRARY_PREFERENCES.columns.map((column) => ({ ...column })),
    searchFields: [...DEFAULT_LIBRARY_PREFERENCES.searchFields],
  };
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
