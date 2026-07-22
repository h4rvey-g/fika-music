import { describe, expect, it } from "vitest";
import {
  DEFAULT_LIBRARY_PREFERENCES,
  LIBRARY_COLUMN_DEFAULTS,
  loadLibraryPreferences,
  parseLibraryPreferences,
  saveLibraryPreferences,
} from "./library-preferences";

describe("library preferences", () => {
  it("adds newly introduced columns without discarding the stored order", () => {
    const preferences = parseLibraryPreferences({
      columns: [
        { id: "artist", visible: true, width: 200 },
        { id: "title", visible: true, width: 300 },
      ],
      searchFields: ["artist"],
      sortField: "artist",
      sortDirection: "ascending",
    });

    expect(preferences.columns.slice(0, 2).map((column) => column.id)).toEqual([
      "artist",
      "title",
    ]);
    expect(preferences.columns).toHaveLength(LIBRARY_COLUMN_DEFAULTS.length);
  });

  it("clamps unsafe widths and keeps at least one data column visible", () => {
    const preferences = parseLibraryPreferences({
      columns: LIBRARY_COLUMN_DEFAULTS.map((column) => ({
        ...column,
        visible: column.id === "playing",
        width: column.id === "title" ? 50_000 : -10,
      })),
    });

    expect(preferences.columns.find((column) => column.id === "title")).toEqual(
      expect.objectContaining({ visible: true, width: 640 }),
    );
    expect(preferences.columns.every((column) => column.width >= 36)).toBe(true);
  });

  it("falls back from malformed storage without throwing", () => {
    const storage = {
      getItem: () => "{not-json",
    };

    expect(loadLibraryPreferences(storage)).toEqual(DEFAULT_LIBRARY_PREFERENCES);
  });

  it("persists a validated preference payload", () => {
    let stored = "";
    saveLibraryPreferences(DEFAULT_LIBRARY_PREFERENCES, {
      setItem: (_key, value) => {
        stored = value;
      },
    });

    expect(JSON.parse(stored).searchFields).toEqual(["title", "artist", "album"]);
  });
});
